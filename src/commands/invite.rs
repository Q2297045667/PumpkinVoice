use crate::state::StateManager;
use pumpkin_plugin_api::{
    Server,
    command::{CommandError, CommandSender, ConsumedArgs},
    command_wit::Arg,
    commands::CommandHandler,
    text::TextComponent,
};
use std::sync::Arc;

pub struct InviteCommandExecutor {
    pub state_manager: Arc<StateManager>,
}

impl CommandHandler for InviteCommandExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let players = match args.get_value("target") {
            Arg::Players(p) => p,
            _ => return Err(CommandError::InvalidConsumption(Some("target".to_string()))),
        };

        let source_player = match sender.as_player() {
            Some(p) => p,
            None => {
                return Err(CommandError::CommandFailed(crate::i18n::tr(
                    crate::i18n::default_locale(),
                    "command.invite.only_player",
                )));
            }
        };

        let source_locale = source_player.get_locale();

        if !source_player.has_permission("pumpkin_voice:groups") {
            sender.send_message(crate::i18n::tr(
                &source_locale,
                "command.join.no_permission",
            ));
            return Ok(1);
        }

        let source_uuid = crate::util::wit_uuid_to_uuid(source_player.get_id());

        if let Some(player_state) = self.state_manager.get_player_sync(&source_uuid) {
            if let Some(group_id) = player_state.group {
                if let Some(group) = self.state_manager.get_group_sync(&group_id) {
                    let pwd_suffix = group
                        .password
                        .as_ref()
                        .map(|p| format!(" {}", p))
                        .unwrap_or_default();

                    for target_player in players {
                        // The invite text is resolved in the *target's* locale.
                        let target_locale = target_player.get_locale();
                        target_player.send_system_message(
                            crate::i18n::tr_with(
                                &target_locale,
                                "command.invite.message",
                                vec![
                                    TextComponent::text(&source_player.get_name()),
                                    TextComponent::text(&group.name),
                                    TextComponent::text(&group.id.to_string()),
                                    TextComponent::text(&pwd_suffix),
                                ],
                            ),
                            false,
                        );
                    }
                    sender.send_message(crate::i18n::tr(&source_locale, "command.invite.sent"));
                }
            } else {
                sender.send_message(crate::i18n::tr(
                    &source_locale,
                    "command.invite.not_in_group",
                ));
            }
        }

        Ok(1)
    }
}
