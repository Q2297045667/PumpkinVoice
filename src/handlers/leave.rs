use crate::net::custom_payloads::{
    REMOVE_STATE_CHANNEL, RemoveGroupPacket, RemovePlayerStatePacket,
};
use crate::state::StateManager;
use pumpkin_plugin_api::{
    Server,
    events::{EventData, EventHandler, PlayerLeaveEvent},
};
use std::sync::Arc;
use tracing::info;

pub struct LeaveHandler {
    pub state_manager: Arc<StateManager>,
}

impl EventHandler<PlayerLeaveEvent> for LeaveHandler {
    fn handle(
        &self,
        server: Server,
        event: EventData<PlayerLeaveEvent>,
    ) -> EventData<PlayerLeaveEvent> {
        let player = &event.player;
        let uuid = crate::util::wit_uuid_to_uuid(player.get_id());

        let state_manager = self.state_manager.clone();
        state_manager.rate_limiter.on_player_logged_out(uuid);
        let all_clients = server.get_all_players();

        let old_group = state_manager.get_player_sync(&uuid).and_then(|p| p.group);

        // A player leaving the Minecraft server removes the entry entirely;
        // `voicechat:state(disconnected=true)` is reserved for a voice-only
        // disconnect while the player remains online.
        let remove_state = RemovePlayerStatePacket { player_uuid: uuid }.to_bytes();
        for client in &all_clients {
            if crate::util::wit_uuid_to_uuid(client.get_id()) != uuid
                && let Some(java_player) = client.as_java()
            {
                java_player.send_custom_payload(REMOVE_STATE_CHANNEL, &remove_state);
            }
        }

        // Remove player from state manager when they disconnect
        state_manager.remove_player_sync(&uuid);

        if let Some(old_id) = old_group
            && state_manager.remove_if_empty_sync(&old_id)
        {
            let rm_packet = RemoveGroupPacket { group: old_id };
            let rm_bytes = rm_packet.to_bytes();
            for client in &all_clients {
                if let Some(java_player) = client.as_java() {
                    java_player.send_custom_payload("voicechat:remove_group", &rm_bytes);
                }
            }
        }

        info!(
            "{}",
            crate::i18n::translate_str_with(
                crate::i18n::default_locale(),
                "log.player.state_removed",
                &[uuid.to_string()],
            )
        );
        event
    }
}
