use crate::state::StateManager;
use pumpkin_plugin_api::{
    Server,
    command::{
        CommandError, CommandSender, CommandSuggestion, CommandSuggestions, ConsumedArgs,
        SuggestionRequest,
    },
    command_wit::Arg,
    commands::{CommandHandler, CommandSuggestionHandler},
};
use std::sync::Arc;

#[derive(Clone)]
pub struct JoinCommandExecutor {
    pub state_manager: Arc<StateManager>,
}

pub struct GroupNameSuggestionProvider {
    pub state_manager: Arc<StateManager>,
}

impl CommandSuggestionHandler for GroupNameSuggestionProvider {
    fn suggest(
        &self,
        _sender: CommandSender,
        _server: Server,
        request: SuggestionRequest,
    ) -> CommandSuggestions {
        let names = suggested_group_arguments(
            &self.state_manager.get_all_groups_sync(),
            request.remaining.as_str(),
        );
        CommandSuggestions {
            start: request.start,
            length: request.remaining.len() as u32,
            values: names
                .into_iter()
                .map(|value| CommandSuggestion {
                    value,
                    tooltip: None,
                })
                .collect(),
        }
    }
}

fn suggested_group_arguments(groups: &[crate::state::Group], remaining: &str) -> Vec<String> {
    let prefix = remaining
        .strip_prefix('"')
        .unwrap_or(remaining)
        .to_lowercase();
    let mut names: Vec<_> = groups
        .iter()
        .filter(|group| !group.hidden && group.name.to_lowercase().starts_with(&prefix))
        .map(|group| group.name.as_str())
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    names.into_iter().map(quote_argument).collect()
}

fn quote_argument(value: &str) -> String {
    if value.chars().any(char::is_whitespace) {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.to_string()
    }
}
impl CommandHandler for JoinCommandExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let group_name = match args.get_value("group_name") {
            Arg::Simple(s) => s,
            _ => {
                return Err(CommandError::InvalidConsumption(Some(
                    "group_name".to_string(),
                )));
            }
        };

        let password = match args.get_value("password") {
            Arg::Simple(s) => Some(s),
            _ => None,
        };

        let player = match sender.as_player() {
            Some(p) => p,
            None => {
                return Err(CommandError::CommandFailed(crate::i18n::tr(
                    crate::i18n::default_locale(),
                    "command.join.only_player",
                )));
            }
        };

        let locale = player.get_locale();

        if !player.has_permission("pumpkin_voice:groups") {
            sender.send_message(crate::i18n::tr(&locale, "command.join.no_permission"));
            return Ok(1);
        }

        let player_uuid = crate::util::wit_uuid_to_uuid(player.get_id());

        // Invitations contain the stable group UUID. Human-entered commands may
        // use the exact group name instead.
        if let Some(group) = self.state_manager.get_group_by_identifier_sync(&group_name) {
            let password_ok = match &group.password {
                None => true,
                Some(expected) => password.as_deref() == Some(expected.as_str()),
            };

            if password_ok {
                let old_group = self
                    .state_manager
                    .get_player_sync(&player_uuid)
                    .and_then(|p| p.group);

                self.state_manager
                    .set_player_group_sync(&player_uuid, Some(group.id));

                let joined_packet = crate::net::JoinedGroupPacket {
                    group: Some(group.id),
                    wrong_password: false,
                };
                if let Some(java_player) = player.as_java() {
                    java_player
                        .send_custom_payload("voicechat:joined_group", &joined_packet.to_bytes());
                }

                if let Some(state) = self.state_manager.get_player_sync(&player_uuid) {
                    let bc_packet = crate::net::PlayerStatePacket {
                        player_state: &state,
                    };
                    let bc_bytes = bc_packet.to_bytes();
                    for client in server.get_all_players() {
                        if crate::util::wit_uuid_to_uuid(client.get_id()) != player_uuid
                            && let Some(java_player) = client.as_java()
                        {
                            java_player.send_custom_payload("voicechat:state", &bc_bytes);
                        }
                    }
                }

                if let Some(old_id) = old_group
                    && self.state_manager.remove_if_empty_sync(&old_id)
                {
                    let rm_packet = crate::net::RemoveGroupPacket { group: old_id };
                    let rm_bytes = rm_packet.to_bytes();
                    for client in server.get_all_players() {
                        if let Some(java_player) = client.as_java() {
                            java_player.send_custom_payload("voicechat:remove_group", &rm_bytes);
                        }
                    }
                }

                sender.send_message(crate::i18n::tr_with(
                    &locale,
                    "command.join.joined",
                    vec![group.name.clone()],
                ));
            } else {
                let joined_packet = crate::net::JoinedGroupPacket {
                    group: None,
                    wrong_password: true,
                };
                if let Some(java_player) = player.as_java() {
                    java_player
                        .send_custom_payload("voicechat:joined_group", &joined_packet.to_bytes());
                }

                let error_key = if password.is_none() {
                    "command.join.missing_password"
                } else {
                    "command.join.incorrect_password"
                };
                sender.send_message(crate::i18n::tr(&locale, error_key));
            }
        } else {
            sender.send_message(crate::i18n::tr(&locale, "command.join.group_not_found"));
        }

        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use super::{quote_argument, suggested_group_arguments};
    use crate::state::{Group, GroupType};
    use uuid::Uuid;

    fn group(name: &str, hidden: bool) -> Group {
        Group {
            id: Uuid::new_v4(),
            name: name.to_string(),
            password: None,
            persistent: false,
            hidden,
            group_type: GroupType::Normal,
        }
    }

    #[test]
    fn suggestions_are_filtered_sorted_quoted_and_hide_hidden_groups() {
        let groups = vec![
            group("Zulu", false),
            group("Alpha Team", false),
            group("alpha", false),
            group("Admin", true),
        ];

        assert_eq!(
            suggested_group_arguments(&groups, "a"),
            vec!["alpha", "\"Alpha Team\""]
        );
        assert_eq!(
            suggested_group_arguments(&groups, "\"alpha"),
            vec!["alpha", "\"Alpha Team\""]
        );
    }

    #[test]
    fn command_arguments_escape_quotes_and_backslashes() {
        assert_eq!(quote_argument("NoSpaces"), "NoSpaces");
        assert_eq!(
            quote_argument("A \"quoted\" group"),
            "\"A \\\"quoted\\\" group\""
        );
        assert_eq!(quote_argument("A \\ group"), "\"A \\\\ group\"");
    }
}
