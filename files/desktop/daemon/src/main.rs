//! BPL Desktop Daemon - Main entry point
//!
//! This is the background service that runs on the desktop, handling
//! Bluetooth communication and providing services to the Android app.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use tokio::sync::broadcast;
use tokio::signal;
use tracing::{error, info, warn};

use bpl_core::{ConfigManager, Core};
use bpl_protocol::ProtocolError;

/// Command line arguments
#[derive(Parser, Debug)]
#[command(name = "bpl-daemon", version, about = "Bluetooth Personal Link Desktop Daemon")]
struct Args {
    /// Run in foreground (don't daemonize)
    #[arg(long, default_value_t = true)]
    foreground: bool,

    /// Daemonize (run in background)
    #[arg(long, conflicts_with = "foreground")]
    daemonize: bool,

    /// Log level
    #[arg(long, default_value = "info", env = "BPL_LOG_LEVEL")]
    log_level: String,

    /// Log format (json, text, compact)
    #[arg(long, default_value = "json", env = "BPL_LOG_FORMAT")]
    log_format: String,

    /// Config file path
    #[arg(long, env = "BPL_CONFIG")]
    config: Option<PathBuf>,

    /// PID file path
    #[arg(long, env = "BPL_PID_FILE")]
    pid_file: Option<PathBuf>,

    /// Data directory
    #[arg(long, env = "BPL_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// Validate config and exit
    #[arg(long)]
    validate_config: bool,
}

/// Main daemon state
struct Daemon {
    args: Args,
    core: Option<Core>,
    shutdown_tx: broadcast::Sender<()>,
}

impl Daemon {
    /// Create new daemon instance
    async fn new(args: Args) -> Result<Self> {
        let (shutdown_tx, _) = broadcast::channel(1);

        Ok(Self {
            args,
            core: None,
            shutdown_tx,
        })
    }

    /// Initialize daemon components
    async fn initialize(&mut self) -> Result<()> {
        info!("Initializing daemon...");

        // Load configuration
        let config = ConfigManager::load().await
            .context("Failed to load configuration")?;

        info!("Configuration loaded");

        // Create core
        let mut core = Core::new().await
            .context("Failed to create core")?;

        // Initialize services
        core.init().await
            .context("Failed to initialize services")?;

        info!("Core initialized");
        self.core = Some(core);

        Ok(())
    }

    /// Start daemon
    async fn start(&mut self) -> Result<()> {
        info!("Starting daemon...");

        if let Some(core) = &mut self.core {
            core.start().await
                .context("Failed to start core services")?;
        }

        info!("Daemon started successfully");

        // Run main loop
        self.run().await
    }

    /// Main daemon loop
    async fn run(&mut self) -> Result<()> {
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        // Wait for shutdown signal
        tokio::select! {
            _ = shutdown_rx.recv() => {
                info!("Shutdown signal received");
            }
            _ = signal::ctrl_c() => {
                info!("Ctrl-C received");
            }
        }

        Ok(())
    }

    /// Stop daemon gracefully
    async fn stop(&mut self) -> Result<()> {
        info!("Stopping daemon...");

        if let Some(core) = &mut self.core {
            core.stop().await
                .context("Failed to stop core")?;
        }

        info!("Daemon stopped");
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    init_logging(&args.log_level, &args.log_format)
        .context("Failed to initialize logging")?;

    info!("Starting BPL Desktop Daemon v{}", env!("CARGO_PKG_VERSION"));
    info!("Log level: {}, format: {}", args.log_level, args.log_format);

    // Handle config validation
    if args.validate_config {
        info!("Validating configuration...");
        ConfigManager::load().await
            .context("Configuration validation failed")?;
        info!("Configuration is valid");
        return Ok(());
    }

    // Create daemon
    let mut daemon = Daemon::new(args).await
        .context("Failed to create daemon")?;

    // Initialize
    daemon.initialize().await
        .context("Failed to initialize daemon")?;

    // Start
    daemon.start().await
        .context("Daemon failed")?;

    // Graceful shutdown
    daemon.stop().await
        .context("Failed to stop daemon gracefully")?;

    info!("Daemon exited cleanly");
    Ok(())
}

/// Initialize logging
fn init_logging(level: &str, format: &str) -> Result<()> {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));

    let fmt_layer = match format {
        "json" => fmt::layer().json().boxed(),
        "compact" => fmt::layer().compact().boxed(),
        _ => fmt::layer().boxed(),
    };

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();

    Ok(())
}