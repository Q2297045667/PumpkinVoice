use bytes::{BufMut, BytesMut};
use std::net::UdpSocket;

use crate::net::voice_packets::VoicePacket;
use crate::state::Secret;
use crate::util::buf_ext::BufMutExt;

pub fn send_packet(
    socket: &UdpSocket,
    target: std::net::SocketAddr,
    packet: VoicePacket,
    secret: &Secret,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut inner_buf = BytesMut::new();
    inner_buf.put_u8(packet.get_type_id());
    match &packet {
        VoicePacket::AuthenticateAck(p) => p.to_bytes(&mut inner_buf),
        VoicePacket::ConnectionCheckAck(p) => p.to_bytes(&mut inner_buf),
        VoicePacket::PlayerSound(p) => p.to_bytes(&mut inner_buf),
        VoicePacket::GroupSound(p) => p.to_bytes(&mut inner_buf),
        VoicePacket::LocationSound(p) => p.to_bytes(&mut inner_buf),
        VoicePacket::Ping(p) => p.to_bytes(&mut inner_buf),
        VoicePacket::KeepAlive(p) => p.to_bytes(&mut inner_buf),
        _ => {} // Other packets
    }

    let encrypted = match secret.encrypt(&inner_buf) {
        Ok(enc) => enc,
        Err(e) => {
            tracing::error!(
                "{}",
                crate::i18n::translate_str_with(
                    crate::i18n::default_locale(),
                    "log.udp.encrypt_failed",
                    &[packet.get_type_id().to_string(), e.to_string()],
                )
            );
            return Err(crate::i18n::translate_str(
                crate::i18n::default_locale(),
                "error.udp.encryption",
            )
            .into());
        }
    };

    let mut final_buf = BytesMut::new();
    final_buf.put_u8(0xFF);
    final_buf.put_varint(encrypted.len() as i32);
    final_buf.put_slice(&encrypted);

    if let Err(e) = socket.send_to(&final_buf, target) {
        tracing::error!(
            "{}",
            crate::i18n::translate_str_with(
                crate::i18n::default_locale(),
                "log.udp.send_failed",
                &[target.to_string(), e.to_string()],
            )
        );
        return Err(e.into());
    }
    Ok(())
}
