//! Verification code / link autofill, ported from
//! [linuxautofill](https://github.com/craftyderpysmall/linuxautofill) and built into
//! BlackBubbles so it no longer has to run separately.
//!
//! Sources (all feed one channel): iMessage/SMS from the Mac server, Gmail IMAP, and the
//! iPhone Shortcut HTTP endpoint. Each message goes through [`detect`], and anything found
//! is handed to [`deliver::Deliverer`].

pub mod config;
pub mod deliver;
pub mod detect;
pub mod http;
pub mod imap;
pub mod mail;
pub mod platform;

use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

pub use config::Config;
pub use detect::{DEFAULT_KEYWORDS, Link, find_code, find_code_with, find_link, plain_links};
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A text message (iMessage/SMS via the Mac, or the iPhone Shortcut).
    Sms,
    Mail,
}

#[derive(Debug, Clone)]
pub struct Incoming {
    pub source: Source,
    pub sender: String,
    pub text: String,
    pub links: Vec<Link>,
}

/// Start the configured sources and the delivery loop. Returns a sender for messages
/// from other sources (the client pushes iMessage/SMS from the Mac server here).
pub fn start(config: &Config) -> anyhow::Result<Sender<Incoming>> {
    let (tx, rx) = channel::<Incoming>();
    let keywords = detect::keyword_regex(&config.keywords());
    let deliverer = deliver::Deliverer::new(Arc::from(platform::detect()), Duration::from_secs(config.clear_after.max(5)));

    if let Some(http) = config.http.clone() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            if let Err(err) = http::run(&http.host, http.port, http.token, tx) {
                warn!("autofill HTTP endpoint stopped: {err:#}");
            }
        });
    }
    for account in config.gmail.clone() {
        let tx = tx.clone();
        std::thread::spawn(move || imap::run(account, tx));
    }

    std::thread::spawn(move || {
        for message in rx {
            let code = find_code_with(&message.text, &keywords);
            // Links only from email: "verify" links in texts are a common scam.
            let link = match (&code, message.source) {
                (None, Source::Mail) => find_link(&message.links, &message.text),
                _ => None,
            };
            match (code, link) {
                (Some(code), _) => deliverer.deliver_code(&code, &message.sender),
                (None, Some(link)) => deliverer.deliver_link(&link, &message.sender),
                (None, None) => info!("autofill: {:?} message had nothing to fill", message.source),
            }
        }
    });
    Ok(tx)
}
