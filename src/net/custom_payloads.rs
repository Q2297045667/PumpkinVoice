use bytes::{BufMut, BytesMut};
use uuid::Uuid;

use crate::state::Secret;
use crate::util::buf_ext::BufMutExt;

pub const SECRET_CHANNEL: &str = "voicechat:secret";
pub const REQUEST_SECRET_CHANNEL: &str = "voicechat:request_secret";
pub const REMOVE_STATE_CHANNEL: &str = "voicechat:remove_state";
pub const PLUGIN_MESSAGE_PORT: i32 = 24454;

pub struct SecretPacket {
    pub secret: Secret,
    pub server_port: i32,
    pub player_uuid: Uuid,
    pub codec: u8,
    pub mtu_size: i32,
    pub distance: f64,
    pub keep_alive: i32,
    pub groups_enabled: bool,
    pub voice_host: String,
    pub allow_recording: bool,
}

impl SecretPacket {
    #[must_use]
    pub fn from_config(
        secret: Secret,
        player_uuid: Uuid,
        config: &crate::config::VoicechatConfig,
    ) -> Self {
        let codec = match config.codec.as_str() {
            "VOIP" => 0,
            "AUDIO" => 1,
            "RESTRICTED_LOWDELAY" => 2,
            _ => 0,
        };
        let server_port = if config.port == -1 {
            PLUGIN_MESSAGE_PORT
        } else {
            config.port
        };

        Self {
            secret,
            server_port,
            player_uuid,
            codec,
            mtu_size: config.mtu_size,
            distance: config.max_voice_distance,
            keep_alive: config.keep_alive,
            groups_enabled: config.enable_groups,
            voice_host: config.voice_host.clone(),
            allow_recording: config.allow_recording,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::new();
        // Secret mapped as UUID bytes
        buf.put_slice(&self.secret.to_bytes());
        buf.put_i32(self.server_port);
        buf.put_uuid(self.player_uuid);
        buf.put_u8(self.codec);
        buf.put_i32(self.mtu_size);
        buf.put_f64(self.distance);
        buf.put_i32(self.keep_alive);
        buf.put_u8(if self.groups_enabled { 1 } else { 0 });

        buf.put_string(&self.voice_host);

        buf.put_u8(if self.allow_recording { 1 } else { 0 });
        buf.to_vec()
    }
}

pub struct RequestSecretPacket {
    pub compatibility_version: i32,
}

impl RequestSecretPacket {
    #[must_use]
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        let bytes: [u8; 4] = data.get(..4)?.try_into().ok()?;
        Some(Self {
            compatibility_version: i32::from_be_bytes(bytes),
        })
    }
}

pub struct CreateGroupPacket {
    pub name: String,
    pub password: Option<String>,
    pub group_type: i16,
}

pub struct JoinGroupPacket {
    pub group: Uuid,
    pub password: Option<String>,
}

pub struct AddGroupPacket<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub password: bool,
    pub persistent: bool,
    pub hidden: bool,
    pub group_type: i16,
}

impl<'a> AddGroupPacket<'a> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::new();
        buf.put_uuid(self.id);
        buf.put_string(self.name);
        buf.put_u8(if self.password { 1 } else { 0 });
        buf.put_u8(if self.persistent { 1 } else { 0 });
        buf.put_u8(if self.hidden { 1 } else { 0 });
        buf.put_i16(self.group_type);
        buf.to_vec()
    }
}

pub struct RemoveGroupPacket {
    pub group: Uuid,
}

pub struct RemovePlayerStatePacket {
    pub player_uuid: Uuid,
}

impl RemovePlayerStatePacket {
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::new();
        buf.put_uuid(self.player_uuid);
        buf.to_vec()
    }
}

impl RemoveGroupPacket {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::new();
        buf.put_uuid(self.group);
        buf.to_vec()
    }
}

pub struct JoinedGroupPacket {
    pub group: Option<Uuid>,
    pub wrong_password: bool,
}

impl JoinedGroupPacket {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::new();
        if let Some(uuid) = self.group {
            buf.put_u8(1);
            buf.put_uuid(uuid);
        } else {
            buf.put_u8(0);
        }
        buf.put_u8(if self.wrong_password { 1 } else { 0 });
        buf.to_vec()
    }
}

pub struct PlayerStatePacket<'a> {
    pub player_state: &'a crate::state::PlayerState,
}

impl<'a> PlayerStatePacket<'a> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::new();
        let state = self.player_state;
        buf.put_u8(if state.disabled { 1 } else { 0 });
        buf.put_u8(if state.disconnected { 1 } else { 0 });
        buf.put_uuid(state.uuid);
        buf.put_string(&state.name);

        if let Some(group) = state.group {
            buf.put_u8(1);
            buf.put_uuid(group);
        } else {
            buf.put_u8(0);
        }
        buf.to_vec()
    }
}

pub struct VolumeCategory {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

pub struct AddCategoryPacket<'a> {
    pub category: &'a VolumeCategory,
}

impl<'a> AddCategoryPacket<'a> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::new();
        // ID (up to 16 chars)
        buf.put_string(&self.category.id);
        // Name (up to 16 chars)
        buf.put_string(&self.category.name);

        // nameTranslationKey optional
        buf.put_u8(0);

        // description optional
        if let Some(desc) = &self.category.description {
            buf.put_u8(1);
            buf.put_string(desc);
        } else {
            buf.put_u8(0);
        }

        // descriptionTranslationKey optional
        buf.put_u8(0);

        // icon missing
        buf.put_u8(0);

        buf.to_vec()
    }
}

pub struct RemoveCategoryPacket<'a> {
    pub category_id: &'a str,
}

impl<'a> RemoveCategoryPacket<'a> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::new();
        buf.put_string(self.category_id);
        buf.to_vec()
    }
}

pub struct PlayerStatesPacket<'a> {
    pub player_states: &'a [crate::state::PlayerState],
}

impl<'a> PlayerStatesPacket<'a> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::new();
        buf.put_i32(self.player_states.len() as i32);

        for state in self.player_states {
            buf.put_u8(if state.disabled { 1 } else { 0 });
            buf.put_u8(if state.disconnected { 1 } else { 0 });
            buf.put_uuid(state.uuid);
            buf.put_string(&state.name);

            if let Some(group) = state.group {
                buf.put_u8(1);
                buf.put_uuid(group);
            } else {
                buf.put_u8(0);
            }
        }

        buf.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AddGroupPacket, PLUGIN_MESSAGE_PORT, RemovePlayerStatePacket, RequestSecretPacket,
        SecretPacket,
    };
    use crate::{config::VoicechatConfig, state::Secret};
    use uuid::Uuid;

    #[test]
    fn request_secret_reads_the_big_endian_compatibility_version() {
        assert_eq!(
            RequestSecretPacket::from_bytes(&20_i32.to_be_bytes())
                .expect("four bytes should form a request")
                .compatibility_version,
            20
        );
        assert!(RequestSecretPacket::from_bytes(&[0, 0, 0]).is_none());
    }

    #[test]
    fn secret_replies_reuse_the_configured_wire_settings() {
        let config = VoicechatConfig {
            port: -1,
            codec: "AUDIO".to_string(),
            voice_host: "voice.example.test".to_string(),
            ..VoicechatConfig::default()
        };
        let secret = Secret::from_bytes([7; 16]);
        let player_uuid = Uuid::new_v4();

        let packet = SecretPacket::from_config(secret, player_uuid, &config);

        assert_eq!(packet.server_port, PLUGIN_MESSAGE_PORT);
        assert_eq!(packet.player_uuid, player_uuid);
        assert_eq!(packet.codec, 1);
        assert_eq!(packet.voice_host, "voice.example.test");
        assert_eq!(packet.keep_alive, config.keep_alive);
    }

    #[test]
    fn remove_state_packet_is_exactly_one_uuid() {
        let player_uuid = Uuid::from_u128(0x0011_2233_4455_6677_8899_aabb_ccdd_eeff);
        let bytes = RemovePlayerStatePacket { player_uuid }.to_bytes();

        assert_eq!(bytes, player_uuid.as_bytes());
    }

    #[test]
    fn add_group_packet_preserves_all_group_metadata() {
        let id = Uuid::nil();
        let bytes = AddGroupPacket {
            id,
            name: "g",
            password: true,
            persistent: false,
            hidden: true,
            group_type: 2,
        }
        .to_bytes();

        let mut expected = vec![0; 16];
        expected.extend_from_slice(&[1, b'g', 1, 0, 1, 0, 2]);
        assert_eq!(bytes, expected);
    }
}
