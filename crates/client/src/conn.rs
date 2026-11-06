//! Connection to `bbd`: one background task owns the WebSocket, reconnects with backoff,
//! matches responses to requests and queues server events for the UI.

use std::collections::HashMap;
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use bb_proto::{ApiError, BlobKind, ClientFrame, ErrorCode, Event, PROTOCOL_VERSION, Request, Response, ServerFrame};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot};

use crate::config::ServerConfig;

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    NotPaired,
    Connecting,
    Connected { server_name: String },
    Disconnected(String),
}

type Reply = oneshot::Sender<Result<Response, ApiError>>;

struct Inner {
    status: Mutex<Status>,
    events: Mutex<Vec<Event>>,
    repaint: Mutex<Option<eframe::egui::Context>>,
    autofill: Mutex<Option<std_mpsc::Sender<bb_autofill::Incoming>>>,
}

#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
    tx: Option<mpsc::UnboundedSender<(Request, Reply)>>,
    rt: tokio::runtime::Handle,
}

/// A result the UI polls each frame.
pub struct Pending<T> {
    rx: std_mpsc::Receiver<T>,
    value: Option<T>,
}

impl<T> Pending<T> {
    pub fn ready(&mut self) -> Option<&T> {
        if self.value.is_none()
            && let Ok(v) = self.rx.try_recv()
        {
            self.value = Some(v);
        }
        self.value.as_ref()
    }

    pub fn take(&mut self) -> Option<T> {
        self.ready();
        self.value.take()
    }
}

fn offline() -> ApiError {
    ApiError::new(ErrorCode::Internal, "not connected to the Mac")
}

impl Client {
    pub fn new(rt: tokio::runtime::Handle, server: Option<ServerConfig>) -> Self {
        let inner = Arc::new(Inner {
            status: Mutex::new(if server.is_some() { Status::Connecting } else { Status::NotPaired }),
            events: Mutex::default(),
            repaint: Mutex::default(),
            autofill: Mutex::default(),
        });
        let tx = server.map(|server| {
            let (tx, rx) = mpsc::unbounded_channel();
            rt.spawn(run(inner.clone(), server, rx));
            tx
        });
        Self { inner, tx, rt }
    }

    pub fn set_repaint(&self, ctx: eframe::egui::Context) {
        *self.inner.repaint.lock().unwrap() = Some(ctx);
    }

    pub fn set_autofill(&self, tx: std_mpsc::Sender<bb_autofill::Incoming>) {
        *self.inner.autofill.lock().unwrap() = Some(tx);
    }

    pub fn status(&self) -> Status {
        self.inner.status.lock().unwrap().clone()
    }

    pub fn take_events(&self) -> Vec<Event> {
        std::mem::take(&mut *self.inner.events.lock().unwrap())
    }

    pub async fn request(&self, req: Request) -> Result<Response, ApiError> {
        let tx = self.tx.as_ref().ok_or_else(offline)?;
        let (reply_tx, reply_rx) = oneshot::channel();
        tx.send((req, reply_tx)).map_err(|_| offline())?;
        reply_rx.await.map_err(|_| offline())?
    }

    /// Fire a request; the result arrives in the returned [`Pending`] and triggers a repaint.
    pub fn call(&self, req: Request) -> Pending<Result<Response, ApiError>> {
        let (tx, rx) = std_mpsc::channel();
        let this = self.clone();
        self.rt.spawn(async move {
            let _ = tx.send(this.request(req).await);
            this.repaint();
        });
        Pending { rx, value: None }
    }

    /// Download a whole blob in chunks.
    pub async fn blob(&self, kind: BlobKind, id: String) -> Result<(Vec<u8>, Option<String>), ApiError> {
        const CHUNK: u32 = 1024 * 1024;
        let mut out = Vec::new();
        let mut mime = None;
        loop {
            let req = Request::GetBlob { kind, id: id.clone(), offset: out.len() as u64, len: CHUNK };
            match self.request(req).await? {
                Response::Blob { data, total, mime_type } => {
                    mime = mime.or(mime_type);
                    let done = data.is_empty() || out.len() as u64 + data.len() as u64 >= total;
                    out.extend_from_slice(&data);
                    if done {
                        return Ok((out, mime));
                    }
                }
                other => return Err(ApiError::internal(format!("unexpected reply {other:?}"))),
            }
        }
    }

    pub fn fetch_blob(&self, kind: BlobKind, id: String) -> Pending<Result<(Vec<u8>, Option<String>), ApiError>> {
        let (tx, rx) = std_mpsc::channel();
        let this = self.clone();
        self.rt.spawn(async move {
            let _ = tx.send(this.blob(kind, id).await);
            this.repaint();
        });
        Pending { rx, value: None }
    }

    pub fn spawn<F: std::future::Future<Output = ()> + Send + 'static>(&self, f: F) {
        self.rt.spawn(f);
    }

    fn repaint(&self) {
        if let Some(ctx) = &*self.inner.repaint.lock().unwrap() {
            ctx.request_repaint();
        }
    }
}

impl Inner {
    fn set_status(&self, status: Status) {
        *self.status.lock().unwrap() = status;
        self.repaint();
    }

    fn repaint(&self) {
        if let Some(ctx) = &*self.repaint.lock().unwrap() {
            ctx.request_repaint();
        }
    }

    fn push_event(&self, event: Event) {
        if let Event::IncomingText { sender, text, .. } = &event
            && let Some(tx) = &*self.autofill.lock().unwrap()
        {
            let _ = tx.send(bb_autofill::Incoming {
                source: bb_autofill::Source::Sms,
                sender: sender.clone(),
                text: text.clone(),
                links: vec![],
            });
        }
        self.events.lock().unwrap().push(event);
        self.repaint();
    }
}

async fn run(inner: Arc<Inner>, server: ServerConfig, mut rx: mpsc::UnboundedReceiver<(Request, Reply)>) {
    let mut backoff = Duration::from_secs(1);
    loop {
        inner.set_status(Status::Connecting);
        let mut connected = false;
        let result = session(&inner, &server, &mut rx, &mut connected).await;
        match result {
            Ok(()) => return, // the client was dropped
            Err(err) => inner.set_status(Status::Disconnected(format!("{err:#}"))),
        }
        if connected {
            backoff = Duration::from_secs(1);
        }
        // While waiting, fail requests fast instead of letting the UI hang.
        let sleep = tokio::time::sleep(backoff);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                _ = &mut sleep => break,
                next = rx.recv() => match next {
                    Some((_, reply)) => { let _ = reply.send(Err(offline())); }
                    None => return,
                },
            }
        }
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

async fn session(
    inner: &Inner,
    server: &ServerConfig,
    rx: &mut mpsc::UnboundedReceiver<(Request, Reply)>,
    connected: &mut bool,
) -> Result<()> {
    let pin = server.fingerprint.parse()?;
    let mut ws = tokio::time::timeout(Duration::from_secs(15), bb_net::connect(&server.host, server.port, pin))
        .await
        .map_err(|_| anyhow!("timed out connecting to {}", server.host))??;
    bb_net::send(&mut ws, &ClientFrame::Hello { protocol: PROTOCOL_VERSION, device_token: server.token.clone() }).await?;
    match bb_net::recv::<_, ServerFrame>(&mut ws).await? {
        ServerFrame::Welcome { server_name, .. } => inner.set_status(Status::Connected { server_name }),
        ServerFrame::Rejected(err) => bail!("{}", err.message),
        other => bail!("unexpected greeting {other:?}"),
    }
    *connected = true;

    let (mut sink, mut stream) = ws.split();
    let mut pending: HashMap<u64, Reply> = HashMap::new();
    let mut next_id = 0u64;
    let mut ping = tokio::time::interval(Duration::from_secs(20));
    loop {
        tokio::select! {
            next = rx.recv() => {
                let Some((req, reply)) = next else { return Ok(()) };
                next_id += 1;
                pending.insert(next_id, reply);
                sink.send(bb_net::to_message(&ClientFrame::Request { id: next_id, req })?).await?;
            }
            message = stream.next() => {
                let message = message.ok_or_else(|| anyhow!("server closed the connection"))??;
                match bb_net::from_message::<ServerFrame>(&message)? {
                    Some(ServerFrame::Response { id, res }) => {
                        if let Some(reply) = pending.remove(&id) {
                            let _ = reply.send(res);
                        }
                    }
                    Some(ServerFrame::Event(event)) => inner.push_event(event),
                    Some(ServerFrame::Rejected(err)) => bail!("{}", err.message),
                    _ => {}
                }
            }
            _ = ping.tick() => {
                sink.send(bb_net::tokio_tungstenite::tungstenite::Message::Ping(Vec::new().into())).await?;
            }
        }
    }
}

/// Pair with a server using a `blackbubbles://` string from `bbd pair`.
pub async fn pair(uri: &str, device_name: &str) -> Result<ServerConfig> {
    let uri: bb_net::PairUri = uri.parse()?;
    let code = uri.code.clone().ok_or_else(|| anyhow!("pairing string has no code; run `bbd pair` again"))?;
    let mut ws = bb_net::connect(&uri.host, uri.port, uri.fingerprint).await?;
    bb_net::send(&mut ws, &ClientFrame::Pair { protocol: PROTOCOL_VERSION, code, device_name: device_name.into() }).await?;
    let token = match bb_net::recv::<_, ServerFrame>(&mut ws).await? {
        ServerFrame::Paired { device_token } => device_token,
        ServerFrame::Rejected(err) => bail!("{}", err.message),
        other => bail!("unexpected reply {other:?}"),
    };
    Ok(ServerConfig { host: uri.host, port: uri.port, fingerprint: uri.fingerprint.to_string(), token })
}

pub fn device_name() -> String {
    let host = host_name();
    let os = if cfg!(target_os = "macos") { "macOS" } else { "Linux" };
    if host.is_empty() { format!("BlackBubbles ({os})") } else { format!("{host} ({os})") }
}

/// The machine's host name, without depending on a `hostname` binary.
pub fn host_name() -> String {
    let from_proc = std::fs::read_to_string("/proc/sys/kernel/hostname").ok();
    let from_etc = || std::fs::read_to_string("/etc/hostname").ok();
    let from_cmd = || {
        let cmd = if cfg!(target_os = "macos") { ("scutil", vec!["--get", "ComputerName"]) } else { ("hostname", vec![]) };
        std::process::Command::new(cmd.0).args(cmd.1).output().ok().and_then(|o| String::from_utf8(o.stdout).ok())
    };
    from_proc.or_else(from_etc).or_else(from_cmd).map(|s| s.trim().to_string()).unwrap_or_default()
}
