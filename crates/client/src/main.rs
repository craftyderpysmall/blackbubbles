//! BlackBubbles: your Apple world on Linux and macOS.

mod app;
mod beacon;
mod config;
mod conn;
mod fonts;
mod media;
mod pages;
mod util;

use std::fs::File;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::{info, warn};

#[derive(Parser)]
#[command(name = "blackbubbles", version, about = "BlackBubbles desktop client")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run without a window: stay connected and autofill codes (for a systemd/launchd service).
    Daemon,
    /// Pair with a Mac using the string printed by `bbd pair`.
    Pair {
        uri: String,
        #[arg(long)]
        name: Option<String>,
    },
    /// Register this PC as a Find My beacon (Linux).
    BeaconSetup {
        #[arg(long)]
        name: Option<String>,
    },
}

/// Only one process autofills: whoever holds this lock (the daemon, if running).
fn autofill_lock() -> Option<File> {
    let dir = config::dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    let file = File::options().create(true).truncate(false).write(true).open(dir.join("autofill.lock")).ok()?;
    file.try_lock().ok()?;
    Some(file)
}

fn start_autofill(config: &config::Config) -> (Option<bb_autofill::Autofill>, Option<File>) {
    if !config.autofill.enabled {
        return (None, None);
    }
    let Some(lock) = autofill_lock() else {
        info!("autofill is handled by another BlackBubbles process");
        return (None, None);
    };
    match bb_autofill::start(&config.autofill) {
        Ok(tx) => (Some(tx), Some(lock)),
        Err(err) => {
            warn!("autofill disabled: {err:#}");
            (None, None)
        }
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,wgpu=warn,eframe=warn".into()))
        .init();
    // reqwest (map tiles) and our sockets share rustls with the ring provider.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cli = Cli::parse();
    let runtime = tokio::runtime::Runtime::new()?;

    match cli.command {
        Some(Command::Pair { uri, name }) => {
            let name = name.unwrap_or_else(conn::device_name);
            let server = runtime.block_on(conn::pair(&uri, &name))?;
            let mut config = config::Config::load()?;
            config.server = Some(server);
            config.save()?;
            println!("Paired as {name:?}. Settings saved to {}", config::path()?.display());
            Ok(())
        }
        Some(Command::BeaconSetup { name }) => {
            let mut config = config::Config::load()?;
            let server = config.server.clone().context("pair with your Mac first")?;
            let client = conn::Client::new(runtime.handle().clone(), Some(server));
            let name = name.unwrap_or_else(conn::device_name);
            let beacon = runtime.block_on(async {
                // Give the connection a moment to come up.
                for _ in 0..50 {
                    if matches!(client.status(), conn::Status::Connected { .. }) {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
                beacon::setup(&client, &name).await
            })?;
            config.beacon = Some(beacon);
            config.save()?;
            println!("Registered. Public keys: {}", config::beacon_public_path()?.display());
            println!("Now run: sudo systemctl enable --now blackbubbles-beacon@{}", std::env::var("USER").unwrap_or_default());
            Ok(())
        }
        Some(Command::Daemon) => {
            let config = config::Config::load()?;
            let (autofill, _lock) = start_autofill(&config);
            let client = conn::Client::new(runtime.handle().clone(), config.server.clone());
            if let Some(af) = autofill.as_ref().filter(|_| config.autofill.from_server) {
                client.set_autofill(af.tx.clone());
            }
            info!("BlackBubbles daemon running");
            runtime.block_on(async {
                let mut last = None;
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
                let shutdown = shutdown_signal();
                tokio::pin!(shutdown);
                loop {
                    tokio::select! {
                        _ = &mut shutdown => break,
                        _ = tick.tick() => {
                            client.take_events();
                            let status = client.status();
                            if last.as_ref() != Some(&status) {
                                info!("connection: {status:?}");
                                last = Some(status);
                            }
                        }
                    }
                }
            });
            if let Some(af) = &autofill {
                af.shutdown();
            }
            info!("stopped");
            Ok(())
        }
        None => {
            let (config, startup_error) = match config::Config::load() {
                Ok(c) => (c, None),
                Err(e) => (config::Config::default(), Some(format!("{e:#}"))),
            };
            let (autofill, lock) = start_autofill(&config);
            let on_exit = autofill.clone();
            let autofill = autofill.map(|af| af.tx);
            let rt = runtime.handle().clone();
            let options = eframe::NativeOptions {
                viewport: eframe::egui::ViewportBuilder::default()
                    .with_title("BlackBubbles")
                    .with_app_id("blackbubbles")
                    .with_inner_size([1180.0, 760.0])
                    .with_min_inner_size([720.0, 480.0]),
                ..Default::default()
            };
            let result = eframe::run_native(
                "BlackBubbles",
                options,
                Box::new(move |cc| Ok(Box::new(app::App::new(cc, rt, config, autofill, startup_error)))),
            );
            if let Some(af) = on_exit {
                af.shutdown();
            }
            drop(lock);
            result.map_err(|e| anyhow::anyhow!("{e}"))
        }
    }
}
