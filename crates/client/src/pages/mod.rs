pub mod calendar;
pub mod findmy;
pub mod messages;
pub mod notes;
pub mod photos;
pub mod settings;
pub mod voice;

use std::sync::mpsc::Sender;

use bb_proto::BlobKind;

use crate::conn::Client;
use crate::media::Images;

/// What every page gets each frame.
pub struct Cx<'a> {
    pub client: &'a Client,
    pub images: &'a mut Images,
    pub connected: bool,
    pub notices: Sender<String>,
}

impl Cx<'_> {
    pub fn toast(&self, message: impl Into<String>) {
        let _ = self.notices.send(message.into());
    }

    /// Fetch a blob and save it to ~/Downloads, reporting the result as a toast.
    pub fn download(&self, kind: BlobKind, id: String, name: String) {
        let (client, notices) = (self.client.clone(), self.notices.clone());
        self.toast(format!("Downloading {name}…"));
        self.client.spawn(async move {
            let message = match client.blob(kind, id).await {
                Ok((data, _)) => match crate::util::save_download(&name, &data) {
                    Ok(path) => format!("Saved {}", path.display()),
                    Err(e) => format!("Couldn't save {name}: {e}"),
                },
                Err(e) => format!("Download failed: {}", e.message),
            };
            let _ = notices.send(message);
        });
    }
}
