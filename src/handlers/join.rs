use crate::net::custom_payloads::{
    AddGroupPacket, PlayerStatePacket, PlayerStatesPacket, SECRET_CHANNEL, SecretPacket,
};
use crate::state::StateManager;
use pumpkin_plugin_api::{
    Server,
    events::{EventData, EventHandler, PlayerJoinEvent},
    player::{BedrockDisconnectReason, BedrockKickOptions, JavaKickOptions},
    scheduler::SchedulerExt,
};
use std::sync::Arc;

pub struct JoinHandler {
    pub state_manager: Arc<StateManager>,
}

impl EventHandler<PlayerJoinEvent> for JoinHandler {
    fn handle(
        &self,
        server: Server,
        event: EventData<PlayerJoinEvent>,
    ) -> EventData<PlayerJoinEvent> {
        let player = &event.player;
        let player_api_uuid = player.get_id();
        let uuid = crate::util::wit_uuid_to_uuid(player_api_uuid);
        let name = player.get_name();

        let state_manager = self.state_manager.clone();
        // Sending payloads can synchronously re-enter the plugin on the current API.
        let config = crate::config::CONFIG.read().unwrap().clone();

        // Add player to state manager and generate secret
        let secret = state_manager.add_player_sync(uuid, name);

        let secret_packet = SecretPacket::from_config(secret, uuid, &config);

        let bytes = secret_packet.to_bytes();
        if let Some(java_player) = player.as_java() {
            java_player.send_custom_payload(SECRET_CHANNEL, &bytes);
        }
        tracing::info!(
            "{}",
            crate::i18n::translate_str_with(
                crate::i18n::default_locale(),
                "log.player.secret_sent",
                &[uuid.to_string()],
            )
        );

        if config.force_voice_chat {
            let sm_clone = state_manager.clone();
            let player_id = player_api_uuid;
            let timeout_ticks = (config.login_timeout / 50) as u64; // 50ms per tick

            server.schedule_delayed_task(timeout_ticks, move |server| {
                if let Some(state) = sm_clone.get_player_sync(&uuid)
                    && state.socket_addr.is_none()
                    && let Some(p) = server.get_player_by_uuid(player_id)
                {
                    const REASON_KEY: &str = "kick.voice_chat_required";
                    let locale = p.get_locale();

                    if let Some(java_player) = p.as_java() {
                        java_player
                            .kick(JavaKickOptions::new(crate::i18n::tr(&locale, REASON_KEY)));
                    } else if let Some(bedrock_player) = p.as_bedrock() {
                        let reason = crate::i18n::translate_str(&locale, REASON_KEY);
                        bedrock_player.kick(&BedrockKickOptions::new(
                            BedrockDisconnectReason::Kicked,
                            reason.as_str(),
                        ));
                    }
                }
            });
        }

        // Send all current groups to the new player
        let all_groups = state_manager.get_all_groups_sync();
        for group in all_groups {
            let add_packet = AddGroupPacket {
                id: group.id,
                name: &group.name,
                password: group.password.is_some(),
                persistent: group.persistent,
                hidden: group.hidden,
                group_type: group.group_type.to_wire(),
            };
            if let Some(java_player) = player.as_java() {
                java_player.send_custom_payload("voicechat:add_group", &add_packet.to_bytes());
            }
        }

        // Send all current categories to the new player
        let all_cats = state_manager.get_categories_sync();
        for cat in &all_cats {
            let cat_packet = crate::net::AddCategoryPacket { category: cat };
            if let Some(java_player) = player.as_java() {
                java_player.send_custom_payload("voicechat:add_category", &cat_packet.to_bytes());
            }
        }

        // Send all current player states to the new player
        let all_players = state_manager.get_all_players_sync();
        let states_packet = PlayerStatesPacket {
            player_states: &all_players,
        };
        if let Some(java_player) = player.as_java() {
            java_player.send_custom_payload("voicechat:states", &states_packet.to_bytes());
        }

        // Broadcast the new player's state to everyone else
        let new_state = state_manager.get_player_sync(&uuid).unwrap();
        let bc_packet = PlayerStatePacket {
            player_state: &new_state,
        };
        let bc_bytes = bc_packet.to_bytes();

        let all_clients = server.get_all_players();
        for client in all_clients {
            if crate::util::wit_uuid_to_uuid(client.get_id()) != uuid
                && let Some(java_player) = client.as_java()
            {
                java_player.send_custom_payload("voicechat:state", &bc_bytes);
            }
        }

        event
    }
}
