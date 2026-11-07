//! `bbd`: the BlackBubbles server. Run it on the Mac that's signed in to your Apple account.

mod auth;
mod mock;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use bb_proto::{ApiError, Backend, ClientFrame, ErrorCode, Event, PROTOCOL_VERSION, ServerFrame};
use clap::{Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc};
use tracing::{info, warn};

#[derive(Parser)]
#[command(name = "bbd", version, about = "BlackBubbles server")]
struct Cli {
    /// Where the certificate, paired devices and caches live.
    #[arg(long, global = true, env = "BBD_DATA_DIR")]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve clients.
    Run {
        #[arg(long, default_value = "0.0.0.0:7878")]
        bind: SocketAddr,
        /// Serve fake data (works on any OS; for development).
        #[arg(long)]
        mock: bool,
        /// With --mock: send a fake verification code text every N seconds.
        #[arg(long, value_name = "SECS")]
        mock_codes: Option<u64>,
        /// Name shown to clients.
        #[arg(long)]
        name: Option<String>,
    },
    /// Print a one-time pairing string for a new client.
    Pair {
        /// Host name or IP clients should use to reach this Mac (e.g. its Tailscale name).
        #[arg(long)]
        host: Option<String>,
        #[arg(long, default_value_t = bb_net::DEFAULT_PORT)]
        port: u16,
    },
    /// List paired devices.
    Devices,
    /// Remove a paired device by name.
    Revoke { name: String },
    /// Sign in to iCloud for Find My (interactive, asks for the 2FA code).
    FindmyLogin {
        apple_id: String,
        /// Also sign in to the Offline-Finding service so beacons (your Linux PC) can be located.
        #[arg(long)]
        beacons: bool,
        /// Anisette server URL. Defaults to this Mac's own anisette on macOS.
        #[arg(long)]
        anisette_url: Option<String>,
    },
}

fn data_dir(cli: &Cli) -> Result<PathBuf> {
    if let Some(dir) = &cli.data_dir {
        return Ok(dir.clone());
    }
    let dirs = directories::ProjectDirs::from("", "", "BlackBubbles")
        .context("cannot determine a data directory; pass --data-dir")?;
    Ok(dirs.data_dir().join("server"))
}

/// This Mac's name for pairing strings: its Bonjour name (`<LocalHostName>.local`).
fn local_host_name() -> String {
    let run = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd).args(args).output().ok().and_then(|o| String::from_utf8(o.stdout).ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    };
    run("scutil", &["--get", "LocalHostName"])
        .map(|n| format!("{n}.local"))
        .or_else(|| std::fs::read_to_string("/proc/sys/kernel/hostname").ok().map(|s| s.trim().to_string()))
        .or_else(|| run("hostname", &[]))
        .unwrap_or_else(|| "localhost".into())
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let cli = Cli::parse();
    let dir = data_dir(&cli)?;
    std::fs::create_dir_all(&dir)?;
    let store = auth::Store::new(&dir);

    match cli.command {
        Command::Pair { host, port } => {
            let identity = bb_net::Identity::load_or_create(&dir)?;
            let code = store.new_pairing_code()?;
            let uri = bb_net::PairUri {
                host: host.unwrap_or_else(local_host_name),
                port,
                fingerprint: identity.fingerprint(),
                code: Some(code),
            };
            println!("Paste this into BlackBubbles → Settings → Pair (valid for 10 minutes, single use):\n");
            println!("  {uri}\n");
            println!("Certificate fingerprint: {}", identity.fingerprint());
            Ok(())
        }
        Command::Devices => {
            for device in store.devices() {
                println!("{}\t(paired at unix {})", device.name, device.paired_at);
            }
            Ok(())
        }
        Command::Revoke { name } => {
            let removed = store.revoke(&name)?;
            println!("removed {removed} device(s)");
            Ok(())
        }
        Command::FindmyLogin { apple_id, beacons, anisette_url } => {
            let password = rpassword_prompt("Apple ID password: ")?;
            let icloud_dir = dir.join("icloud");
            eprintln!("{CODE_HELP}");
            bb_findmy::icloud::interactive_login(&icloud_dir, &apple_id, &password, ask)?;
            println!("Signed in to iCloud. Your Apple devices will now appear in Find My.");
            if beacons {
                if let Some(url) = &anisette_url {
                    std::fs::write(dir.join("anisette_url"), url)?;
                }
                let provider = anisette_provider(&dir)?;
                eprintln!("\nSigning in a second time for Offline Finding (locating this PC's beacon). This needs a new code.");
                let auth = bb_findmy::gsa::login(&*provider, &apple_id, &password, || {
                    ask("Enter the new 6-digit code from your Apple device: ")
                })?;
                bb_net::write_private(&icloud_dir.join("searchparty.json"), &serde_json::to_vec_pretty(&auth)?)?;
                println!("Signed in to Offline Finding. Registered beacons will now be located.");
            }
            Ok(())
        }
        Command::Run { bind, mock, mock_codes, name } => {
            let backend: Arc<dyn Backend> = if mock {
                Arc::new(mock::MockBackend::new(mock_codes.map(Duration::from_secs)))
            } else {
                real_backend(&dir)?
            };
            let identity = bb_net::Identity::load_or_create(&dir)?;
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(serve(bind, identity, store, backend, name.unwrap_or_else(local_host_name)))
        }
    }
}

/// Read a line without echoing it when stdin is a terminal.
const CODE_HELP: &str = "\
If Apple asks for a code and no pop-up appears on your devices, make one yourself:
  iPhone/iPad: Settings → your name → Sign-In & Security → Two-Factor Authentication → Get Verification Code
  Mac:         System Settings → your name → Sign-In & Security → Two-Factor Authentication → Get Verification Code
";

/// Read one visible line (for 2FA codes).
fn ask(prompt: &str) -> Result<String> {
    use std::io::{BufRead, Write};
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn rpassword_prompt(prompt: &str) -> Result<String> {
    use std::io::{BufRead, Write};
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    let tty = std::io::IsTerminal::is_terminal(&std::io::stdin());
    if tty {
        let _ = std::process::Command::new("stty").arg("-echo").status();
    }
    let mut line = String::new();
    let result = std::io::stdin().lock().read_line(&mut line);
    if tty {
        let _ = std::process::Command::new("stty").arg("echo").status();
        eprintln!();
    }
    result?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

/// The anisette source: `anisette_url` in the data dir if set, else this Mac's own.
pub fn anisette_provider(dir: &std::path::Path) -> Result<Box<dyn bb_findmy::anisette::Provider>> {
    if let Ok(url) = std::fs::read_to_string(dir.join("anisette_url")) {
        return Ok(Box::new(bb_findmy::anisette::Remote::new(url.trim())));
    }
    #[cfg(target_os = "macos")]
    return Ok(Box::new(bb_mac::anisette::Native::new()?));
    #[cfg(not(target_os = "macos"))]
    bail!("no anisette source; pass --anisette-url (e.g. a self-hosted anisette-v3-server)")
}

#[cfg(target_os = "macos")]
fn real_backend(dir: &std::path::Path) -> Result<Arc<dyn Backend>> {
    Ok(Arc::new(bb_mac::MacBackend::new(dir)?))
}

#[cfg(not(target_os = "macos"))]
fn real_backend(_dir: &std::path::Path) -> Result<Arc<dyn Backend>> {
    bail!("the real backend reads Apple data and only runs on macOS; use `bbd run --mock` here")
}

struct Shared {
    store: auth::Store,
    backend: Arc<dyn Backend>,
    events: broadcast::Sender<Event>,
    server_name: String,
}

async fn serve(
    bind: SocketAddr,
    identity: bb_net::Identity,
    store: auth::Store,
    backend: Arc<dyn Backend>,
    server_name: String,
) -> Result<()> {
    let acceptor = identity.acceptor()?;
    let (events, _) = broadcast::channel(1024);
    let sender = events.clone();
    backend.start_events(Arc::new(move |event| {
        let _ = sender.send(event);
    }));
    let shared = Arc::new(Shared { store, backend, events, server_name });
    let listener = tokio::net::TcpListener::bind(bind).await.with_context(|| format!("binding {bind}"))?;
    info!("listening on {bind}, fingerprint {}", identity.fingerprint());
    if shared.store.devices().is_empty() {
        info!("no devices paired yet; run `bbd pair` to get a pairing string");
    }
    loop {
        let (tcp, peer) = listener.accept().await?;
        let (acceptor, shared) = (acceptor.clone(), shared.clone());
        tokio::spawn(async move {
            if let Err(err) = session(tcp, peer, &acceptor, shared).await {
                info!("{peer}: session ended: {err:#}");
            }
        });
    }
}

async fn reject(ws: &mut bb_net::ServerStream, code: ErrorCode, message: &str) -> Result<()> {
    // Slow down guessing.
    tokio::time::sleep(Duration::from_secs(1)).await;
    bb_net::send(ws, &ServerFrame::Rejected(ApiError::new(code, message))).await?;
    let _ = ws.close(None).await;
    bail!("{message}")
}

async fn session(
    tcp: tokio::net::TcpStream,
    peer: SocketAddr,
    acceptor: &bb_net::tokio_rustls::TlsAcceptor,
    shared: Arc<Shared>,
) -> Result<()> {
    let mut ws = tokio::time::timeout(Duration::from_secs(10), bb_net::accept(tcp, acceptor)).await??;
    let first: ClientFrame = tokio::time::timeout(Duration::from_secs(10), bb_net::recv(&mut ws)).await??;

    let device = match first {
        ClientFrame::Hello { protocol, .. } | ClientFrame::Pair { protocol, .. }
            if protocol != PROTOCOL_VERSION =>
        {
            return reject(&mut ws, ErrorCode::BadRequest, "protocol version mismatch; update BlackBubbles").await;
        }
        ClientFrame::Hello { device_token, .. } => match shared.store.check_token(&device_token) {
            Some(name) => name,
            None => {
                warn!("{peer}: rejected unknown device token");
                return reject(&mut ws, ErrorCode::Unauthorized, "unknown device; pair again").await;
            }
        },
        ClientFrame::Pair { code, device_name, .. } => match shared.store.pair(&code, &device_name) {
            Some(token) => {
                info!("{peer}: paired new device {device_name:?}");
                bb_net::send(&mut ws, &ServerFrame::Paired { device_token: token }).await?;
                device_name
            }
            None => {
                warn!("{peer}: rejected pairing attempt");
                return reject(&mut ws, ErrorCode::Unauthorized, "invalid or expired pairing code").await;
            }
        },
        ClientFrame::Request { .. } => {
            return reject(&mut ws, ErrorCode::Unauthorized, "authenticate first").await;
        }
    };
    bb_net::send(&mut ws, &ServerFrame::Welcome {
        protocol: PROTOCOL_VERSION,
        server_name: shared.server_name.clone(),
    })
    .await?;
    info!("{peer}: {device:?} connected");

    let (mut sink, mut stream) = ws.split();
    let (out_tx, mut out_rx) = mpsc::channel::<ServerFrame>(256);
    let writer = tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            let message = bb_net::to_message(&frame)?;
            sink.send(message).await?;
        }
        anyhow::Ok(())
    });

    let mut events = shared.events.subscribe();
    let event_tx = out_tx.clone();
    let forwarder = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if event_tx.send(ServerFrame::Event(event)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => warn!("client lagged; dropped {n} events"),
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let result = async {
        while let Some(message) = stream.next().await {
            let Some(frame) = bb_net::from_message::<ClientFrame>(&message?)? else { continue };
            let ClientFrame::Request { id, req } = frame else { continue };
            let (backend, out_tx) = (shared.backend.clone(), out_tx.clone());
            tokio::spawn(async move {
                let res = tokio::task::spawn_blocking(move || backend.handle(req))
                    .await
                    .unwrap_or_else(|err| Err(ApiError::internal(format!("handler panicked: {err}"))));
                let _ = out_tx.send(ServerFrame::Response { id, res }).await;
            });
        }
        anyhow::Ok(())
    }
    .await;
    forwarder.abort();
    writer.abort();
    info!("{peer}: {device:?} disconnected");
    result
}
