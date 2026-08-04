//! BPL Desktop CLI
//!
//! Command line interface for managing the Bluetooth Personal Link daemon.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rand::RngCore;
use tracing::{info, warn};

use bpl_core::{ConfigManager, Core};
use bpl_protocol::ProtocolError;

/// Command line arguments
#[derive(Parser, Debug)]
#[command(name = "bpl-cli", version, about = "Bluetooth Personal Link Desktop CLI")]
struct Args {
    /// Log level
    #[arg(short, long, default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    command: Command,
}

/// Commands
#[derive(Subcommand, Debug)]
enum Command {
    /// Pair with a device
    Pair {
        /// Device address or name
        device: String,
        /// PIN for pairing (optional)
        #[arg(short, long)]
        pin: Option<String>,
    },

    /// Unpair a device
    Unpair {
        /// Device address or name
        device: String,
    },

    /// List paired devices
    #[command(name = "list-devices")]
    ListDevices,

    /// Show device information
    #[command(name = "device-info")]
    DeviceInfo {
        /// Device address or name
        device: String,
    },

    /// List registered services
    Services,

    /// Start a service
    #[command(name = "start-service")]
    StartService {
        /// Service ID
        service: String,
    },

    /// Stop a service
    #[command(name = "stop-service")]
    StopService {
        /// Service ID
        service: String,
    },

    /// Configuration management
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Remote shell
    Shell {
        /// Device address or name
        device: String,
        /// Command to execute (optional, starts interactive if omitted)
        command: Vec<String>,
    },

    /// Synchronize directory
    Sync {
        /// Target device
        target: String,
        /// Path to sync (optional, uses sync job if not specified)
        path: Option<String>,
        /// Force sync even if unchanged
        #[arg(short, long)]
        force: bool,
    },

    /// Show sync status
    #[command(name = "sync-status")]
    SyncStatus {
        /// Job ID (optional, shows all if not specified)
        job: Option<String>,
    },

    /// List sync jobs
    #[command(name = "list-sync-jobs")]
    ListSyncJobs,

    /// Create sync job
    #[command(name = "create-sync-job")]
    CreateSyncJob {
        /// Job name
        name: String,
        /// Local path
        local_path: String,
        /// Remote path
        remote_path: String,
        /// Direction (bidirectional, upload, download)
        direction: String,
        /// Conflict strategy
        #[arg(short, long, default_value = "last_write_wins")]
        conflict_strategy: String,
        /// Auto-sync
        #[arg(short, long)]
        auto: bool,
    },

    /// Launch app on device
    #[command(name = "launch-app")]
    LaunchApp {
        /// Device address or name
        device: String,
        /// Package name
        package: String,
        /// Activity name (optional)
        activity: Option<String>,
    },

    /// List apps on device
    #[command(name = "list-apps")]
    ListApps {
        /// Device address or name
        device: String,
    },

    /// Get media state
    #[command(name = "media-state")]
    MediaState {
        /// Device address or name
        device: String,
    },

    /// Control media
    #[command(name = "media-control")]
    MediaControl {
        /// Device address or name
        device: String,
        /// Action (play, pause, stop, next, previous, volume, seek)
        action: String,
        /// Volume level (0.0-1.0, for volume action)
        #[arg(short, long)]
        volume: Option<f32>,
    },

    /// Get proximity
    Proximity {
        /// Device address or name
        device: String,
    },

    /// Stream file
    Stream {
        /// Device address or name
        device: String,
        /// File path
        path: String,
        /// Output file (optional, stdout if not specified)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Show logs
    Logs {
        /// Number of lines
        #[arg(short, long, default_value = "100")]
        lines: usize,
        /// Follow logs
        #[arg(short, long)]
        follow: bool,
    },

    /// Show daemon status
    Status,

    /// Generate PSK
    #[command(name = "generate-psk")]
    GeneratePsk,

    /// Set PSK
    #[command(name = "set-psk")]
    SetPsk {
        /// Base64 encoded PSK
        psk: String,
    },
}

/// Config actions
#[derive(Subcommand, Debug)]
enum ConfigAction {
    /// Get config value
    Get {
        /// Key
        key: String,
    },
    /// Set config value
    Set {
        /// Key
        key: String,
        /// Value
        value: String,
    },
    /// List all config
    List,
    /// Reset to defaults
    Reset,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    init_logging(&args.log_level)?;

    // Execute command
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(execute_command(args))
}

/// Initialize logging
fn init_logging(level: &str) -> Result<()> {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer())
        .init();

    Ok(())
}

/// Execute command
async fn execute_command(args: Args) -> Result<()> {
    match args.command {
        Command::Pair { device, pin } => cmd_pair(device, pin).await,
        Command::Unpair { device } => cmd_unpair(device).await,
        Command::ListDevices => cmd_list_devices().await,
        Command::DeviceInfo { device } => cmd_device_info(device).await,
        Command::Services => cmd_services().await,
        Command::StartService { service } => cmd_start_service(service).await,
        Command::StopService { service } => cmd_stop_service(service).await,
        Command::Config { action } => cmd_config(action).await,
        Command::Shell { device, command } => cmd_shell(device, command).await,
        Command::Sync { target, path, force } => cmd_sync(target, path, force).await,
        Command::SyncStatus { job } => cmd_sync_status(job).await,
        Command::ListSyncJobs => cmd_list_sync_jobs().await,
        Command::CreateSyncJob { name, local_path, remote_path, direction, conflict_strategy, auto } => {
            cmd_create_sync_job(name, local_path, remote_path, direction, conflict_strategy, auto).await
        }
        Command::LaunchApp { device, package, activity } => cmd_launch_app(device, package, activity).await,
        Command::ListApps { device } => cmd_list_apps(device).await,
        Command::MediaState { device } => cmd_media_state(device).await,
        Command::MediaControl { device, action, volume } => cmd_media_control(device, action, volume).await,
        Command::Proximity { device } => cmd_proximity(device).await,
        Command::Stream { device, path, output } => cmd_stream(device, path, output).await,
        Command::Logs { lines, follow } => cmd_logs(lines, follow).await,
        Command::Status => cmd_status().await,
        Command::GeneratePsk => cmd_generate_psk().await,
        Command::SetPsk { psk } => cmd_set_psk(psk).await,
    }
}

/// Pair with device
async fn cmd_pair(device: String, pin: Option<String>) -> Result<()> {
    info!("Pairing with device: {}", device);
    println!("Pairing with {}...", device);
    if let Some(pin) = pin {
        println!("Using PIN: {}", pin);
    }
    println!("Pairing not yet implemented - connect to daemon via IPC");
    Ok(())
}

/// Unpair device
async fn cmd_unpair(device: String) -> Result<()> {
    info!("Unpairing device: {}", device);
    println!("Unpairing {}...", device);
    println!("Unpairing not yet implemented - connect to daemon via IPC");
    Ok(())
}

/// List paired devices
async fn cmd_list_devices() -> Result<()> {
    info!("Listing paired devices");
    let core = Core::new().await?;
    let devices = core.database.get_paired_devices()?;

    if devices.is_empty() {
        println!("No paired devices");
        return Ok(());
    }

    println!("Paired devices:");
    for device in devices {
        let status = if device.paired { "paired" } else { "unpaired" };
        let trusted = if device.trusted { " (trusted)" } else { "" };
        println!("  {} ({}) - {}{}", device.name.unwrap_or_else(|| "Unknown".to_string()), device.address, status, trusted);
    }

    Ok(())
}

/// Show device info
async fn cmd_device_info(device: String) -> Result<()> {
    info!("Getting device info for: {}", device);
    println!("Device info for {} not yet implemented", device);
    Ok(())
}

/// List services
async fn cmd_services() -> Result<()> {
    info!("Listing services");
    let core = Core::new().await?;
    let services = core.services.list_services();

    println!("Registered services:");
    for service in services {
        println!("  {}", service);
    }

    Ok(())
}

/// Start service
async fn cmd_start_service(service: String) -> Result<()> {
    info!("Starting service: {}", service);
    println!("Starting service {}...", service);
    println!("Service start not yet implemented");
    Ok(())
}

/// Stop service
async fn cmd_stop_service(service: String) -> Result<()> {
    info!("Stopping service: {}", service);
    println!("Stopping service {}...", service);
    println!("Service stop not yet implemented");
    Ok(())
}

/// Config management
async fn cmd_config(action: ConfigAction) -> Result<()> {
    let config = ConfigManager::load().await?;

    match action {
        ConfigAction::Get { key } => {
            if let Some(value) = config.get_config(&key)? {
                println!("{} = {}", key, value);
            } else {
                println!("Key '{}' not found", key);
            }
        }
        ConfigAction::Set { key, value } => {
            config.set_config(&key, &value, None, false, false)?;
            println!("Set {} = {}", key, value);
        }
        ConfigAction::List => {
            let configs = config.list_config()?;
            for c in configs {
                let secret = if c.secret { " [SECRET]" } else { "" };
                let readonly = if c.read_only { " [READONLY]" } else { "" };
                println!("{} = {}{}{}", c.key, c.value, secret, readonly);
            }
        }
        ConfigAction::Reset => {
            println!("Config reset not yet implemented");
        }
    }

    Ok(())
}

/// Remote shell
async fn cmd_shell(device: String, command: Vec<String>) -> Result<()> {
    info!("Starting shell on device: {}", device);
    println!("Shell on {} not yet implemented", device);
    Ok(())
}

/// Sync directory
async fn cmd_sync(target: String, path: Option<String>, force: bool) -> Result<()> {
    info!("Syncing target: {}, force: {}", target, force);
    println!("Sync not yet implemented");
    Ok(())
}

/// Show sync status
async fn cmd_sync_status(job: Option<String>) -> Result<()> {
    info!("Showing sync status");
    println!("Sync status not yet implemented");
    Ok(())
}

/// List sync jobs
async fn cmd_list_sync_jobs() -> Result<()> {
    info!("Listing sync jobs");
    let core = Core::new().await?;
    let jobs = core.database.list_sync_jobs()?;

    if jobs.is_empty() {
        println!("No sync jobs configured");
        return Ok(());
    }

    println!("Sync jobs:");
    for job in jobs {
        println!("  {} ({}) - {}", job.id, job.direction, job.name);
        println!("    Local: {} -> Remote: {}", job.local_path, job.remote_path);
        println!("    Enabled: {}, Auto: {}, Strategy: {}", job.enabled, job.auto_sync, job.conflict_strategy);
    }

    Ok(())
}

/// Create sync job
async fn cmd_create_sync_job(
    name: String,
    local_path: String,
    remote_path: String,
    direction: String,
    conflict_strategy: String,
    auto: bool,
) -> Result<()> {
    info!("Creating sync job: {}", name);
    println!("Creating sync job '{}'...", name);
    println!("Sync job creation not yet implemented");
    Ok(())
}

/// Launch app
async fn cmd_launch_app(device: String, package: String, activity: Option<String>) -> Result<()> {
    info!("Launching app {} on device {}", package, device);
    println!("Launching {} on {}...", package, device);
    println!("App launch not yet implemented");
    Ok(())
}

/// List apps
async fn cmd_list_apps(device: String) -> Result<()> {
    info!("Listing apps on device: {}", device);
    println!("Apps on {} not yet implemented", device);
    Ok(())
}

/// Get media state
async fn cmd_media_state(device: String) -> Result<()> {
    info!("Getting media state for device: {}", device);
    println!("Media state for {} not yet implemented", device);
    Ok(())
}

/// Control media
async fn cmd_media_control(device: String, action: String, volume: Option<f32>) -> Result<()> {
    info!("Media control: {} on device {}", action, device);
    println!("Media control {} on {}...", action, device);
    println!("Media control not yet implemented");
    Ok(())
}

/// Get proximity
async fn cmd_proximity(device: String) -> Result<()> {
    info!("Getting proximity for device: {}", device);
    println!("Proximity for {} not yet implemented", device);
    Ok(())
}

/// Stream file
async fn cmd_stream(device: String, path: String, output: Option<PathBuf>) -> Result<()> {
    info!("Streaming file {} from device {}", path, device);
    println!("Streaming not yet implemented");
    Ok(())
}

/// Show logs
async fn cmd_logs(lines: usize, follow: bool) -> Result<()> {
    info!("Showing logs (lines: {}, follow: {})", lines, follow);
    println!("Logs not yet implemented");
    Ok(())
}

/// Show status
async fn cmd_status() -> Result<()> {
    info!("Showing daemon status");
    println!("Daemon status not yet implemented");
    Ok(())
}

/// Generate PSK
async fn cmd_generate_psk() -> Result<()> {
    let mut psk = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut psk);

    let encoded = base64::encode(psk);
    println!("Generated PSK: {}", encoded);
    println!("Share this with the Android app during pairing");

    Ok(())
}

/// Set PSK
async fn cmd_set_psk(psk: String) -> Result<()> {
    info!("Setting PSK");
    let config = ConfigManager::load().await?;

    let decoded = base64::decode(&psk)
        .context("Invalid base64 PSK")?;

    if decoded.len() != 32 {
        anyhow::bail!("PSK must be 32 bytes (base64 encoded)");
    }

    config.set_psk(decoded).await?;
    println!("PSK set successfully");

    Ok(())
}