# Simple Voice Chat for PumpkinMC

This plugin implements the backend compatibility needed to host the [Simple Voice Chat](https://modrinth.com/plugin/simple-voice-chat) mod on a [PumpkinMC](https://github.com/Pumpkin-MC/Pumpkin) server. It allows players connecting with modern Minecraft clients (Fabric, Forge, NeoForge) to use proximity voice chat and dynamically created voice groups.

## Key Features

- **Proximity Chat**: Accurately simulates dimensional audio using 3D vector coordinates sent directly to your game client.
- **Group Channels**: Full support for the GUI group lifecycle (creating, publishing, joining, leaving, and removing empty groups), including passwords and all three upstream group types.
- **Dynamic Audio Categories**: Create custom audio categories via configuration to differentiate audio streams (e.g. Radio, Global Broadcast) with custom names and descriptions.
- **Packet Rate Limiting**: Built-in protection against network flooding/DoS using a high-performance token-bucket rate limiter.
- **Permissions Support**: Fully respects the native PumpkinMC permission node trees.
- **Optimized Transport**: Connects entirely over UDP with lightweight `AES-128-GCM` encryption for optimal performance.

---

## Tech Stack

- **Language**: Rust (edition 2024), compiled to a `wasm32-wasip2` WebAssembly component
- **Framework**: [`pumpkin-plugin-api`](https://github.com/Pumpkin-MC/Pumpkin) (PumpkinMC Plugin SDK)
- **Plugin Interface**: the `pumpkin:plugin@0.1.0` WIT world from [`pumpkin-plugin-wit`](https://github.com/Pumpkin-MC/pumpkin-plugin-wit)
- **Networking**: non-blocking `std::net::UdpSocket` driven by the host scheduler (no async runtime inside the WASI sandbox)
- **Cryptography**: `aes-gcm` (AES-128-GCM) suite for packet serialization matching JVM mod signatures
- **Configuration**: `serde` / `toml`

### Current Dependency / API Pins

| Component | Pinned version |
| --------- | -------------- |
| `pumpkin-plugin-api` | `0.1.0+26.2-26.45` — Pumpkin `master` rev `2c49af7acb9c62a62b5f22251dc2925f00c4edd8` (verified 2026-09-11) |
| WIT interface | `pumpkin:plugin@0.1.0` (`pumpkin-plugin-wit` rev `1ad73fff1e0a9e21b99255816df5f99f6260c1b9`) |
| WASM target | `wasm32-wasip2` |
| Crypto / support crates | `aes-gcm` 0.11, `rand` 0.10, `uuid` 1.26, `bytes` 1.12, `serde` 1.0, `toml` 1.1, `tracing` 0.1, `unicode-general-category` 1.1 |

The API is pinned to the Pumpkin `master` tip verified on 2026-09-11. That Pumpkin revision points its `pumpkin-plugin-wit` submodule at `1ad73fff1e0a9e21b99255816df5f99f6260c1b9`, which is also the current WIT repository `master` tip, so the generated guest bindings match the interface consumed by that server revision. `wit-bindgen` is **not** a direct dependency here: the SDK crate owns the `wit_bindgen::generate!` / `export!` component glue.

---

## Prerequisites

Before setting up the plugin, make sure you have the following installed on your machine:
- The [Rust Toolchain](https://rustup.rs/) (`cargo`, `rustc`).
- A built and running instance of the [PumpkinMC](https://github.com/Pumpkin-MC/Pumpkin) Server.
- A Minecraft Client with the [Simple Voice Chat Mod](https://modrinth.com/plugin/simple-voice-chat/versions) installed.

---

## Getting Started

### Download Pre-Release Binaries

We provide pre-built WASM components under the Releases tab.

1. Download the latest `pumpkin_voice.wasm` from the Releases page.
2. Place the downloaded `.wasm` file directly into your PumpkinMC server's `plugins/` directory.

### Build from Source (Rust)

If you prefer to compile the plugin yourself or are contributing to development:

1. **Clone the Repository**
   ```bash
   git clone https://github.com/hmdnnrmn/PumpkinVoice.git
   cd PumpkinVoice
   ```

2. **Install the WASM Target**
   Ensure you have the WebAssembly target installed:
   ```bash
   rustup target add wasm32-wasip2
   ```

3. **Build the Plugin**
   Compile the plugin to a WASM component:
   ```bash
   cargo build --release --target wasm32-wasip2
   ```

4. **Run the Unit Tests** (optional)
   The test binary is a WASM component that imports the Pumpkin host interfaces, so it cannot be instantiated by a bare `wasmtime`. Run the pure-Rust logic tests on the host target instead:
   ```bash
   cargo test --target x86_64-unknown-linux-gnu
   ```

5. **Deploy the Executable**
   Once compiled, move the output WASM file into your server's plugin pool:
   ```bash
   cp target/wasm32-wasip2/release/pumpkin_voice.wasm /path/to/pumpkin/plugins/
   ```

### Adjust Server Configurations & Connect

The first time you boot the server, the plugin will construct a default configuration file at `plugins/data/pumpkin_voice/config.toml`.
By default, the plugin will span out a UDP listener concurrently running on port `24454`.

Connect via your Minecraft client. Look at the bottom left of your screen, you should see no "Unplugged" symbol. Press <kbd>V</kbd> to open up the Simple Voice Chat UI to guarantee that the UI says "Voice Chat Connected".

---

## Commands

PumpkinMC directly delegates commands to the plugin via the Brigadier argument mapping interface. Use the following commands in-game:

| Command | Description | Permission Node |
|---------|-------------|-----------------|
| `/voicechat join <group_name> <password>` | Looks up a global group and assigns you to it. Supports passwords. | `pumpkin_voice:groups` |
| `/voicechat leave` | Leaves your active voice group. As upstream, this does not require the group permission, so a permission change cannot trap a player in a group. | command permission only |
| `/voicechat invite <target>` | Sends a chat message to a player with a one-click join link. | `pumpkin_voice:groups` |

---

## Architecture

This codebase acts as an extremely rapid buffer bridging Minecraft Plugin Messages (TCP) and the secure stream bounds (UDP/Datagram). 

### Directory Structure

```text
src/
├── commands/          # Brigadier command interfaces (/voicechat branch)
├── config/            # TOML layout and initial injection maps
├── handlers/          # Event interceptors (Player Join/Leave, GUI Custom Payloads)
├── net/               # Networking logic
│   ├── udp/           # UDP socket, cryptography, and packet handling
│   ├── custom_payloads.rs # TCP Custom payload definitions
│   └── voice_packets.rs   # Audio specific byte arrays mimicking `FriendlyByteBuf`
├── state/             # Shared asynchronous connection cache logic (Groups, Players)
├── util/              # Byte buffer extensions
└── lib.rs             # Plugin Entrypoint. Registers macro hooks and routes exports
```

### Request Lifecycle

1. **Player Connection and Compatibility Check**:
    - Trigger: `PlayerJoinEvent`, followed by the client's `voicechat:request_secret` payload.
    - Action: Join creates the upstream-compatible default disconnected state. After compatibility version `20` is confirmed, the server sends the player-state, category, and group registries in that order, followed by `SecretPacket`. These packets are intentionally not sent before the client registers its plugin channels.
2. **UDP Handshake Authentication**:
    - Trigger: Client triggers a `AuthenticatePacket` to `udp_server.rs:24454`.
    - Action: Server validates the outer and inner player UUID, the expected `Secret`, and the UDP source address. `ConnectionCheck` then promotes the pending socket to a connected voice state and broadcasts that state.
3. **Continuous Audio Delivery**:
    - Trigger: Player pushes to talk. Client issues `MicPacket` encoded datagrams.
    - Action: `udp_server.rs` assesses constraints (distance, group ID). If the checks pass, it routes via `PlayerSoundPacket` or `GroupSoundPacket`. Audio bleeding between different worlds is prevented by comparing Pumpkin world IDs.
4. **Heartbeat and Reconnection**:
    - Trigger: Periodic `KeepAlive` packets and the client's `KeepAlive` response.
    - Action: Responses refresh the authenticated connection timestamp. After `10 × keep_alive` without a response, the state is marked disconnected, a fresh secret is generated, registries are resynchronized, and the client is asked to authenticate again.

### Deep Permission Integration

The plugin registers native permission nodes via `pumpkin_plugin_api::permission::Permission`. Adjust these directly inside your primary Pumpkin engine deployment!

- `pumpkin_voice:command.voicechat`: Required to view the commands layout inside chat.
- `pumpkin_voice:speak`: Prevents sending encrypted UDP `MicPackets` outbound.
- `pumpkin_voice:listen`: Prevents receiving encrypted `PlayerSoundPackets` inside loops.
- `pumpkin_voice:groups`: Enables UI access to channels.

---

## Feature Comparison vs. Upstream Simple Voice Chat

Baseline: the upstream [Simple Voice Chat](https://modrinth.com/plugin/simple-voice-chat) server implementation by henkelmax (`2.6.24+26.2` — Bukkit/Paper plugin plus the shared server core), verified against the [upstream `26.2` source branch](https://github.com/henkelmax/simple-voice-chat/tree/26.2). Everything below is **server-side** behavior; client-side features (see the end of this section) ship in the client mod and work as long as this server speaks the protocol.

### ✅ Implemented (server-side parity)

| Upstream feature | Status here |
| ---------------- | ----------- |
| Dedicated UDP voice port with per-player AES-128-GCM secrets | ✅ |
| `SecretPacket` handshake (port, codec, MTU, distance, keep-alive, groups flag, voice host, recording flag) | ✅ |
| Compatibility-version gate plus `states → categories → groups → secret` initialization order | ✅ |
| UDP `Authenticate` / `AuthenticateAck` with secret verification | ✅ |
| Proximity audio with `max_voice_distance`, `whisper_distance`, `broadcast_range`, same-dimension filter | ✅ |
| Groups: create / join / leave via GUI (`set_group`, `create_group`, `leave_group`) with password protection | ✅ |
| Group & player-state synchronization (`add_group`, `remove_group`, `joined_group`, `state`, `states`, `update_state`) | ✅ |
| Volume categories from configuration (`add_category`) | ✅ (no category icons) |
| Keep-alive heartbeat, timeout detection, disconnected-state broadcast, and fresh-secret reconnect | ✅ |
| `force_voice_chat` + `login_timeout` kick for unmodded clients | ✅ |
| `allow_pings` UDP ping echo | ✅ |
| `ConnectionCheck` / `ConnectionCheckAck` | ✅ |
| `spectator_interaction` with positional `LocationSoundPacket` audio | ✅ |
| Group-type routing (`NORMAL` / `OPEN` / `ISOLATED`) | ✅ |
| Group input validation matching upstream `GROUP_REGEX` (`\p{C}`, leading whitespace, and packet length limits) | ✅ |
| Bounds-checked TCP/UDP packet decoding and authenticated-source enforcement | ✅ |
| Player quit state removal via `voicechat:remove_state` | ✅ |
| `/voicechat join` by UUID or quoted name, with Pumpkin server-side suggestions | ✅ |
| `allow_recording`, `codec`, `mtu_size`, `voice_host` passthrough to clients | ✅ |
| Permission nodes (`speak` / `listen` / `groups`) enforced on the audio path | ✅ (renamed `pumpkin_voice:*`) |
| Disabled/disconnected receiver filtering | ✅ |
| Offline-mode encryption identity warning via Pumpkin server API | ✅ |
| Localized player-facing messages, descriptions, and console logs | ✅ plugin-owned JSON registry — `en_us` + `zh_cn` built in, data-folder additions/overrides (see [Translations](#translations)) |
| Packet rate limiting | ✅ (ours limits UDP; upstream limits the plugin-message channel) |
| Bedrock clients (kicked under `force_voice_chat`, skipped for Java payloads) | ➕ beyond upstream |

### ⚠️ Partially implemented

| Area | Upstream behavior | Current behavior |
| ---- | ------------------ | ----------------- |
| Persistent groups | Groups flagged persistent survive becoming empty | The flag is honored during empty-group cleanup, but nothing ever creates a persistent group |
| Permission denial feedback | Players get a cooldown-limited "no speak/listen permission" chat message | Silent debug log only |
| Hidden groups | Hidden groups are marked so clients omit them from public listings | State and synchronization preserve the hidden flag, but no current command or API creates hidden groups |

### ❌ Not implemented (server-side)

**Protocol & robustness**
- TCP plugin-message rate limit (`tcp_rate_limit`, upstream default 16 packets/s)
- Vanish / visibility (`canSee`) integration — hidden players are treated like normal players

**Groups & audio**
- `spectator_player_possession` — config option is parsed but unused; spectators cannot speak *through* the player they are spectating
- Persistent/hidden group creation and persistence across server restarts

**Commands & permissions**
- `/voicechat help`
- `/voicechat test <target>` (admin connection ping test) and the equivalent `voicechat.admin` permission node

**Integrations & extensibility**
- The addon/plugin API (`VoicechatServerApi`): 38 event types, audio channels (`Static` / `Locational` / `Entity`), `AudioPlayer`, Opus encoder/decoder, MP3, custom sockets, raw UDP packet interception
- Proxy forwarding (Velocity / BungeeCord / Waterfall companion plugins)
- PlaceholderAPI placeholders and ViaVersion compatibility layer
- `use_natives` / `threaded_server_support` config options (not portable to WASM/Pumpkin — intentionally omitted)

### Client-side features (out of scope for the server)

Push-to-talk, voice activation, automatic voice-activity detection, automatic microphone gain, RNNoise noise suppression, OpenAL output, Opus encoding/decoding, microphone & speaker test playback, configurable PTT key, individual player volume adjustment, microphone amplification, 3D sound, HUD icons, the group UI, and the recording UI all live in the client mod. They work against this server as long as the protocol above stays compatible.

---

## Translations

The plugin owns a small JSON translation registry so languages are discovered by file name and are not coupled to a hand-maintained Rust locale enum:

1. `build.rs` scans every `lang/*.json` file and embeds it in the WASM component at build time.
2. On load, the plugin scans `<plugin data folder>/lang/*.json` and merges those files after the embedded catalogs. A data-folder file may define a new locale or override only selected keys.
3. Player-facing messages use `player.get_locale()`. Registration-time descriptions and console logs use the `language` value from `config.toml`.
4. `%s` and indexed `%N$s` placeholders are substituted by the plugin. Missing keys fall back to `en_us`, then to the raw key.

The files currently shipped under `lang/` provide `en_us` (the fallback) and `zh_cn`. That list is intentionally not duplicated in Rust source: the directory is the source of truth.

### Adding or overriding languages at runtime

Server admins can add a language or override any built-in string without recompiling: drop a `lang/<locale>.json` file into the plugin data folder (e.g. `plugins/data/pumpkin_voice/lang/de_de.json`) and reload the plugin. Data-folder files are loaded after the embedded ones, so they win.

```json
{
  "command.join.joined": "Gruppe %s beigetreten",
  "kick.voice_chat_required": "Du musst den Simple Voice Chat Mod installiert haben!"
}
```

To ship a new language **built in**, add `lang/<locale>.json` at the crate root and rebuild. No Rust source change or locale registration is needed.

`lang/en_us.json` is the key reference. It includes player messages, plugin/command/permission descriptions, default category labels, errors, and console log templates. Tests reject embedded catalogs with invalid JSON or a key set that differs from `en_us`.

Pumpkin requests plugin metadata before it provides the plugin data-folder path or loads `config.toml`, so the metadata description uses the embedded `en_us` fallback. Command and permission descriptions are registered later and therefore use the configured server language.

---

## Environment Variables / Configuration

Here is a breakdown of the standard `config.toml` structure dynamically dropped upon deployment:

| Variable | Description | Default |
| -------- | ----------- | ------- |
| `port` | The UDP Binding port. `-1` aligns directly to TCP game port. | `24454` |
| `bind_address` | String address the UDP socket clamps to. | `""` (0.0.0.0) |
| `max_voice_distance` | Range cap for dimensional fading audios. | `48.0` |
| `whisper_distance` | Range cap specifically for whispering clients. | `24.0` |
| `codec` | Opus codec compression parameter strings. | `VOIP` |
| `mtu_size` | Maximum audio packet size forwarded to clients. | `1024` |
| `keep_alive` | Millisecond trigger interval looping connection verifications. | `1000` |
| `enable_groups` | Allow or reject GUI `voicechat:create_group` payloads. | `true` |
| `voice_host` | Hostname clients should use to reach the voice server. | `""` (game host) |
| `allow_recording` | Whether clients may record voice chat audio. | `true` |
| `spectator_interaction` | Whether spectators can talk to nearby players. | `false` |
| `spectator_player_possession` | Parsed but currently unused (see feature comparison). | `false` |
| `force_voice_chat` | If `true`, non-modded clients are immediately dropped using a kick constraint. | `false` |
| `login_timeout` | Grace period before `force_voice_chat` kicks unmodded clients (ms). | `10000` |
| `max_packets_per_second` | Maximum UDP packets allowed per player per second before throttling. | `500` |
| `allow_pings` | Whether to respond to UDP ping packets from clients. | `true` |
| `broadcast_range` | Maximum range for audio broadcast. `-1` uses max voice distance. | `-1.0` |

### Categories Configuration

You can define custom categories in the `config.toml`:

```toml
[[categories]]
id = "radio"
name = "Radio Team"
description = "Global broadcast"
```

---

## Troubleshooting

### Connection Timeouts / GUI Shows Unplugged
**Error:** Connecting prints "Voice Chat not found!" or times out aggressively.
**Solution:** 
1. Determine if the UDP port `24454` is exposed in your cloud firewall (e.g., UFW/AWS/OCI panels). UDP acts alongside TCP constraints but requires dedicated protocol openings.
2. Check the server console for `Voice chat UDP server listening on ...` — if the UDP bind failed, the plugin logs `Failed to start UDP server` and voice chat stays offline.
3. Check for `Rate limiting player ...` warnings in the server console; if seen, increase `max_packets_per_second` in `config.toml`.

### Group Join Discarding
**Error:** User selects a correct password but receives "Invalid Password."
**Solution:** Ensure the client and server code are mirrored correctly. Abandoned GUI parameters occasionally drop payload arrays if the UI bugs out locally. Validate through the standard `/voicechat join` commands as a bypass mechanic. 

### Config Write Errors (WASI)
**Error:** `Failed to create config folder ... (os error 44)` or `Operation not permitted`.
**Solution:** This typically indicates a permission or preopen mismatch in the WASI environment. Ensure the plugin metadata requests `fs.read.data` and `fs.write.data` (default in recent versions). The plugin now uses absolute-style relative paths to ensure compatibility with Pumpkin's virtual filesystem.
