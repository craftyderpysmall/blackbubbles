//! BlackBubbles: your Apple world on Linux and macOS.

mod audio;
mod blob;
mod commands;
mod config;
mod conn;
mod notify;
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
    /// Developer aid: send one request (proto JSON, e.g. '"ListContacts"') to the Mac and
    /// print the reply as JSON.
    #[command(hide = true)]
    Request { json: String },
}

/// One process is "primary" and does autofill and notifications: whoever holds this
/// lock (the daemon, if it runs; otherwise the window).
fn primary_lock() -> Option<File> {
    let dir = config::dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    let file = File::options().create(true).truncate(false).write(true).open(dir.join("primary.lock")).ok()?;
    file.try_lock().ok()?;
    Some(file)
}

/// Returns the lock (if this process is primary) and the running autofill engine.
fn start_primary(config: &config::Config) -> (Option<File>, Option<bb_autofill::Autofill>) {
    let Some(lock) = primary_lock() else {
        info!("another BlackBubbles process handles autofill and notifications");
        return (None, None);
    };
    if !config.autofill.enabled {
        return (Some(lock), None);
    }
    match bb_autofill::start(&config.autofill) {
        Ok(af) => (Some(lock), Some(af)),
        Err(err) => {
            warn!("autofill disabled: {err:#}");
            (Some(lock), None)
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
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tao=warn,wry=warn".into()))
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
        Some(Command::Request { json }) => {
            let request: bb_proto::Request = serde_json::from_str(&json).context("not a valid request")?;
            let config = config::Config::load()?;
            let client = conn::Client::new(runtime.handle().clone(), config.server.clone());
            let reply = runtime.block_on(async {
                for _ in 0..50 {
                    if matches!(client.status(), conn::Status::Connected { .. }) {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
                client.request(request).await
            });
            match reply {
                Ok(response) => println!("{}", serde_json::to_string_pretty(&response)?),
                Err(err) => println!("error: {err:?}"),
            }
            Ok(())
        }
        Some(Command::Daemon) => {
            let config = config::Config::load()?;
            let (lock, autofill) = start_primary(&config);
            let client = conn::Client::new(runtime.handle().clone(), config.server.clone());
            if let Some(af) = autofill.as_ref().filter(|_| config.autofill.from_server) {
                client.set_autofill(af.tx.clone());
            }
            let shared = std::sync::Arc::new(std::sync::RwLock::new(notify::Settings::from(&config)));
            let unfocused = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let _notifier = lock.is_some().then(|| {
                let _guard = runtime.enter();
                notify::Notifier::start(&client, shared.clone(), unfocused, autofill.is_some() && config.autofill.notify)
            });
            // Pick up notification settings changed in the window without a restart.
            let modified = || config::path().ok().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok());
            let mut seen = modified();
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
                            let now = modified();
                            if now != seen {
                                seen = now;
                                if let Ok(fresh) = config::Config::load() {
                                    *shared.write().unwrap() = notify::Settings::from(&fresh);
                                    info!("reloaded notification settings");
                                }
                            }
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
            let (lock, autofill) = start_primary(&config);
            let primary = lock.is_some();
            let on_exit = autofill.clone();
            let state = commands::AppState::new(runtime.handle().clone(), config, autofill.map(|af| af.tx), primary, startup_error);
            let result = run_gui(&runtime, state);
            if let Some(af) = on_exit {
                af.shutdown();
            }
            drop(lock);
            result
        }
    }
}

fn run_gui(runtime: &tokio::runtime::Runtime, state: commands::AppState) -> Result<()> {
    use tauri::Manager;
    // Tauri, the connection and the bb:// handler all share one runtime.
    tauri::async_runtime::set(runtime.handle().clone());
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .register_asynchronous_uri_scheme_protocol("bb", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<commands::AppState>();
                responder.respond(blob::handle(&state.blobs, state.client(), request).await);
            });
        })
        .setup(|app| {
            app.state::<commands::AppState>().connect(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(focused) = event {
                window.state::<commands::AppState>().focused.store(*focused, std::sync::atomic::Ordering::Relaxed);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::request,
            commands::send_attachment,
            commands::save_blob,
            commands::save_blob_as,
            commands::pair,
            commands::save_notifications,
            commands::save_services,
            commands::save_autofill,
            commands::autofill_http,
            commands::gmail_add,
            commands::gmail_remove,
            commands::audio_play,
            commands::audio_toggle,
            commands::audio_seek,
            commands::audio_stop,
            commands::audio_state,
        ])
        .run(tauri::generate_context!())
        .context("running the window")
}
