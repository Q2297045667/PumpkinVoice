use super::secret::Secret;
use uuid::Uuid;

#[derive(Clone)]
pub struct PlayerState {
    pub uuid: Uuid,
    pub name: String,
    pub disconnected: bool,
    pub disabled: bool,
    pub group: Option<Uuid>,
    /// Simple Voice Chat protocol version reported by `request_secret`.
    /// `None` means the client has not completed the compatibility handshake.
    pub compatibility_version: Option<i32>,
    pub secret: Secret,
    pub socket_addr: Option<std::net::SocketAddr>,
    pub last_keep_alive_response: Option<std::time::Instant>,
}
