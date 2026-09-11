use std::net::UdpSocket;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

use super::crypto::send_packet;
use crate::net::voice_packets::{
    AuthenticateAckPacket, AuthenticatePacket, ConnectionCheckAckPacket,
    MAX_VOICE_CHAT_PACKET_SIZE, MicPacket, PingPacket, VoicePacket,
};
use crate::state::StateManager;
use crate::util::payload_reader::PayloadReader;
use pumpkin_plugin_api::Server;

pub struct UdpServer {
    state_manager: Arc<StateManager>,
    socket: UdpSocket,
}

impl UdpServer {
    pub fn new(state_manager: Arc<StateManager>, addr: &str) -> Result<Self, std::io::Error> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        info!(
            "{}",
            crate::i18n::translate_str_with(
                crate::i18n::default_locale(),
                "log.udp.initialized",
                &[addr.to_string()],
            )
        );
        Ok(Self {
            state_manager,
            socket,
        })
    }

    pub fn poll(&self, server: &Server) {
        let mut buf = [0u8; 4096];
        loop {
            match self.socket.recv_from(&mut buf) {
                Ok((len, src)) => {
                    self.handle_packet(server, &buf[..len], src);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    break;
                }
                Err(ref e) => {
                    let msg = e.to_string();
                    if e.kind() == std::io::ErrorKind::ConnectionReset
                        || e.kind() == std::io::ErrorKind::Interrupted
                        || msg.contains("reset")
                        || msg.contains("15")
                    {
                        continue;
                    }
                    error!(
                        "{}",
                        crate::i18n::translate_str_with(
                            crate::i18n::default_locale(),
                            "log.udp.receive_failed",
                            &[e.to_string(), format!("{:?}", e.kind())],
                        )
                    );
                    break;
                }
            }
        }
    }

    pub fn send_keep_alives(&self, server: &Server) {
        let keep_alive_millis = crate::config::CONFIG.read().unwrap().keep_alive.max(1) as u64;
        let timeout = std::time::Duration::from_millis(keep_alive_millis.saturating_mul(10));

        for state in self.state_manager.expire_voice_connections_sync(timeout) {
            info!(
                "{}",
                crate::i18n::translate_str_with(
                    crate::i18n::default_locale(),
                    "log.udp.timed_out",
                    &[state.uuid.to_string()],
                )
            );
            crate::net::sync::broadcast_player_state(server, &self.state_manager, &state);

            if let Some(player) =
                server.get_player_by_uuid(crate::util::uuid_to_wit_uuid(state.uuid))
                && self.state_manager.is_client_compatible_sync(
                    &state.uuid,
                    crate::net::custom_payloads::VOICECHAT_COMPATIBILITY_VERSION,
                )
            {
                crate::net::sync::send_full_sync(&player, &self.state_manager);
                crate::net::sync::send_secret(&player, &self.state_manager);
            }
        }

        let targets = self.state_manager.get_keep_alive_targets_sync();
        for (target, secret) in targets {
            let _ = send_packet(
                &self.socket,
                target,
                VoicePacket::KeepAlive(crate::net::voice_packets::KeepAlivePacket),
                &secret,
            );
        }
    }

    fn handle_packet(&self, server: &Server, data: &[u8], src: std::net::SocketAddr) {
        // Snapshot only the hot-path values needed below. Server/player host calls may
        // synchronously re-enter the plugin, so no configuration lock can span them.
        let (
            spectator_interaction,
            whisper_distance,
            max_voice_distance,
            broadcast_range,
            allow_pings,
        ) = {
            let config = crate::config::CONFIG.read().unwrap();
            (
                config.spectator_interaction,
                config.whisper_distance,
                config.max_voice_distance,
                config.broadcast_range,
                config.allow_pings,
            )
        };
        if data.len() < 17 {
            return;
        }
        if data[0] != 0xFF {
            return;
        }

        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&data[1..17]);
        let player_id = Uuid::from_bytes(uuid_bytes);

        if let Some(player_state) = self.state_manager.get_player_sync(&player_id) {
            let mut payload_reader = PayloadReader::new(&data[17..]);
            let Some(payload_bytes) = payload_reader.read_byte_array(MAX_VOICE_CHAT_PACKET_SIZE)
            else {
                tracing::debug!(
                    "{}",
                    crate::i18n::translate_str_with(
                        crate::i18n::default_locale(),
                        "log.udp.invalid_payload",
                        &[player_id.to_string(), data.len().to_string()],
                    )
                );
                return;
            };
            if !payload_reader.is_finished() {
                tracing::debug!(
                    "{}",
                    crate::i18n::translate_str_with(
                        crate::i18n::default_locale(),
                        "log.udp.invalid_payload",
                        &[player_id.to_string(), data.len().to_string()],
                    )
                );
                return;
            }

            match player_state.secret.decrypt(&payload_bytes) {
                Ok(decrypted) => {
                    if decrypted.is_empty() {
                        return;
                    }

                    if !self.state_manager.rate_limiter.allow(player_id) {
                        return;
                    }

                    let packet_type = decrypted[0];
                    let packet_data = &decrypted[1..];

                    match packet_type {
                        0x1 => {
                            if player_state.socket_addr != Some(src) || player_state.disconnected {
                                return;
                            }
                            let Some(mic_packet) = MicPacket::from_bytes(packet_data) else {
                                return;
                            };
                            tracing::debug!(
                                "{}",
                                crate::i18n::translate_str_with(
                                    crate::i18n::default_locale(),
                                    "log.udp.mic_received",
                                    &[
                                        player_id.to_string(),
                                        mic_packet.sequence_number.to_string(),
                                        mic_packet.data.len().to_string(),
                                    ],
                                )
                            );
                            let all_players = self.state_manager.get_all_players_sync();

                            let sender_pl = match server
                                .get_player_by_uuid(crate::util::uuid_to_wit_uuid(player_id))
                            {
                                Some(p) => p,
                                None => {
                                    tracing::debug!(
                                        "{}",
                                        crate::i18n::translate_str_with(
                                            crate::i18n::default_locale(),
                                            "log.udp.player_not_found",
                                            &[player_id.to_string()],
                                        )
                                    );
                                    return;
                                }
                            };

                            if !sender_pl.has_permission("pumpkin_voice:speak") {
                                tracing::debug!(
                                    "{}",
                                    crate::i18n::translate_str_with(
                                        crate::i18n::default_locale(),
                                        "log.udp.speak_permission_missing",
                                        &[player_id.to_string()],
                                    )
                                );
                                return;
                            }

                            // Spectator check
                            let is_spectator = matches!(
                                sender_pl.get_gamemode(),
                                pumpkin_plugin_api::common::GameMode::Spectator
                            );

                            let sender_group = player_state
                                .group
                                .and_then(|group_id| self.state_manager.get_group_sync(&group_id));

                            if let Some(group_id) = player_state.group {
                                let group_packet = crate::net::voice_packets::GroupSoundPacket {
                                    channel_id: group_id,
                                    sender: player_id,
                                    data: mic_packet.data.clone(),
                                    sequence_number: mic_packet.sequence_number,
                                    category: None,
                                };
                                for receiver in &all_players {
                                    if receiver.uuid == player_id
                                        || !receiver_accepts_audio(
                                            receiver.disabled,
                                            receiver.disconnected,
                                        )
                                    {
                                        continue;
                                    }
                                    if receiver.group == Some(group_id)
                                        && let Some(addr) = receiver.socket_addr
                                        && let Some(recv_pl) = server.get_player_by_uuid(
                                            crate::util::uuid_to_wit_uuid(receiver.uuid),
                                        )
                                    {
                                        if recv_pl.has_permission("pumpkin_voice:listen") {
                                            let _ = send_packet(
                                                &self.socket,
                                                addr,
                                                VoicePacket::GroupSound(group_packet.clone()),
                                                &receiver.secret,
                                            );
                                        } else {
                                            tracing::debug!(
                                                "{}",
                                                crate::i18n::translate_str_with(
                                                    crate::i18n::default_locale(),
                                                    "log.udp.listen_permission_missing",
                                                    &[receiver.uuid.to_string()],
                                                )
                                            );
                                        }
                                    }
                                }
                            }

                            if should_route_proximity(sender_group.as_ref())
                                && (!is_spectator || spectator_interaction)
                            {
                                let pos_a = sender_pl.get_position();
                                let distance_config = if mic_packet.whispering {
                                    whisper_distance
                                } else {
                                    max_voice_distance
                                };

                                let broadcast_range = if broadcast_range < 0.0 {
                                    max_voice_distance + 1.0
                                } else {
                                    broadcast_range
                                }
                                .max(distance_config);

                                let distance_sq = broadcast_range.powi(2);
                                let proximity_packet = if is_spectator {
                                    let eye_pos = sender_pl.as_entity().get_eye_position();
                                    VoicePacket::LocationSound(
                                        crate::net::voice_packets::LocationSoundPacket {
                                            channel_id: player_id,
                                            sender: player_id,
                                            location: [eye_pos.0, eye_pos.1, eye_pos.2],
                                            data: mic_packet.data.clone(),
                                            sequence_number: mic_packet.sequence_number,
                                            distance: distance_config as f32,
                                            category: None,
                                        },
                                    )
                                } else {
                                    VoicePacket::PlayerSound(
                                        crate::net::voice_packets::PlayerSoundPacket {
                                            channel_id: player_id,
                                            sender: player_id,
                                            data: mic_packet.data.clone(),
                                            sequence_number: mic_packet.sequence_number,
                                            distance: distance_config as f32,
                                            whispering: mic_packet.whispering,
                                            category: None,
                                        },
                                    )
                                };

                                for receiver in &all_players {
                                    let receiver_group_type = receiver
                                        .group
                                        .and_then(|group_id| {
                                            self.state_manager.get_group_sync(&group_id)
                                        })
                                        .map(|group| group.group_type);
                                    if receiver.uuid == player_id
                                        || !should_receive_proximity(
                                            player_state.group,
                                            receiver.group,
                                            receiver_group_type,
                                            receiver.disabled,
                                            receiver.disconnected,
                                        )
                                    {
                                        continue;
                                    }
                                    if let Some(addr) = receiver.socket_addr
                                        && let Some(recv_pl) = server.get_player_by_uuid(
                                            crate::util::uuid_to_wit_uuid(receiver.uuid),
                                        )
                                    {
                                        // Same world check
                                        if sender_pl.get_world().get_id()
                                            == recv_pl.get_world().get_id()
                                        {
                                            if recv_pl.has_permission("pumpkin_voice:listen") {
                                                let pos_b = recv_pl.get_position();
                                                let dist = (pos_a.0 - pos_b.0).powi(2)
                                                    + (pos_a.1 - pos_b.1).powi(2)
                                                    + (pos_a.2 - pos_b.2).powi(2);

                                                if dist <= distance_sq {
                                                    let _ = send_packet(
                                                        &self.socket,
                                                        addr,
                                                        proximity_packet.clone(),
                                                        &receiver.secret,
                                                    );
                                                }
                                            } else {
                                                tracing::debug!(
                                                    "{}",
                                                    crate::i18n::translate_str_with(
                                                        crate::i18n::default_locale(),
                                                        "log.udp.listen_permission_missing",
                                                        &[receiver.uuid.to_string()],
                                                    )
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        0x5 => {
                            let Some(auth_packet) = AuthenticatePacket::from_bytes(packet_data)
                            else {
                                return;
                            };
                            if auth_packet.player_uuid == player_id
                                && auth_packet.secret.to_bytes() == player_state.secret.to_bytes()
                            {
                                info!(
                                    "{}",
                                    crate::i18n::translate_str_with(
                                        crate::i18n::default_locale(),
                                        "log.udp.authenticated",
                                        &[auth_packet.player_uuid.to_string()],
                                    )
                                );
                                self.state_manager.authenticate_voice_sync(&player_id, src);

                                let _ = send_packet(
                                    &self.socket,
                                    src,
                                    VoicePacket::AuthenticateAck(AuthenticateAckPacket),
                                    &player_state.secret,
                                );
                            }
                        }
                        0x7 => {
                            if allow_pings
                                && player_state.socket_addr == Some(src)
                                && !player_state.disconnected
                                && let Some(packet) = PingPacket::from_bytes(packet_data)
                            {
                                let _ = send_packet(
                                    &self.socket,
                                    src,
                                    VoicePacket::Ping(packet),
                                    &player_state.secret,
                                );
                            }
                        }
                        0x8 => {
                            if !packet_data.is_empty()
                                || player_state.socket_addr != Some(src)
                                || player_state.disconnected
                            {
                                return;
                            }
                            self.state_manager.record_keep_alive_sync(&player_id, src);
                        }
                        0x9 => {
                            if !packet_data.is_empty() {
                                return;
                            }
                            let authenticated = self
                                .state_manager
                                .get_player_sync(&player_id)
                                .is_some_and(|state| state.socket_addr == Some(src));
                            if !authenticated {
                                return;
                            }
                            info!(
                                "{}",
                                crate::i18n::translate_str_with(
                                    crate::i18n::default_locale(),
                                    "log.udp.validated",
                                    &[player_id.to_string()],
                                )
                            );
                            if let Some(state) = self
                                .state_manager
                                .mark_voice_connected_sync(&player_id, src)
                            {
                                crate::net::sync::broadcast_player_state(
                                    server,
                                    &self.state_manager,
                                    &state,
                                );
                            }
                            let _ = send_packet(
                                &self.socket,
                                src,
                                VoicePacket::ConnectionCheckAck(ConnectionCheckAckPacket),
                                &player_state.secret,
                            );
                        }
                        _ => {
                            tracing::debug!(
                                "{}",
                                crate::i18n::translate_str_with(
                                    crate::i18n::default_locale(),
                                    "log.udp.unknown_packet",
                                    &[packet_type.to_string(), player_id.to_string()],
                                )
                            );
                        }
                    }
                }
                Err(e) => {
                    error!(
                        "{}",
                        crate::i18n::translate_str_with(
                            crate::i18n::default_locale(),
                            "log.udp.decrypt_failed",
                            &[
                                player_id.to_string(),
                                payload_bytes.len().to_string(),
                                e.to_string(),
                            ],
                        )
                    );
                }
            }
        }
    }
}

fn receiver_accepts_audio(disabled: bool, disconnected: bool) -> bool {
    !disabled && !disconnected
}

fn should_route_proximity(sender_group: Option<&crate::state::Group>) -> bool {
    sender_group.is_none_or(|group| group.group_type.is_open())
}

fn should_receive_proximity(
    sender_group: Option<Uuid>,
    receiver_group: Option<Uuid>,
    receiver_group_type: Option<crate::state::GroupType>,
    disabled: bool,
    disconnected: bool,
) -> bool {
    receiver_accepts_audio(disabled, disconnected)
        && !(sender_group.is_some() && sender_group == receiver_group)
        && !receiver_group_type.is_some_and(crate::state::GroupType::is_isolated)
}

#[cfg(test)]
mod tests {
    use super::{receiver_accepts_audio, should_receive_proximity, should_route_proximity};
    use crate::state::{Group, GroupType};
    use uuid::Uuid;

    fn group(group_type: GroupType) -> Group {
        Group {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            password: None,
            persistent: false,
            hidden: false,
            group_type,
        }
    }

    #[test]
    fn only_open_groups_also_route_proximity_audio() {
        assert!(should_route_proximity(None));
        assert!(!should_route_proximity(Some(&group(GroupType::Normal))));
        assert!(should_route_proximity(Some(&group(GroupType::Open))));
        assert!(!should_route_proximity(Some(&group(GroupType::Isolated))));
    }

    #[test]
    fn disabled_or_disconnected_receivers_do_not_accept_audio() {
        assert!(receiver_accepts_audio(false, false));
        assert!(!receiver_accepts_audio(true, false));
        assert!(!receiver_accepts_audio(false, true));
        assert!(!receiver_accepts_audio(true, true));
    }

    #[test]
    fn proximity_skips_same_open_group_and_isolated_receivers() {
        let sender_group = Uuid::new_v4();
        let other_group = Uuid::new_v4();

        assert!(!should_receive_proximity(
            Some(sender_group),
            Some(sender_group),
            Some(GroupType::Open),
            false,
            false,
        ));
        assert!(!should_receive_proximity(
            None,
            Some(other_group),
            Some(GroupType::Isolated),
            false,
            false,
        ));
        assert!(should_receive_proximity(
            None,
            Some(other_group),
            Some(GroupType::Normal),
            false,
            false,
        ));
    }
}
