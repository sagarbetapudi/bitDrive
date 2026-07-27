# Bluetooth Personal Link (BPL)

A persistent Bluetooth-based communication platform between laptop (desktop) and Android phone without any network infrastructure.

## Architecture Overview

```
┌─────────────────┐    Bluetooth RFCOMM    ┌─────────────────┐
│  Android App    │ ◄─────────────────────► │ Desktop Daemon  │
│  (Kotlin)       │                         │  (Rust)         │
└─────────────────┘                         └─────────────────┘
```

## Project Structure

```
playground/
├── proto/                    # Shared Protocol Buffers definitions
├── desktop/                  # Rust desktop component
│   ├── Cargo.toml           # Workspace manifest
│   ├── protocol/            # Protocol implementation
│   ├── bluetooth/           # Bluetooth RFCOMM (Windows/Linux/macOS)
│   ├── core/                # Core services (config, DB, events)
│   ├── daemon/              # Main daemon binary
│   └── cli/                 # CLI management tool
└── android/                 # Android application
    ├── build.gradle.kts
    ├── settings.gradle.kts
    ├── gradle.properties
    └── app/                 # Main application module
```

## Features

### Core Protocol
- Custom binary protocol over Bluetooth Classic RFCOMM
- Frame-based with CRC32C checksums
- Session management with capability negotiation
- Mutual authentication (PSK-based)
- Multiplexed logical channels with flow control
- Service registry for dynamic service discovery

### Desktop Services (Rust)
- **Filesystem**: Browse, read, write, watch desktop directories
- **Sync**: Bidirectional directory synchronization with conflict resolution
- **Photo Backup**: Automatic incremental photo backup from phone
- **Remote Shell**: PTY-based command execution (ConPTY on Windows)
- **Media Control**: MPRIS (Linux) / Media Session (Windows) integration
- **Phone FS Access**: Access Android storage via FUSE/Dokan mount
- **Proximity**: RSSI-based distance estimation (Near/Far/Out of Range)
- **File Streaming**: On-demand streaming with range requests
- **App Launcher**: Launch Android apps from desktop

### Android Services (Kotlin)
- **Foreground Service**: Persistent Bluetooth connection
- **DocumentsProvider**: Filesystem integration with system file picker
- **Notification Listener**: Media control buttons
- **WorkManager**: Background sync scheduling
- **Room Database**: Local storage for sessions, jobs, conflicts

## Building

### Desktop (Rust)

**Prerequisites:**
- Rust 1.75+ (`rustup install stable`)
- Windows 10 1809+ / Linux with BlueZ / macOS 10.15+
- Protocol Buffers compiler (`protoc`)

```bash
cd desktop
cargo build --release
```

**Output:**
- `target/release/bpl-daemon` - Main daemon
- `target/release/bpl-cli` - Management CLI

### Android

**Prerequisites:**
- Android Studio Hedgehog+
- JDK 17
- Android SDK 34

```bash
cd android
./gradlew assembleRelease
```

**Output:**
- `app/build/outputs/apk/release/app-release.apk`

## Running

### Desktop Daemon
```bash
# First run - generates config
./bpl-daemon

# With custom config
./bpl-daemon --config /path/to/config.toml

# Validate config
./bpl-daemon --validate-config
```

### CLI Tool
```bash
# List paired devices
bpl-cli list-devices

# Pair with device
bpl-cli pair "AA:BB:CC:DD:EE:FF"

# Unpair device
bpl-cli unpair "AA:BB:CC:DD:EE:FF"

# Start shell session
bpl-cli shell "AA:BB:CC:DD:EE:FF" "ls -la"

# Trigger sync
bpl-cli sync "AA:BB:CC:DD:EE:FF" /home/user/Documents

# View logs
bpl-cli logs --follow
```

### Android App
1. Install APK on Android device
2. Grant required permissions:
   - Bluetooth
   - Nearby devices (Android 12+)
   - Notifications
   - Files access (for DocumentsProvider)
3. Open app and tap "Pair Desktop"
4. Enter PSK shown on desktop (or generate on desktop with `bpl-cli generate-psk`)

## Configuration

### Desktop (`~/.config/bluetooth-personal-link/config.toml`)

```toml
[bluetooth]
adapter_id = ""           # Empty = auto-detect
service_uuid = "B7E5E0F0-1A2B-4C3D-8E9F-A0B1C2D3E4F5"
device_name = "BPL Desktop"
auto_connect = true
reconnect_interval_sec = 30
keepalive_interval_sec = 30
max_frame_size = 16384
max_channels = 16

[database]
path = "data/bpl.db"
max_connections = 10
backup_enabled = true
backup_interval_hours = 24

[logging]
level = "info"
format = "json"
file = "logs/bpl.log"
max_file_size_mb = 100
max_files = 10

[security]
psk = ""                  # Base64 encoded, set via CLI
require_authentication = true
session_timeout_sec = 3600
max_failed_attempts = 5
lockout_duration_sec = 300

[services.filesystem]
enabled = true
root_path = "/home/user"  # Linux/macOS
# root_path = "C:\\Users\\user"  # Windows
read_only = false
follow_symlinks = true
max_concurrent_operations = 10
max_payload_size = 16384

[services.sync]
enabled = true

[services.photo_backup]
enabled = false
auto_backup = true
only_when_charging = true
organize_by_date = true
generate_thumbnails = true
deduplicate = true

[services.shell]
enabled = true
pty_support = true
session_persistence = true
command_history = true
max_sessions = 4

[services.media_control]
enabled = true

[services.phone_fs]
enabled = true
mount_point = "/mnt/phone"  # Linux/macOS
# mount_point = "P:"        # Windows (Dokan)

[services.proximity]
enabled = true
rssi_poll_interval_ms = 2000
near_threshold_dbm = -60
far_threshold_dbm = -80

[services.file_stream]
enabled = true
preferred_chunk_size = 65536
max_concurrent_streams = 4

[services.app_launcher]
enabled = true
```

### Android
Configuration stored in DataStore (`bpl_config.xml`). Managed via app UI or desktop CLI.

## Protocol Specification

### Frame Format
```
┌─────────────────────────────────────────────────────────────┐
│ Magic (4) │ Version (4) │ Type (1) │ Flags (1) │ Channel (4) │
├─────────────────────────────────────────────────────────────┤
│ Sequence (8)                                    │ Length (4) │
├─────────────────────────────────────────────────────────────┤
│ Header CRC32C (4)                                           │
├─────────────────────────────────────────────────────────────┤
│ Payload (variable, max 64KB)                                │
├─────────────────────────────────────────────────────────────┤
│ Auth Tag (16, AES-GCM)                                      │
└─────────────────────────────────────────────────────────────┘
```

### Channel Assignments
| Channel | Service |
|---------|---------|
| 0 | Control (session, auth, capability, channel mgmt) |
| 1 | Filesystem |
| 2 | Streaming |
| 3 | Shell |
| 4 | Launcher |
| 5 | Synchronization |
| 6 | Media Control |

### Session Establishment
```
1. RFCOMM Connect
2. HELLO (SessionOpen) → Protocol version, device info, capabilities
3. CAPABILITY NEGOTIATION → Mutual service agreement
4. AUTHENTICATION → PSK challenge-response + key derivation
5. CHANNEL OPEN → Per-service logical channels
6. SERVICE OPERATION → Feature usage
```

### Authentication (PSK)
- Pre-shared key (32 bytes) configured on both devices
- Challenge-response with nonces
- HKDF-SHA256 key derivation
- Per-channel AES-256-GCM keys
- Mutual authentication with key confirmation

## Development

### Protocol Buffers
Shared `.proto` files in `proto/` directory. Generate code:

```bash
# Rust
cd desktop/protocol && cargo build

# Android (auto via Gradle)
cd android && ./gradlew generateProto
```

### Adding a New Service
1. Define protocol in `proto/<service>.proto`
2. Implement `Service` trait (Rust) / `Service` interface (Kotlin)
3. Register in service registry
4. Assign channel ID
5. Update capability negotiation

## Security

- **Transport**: Bluetooth Classic RFCOMM (encrypted at link layer)
- **Application**: AES-256-GCM per channel
- **Authentication**: PSK-based mutual auth with HKDF
- **Replay Protection**: 64-bit sequence numbers per channel
- **Key Rotation**: Configurable session key rotation
- **Device Trust**: Pairing with explicit user acceptance

## Performance

- **Frame Size**: Up to 64KB (configurable, default 16KB)
- **Window**: Credit-based flow control (default 64KB)
- **Latency**: <10ms local Bluetooth
- **Throughput**: ~2-3 MB/s (Bluetooth Classic 2.1+EDR)
- **Memory**: <50MB idle, <200MB under load

## Testing

```bash
# Desktop unit tests
cd desktop && cargo test

# Android unit tests
cd android && ./gradlew test

# Android instrumented tests
cd android && ./gradlew connectedAndroidTest
```

## License

MIT License - see LICENSE file for details."# bitDrive" 
"# bitDrive" 
