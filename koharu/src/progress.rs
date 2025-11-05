use serde::Serialize;
use tauri::ipc::Channel;

// refer: https://v2.tauri.app/develop/calling-frontend/#channels
#[derive(Debug, Serialize)]
#[serde(tag = "event", content = "data")]
pub enum DownloadEvent {
    Started {
        url: String,
        total: usize,
    },
    Progress {
        url: String,
        current: usize,
        total: usize,
    },
    Finished {
        url: String,
    },
}

pub struct Emitter {
    url: String,
    total: usize,
    channel: Channel<DownloadEvent>,
}

impl Emitter {
    pub fn new(url: impl Into<String>, channel: Channel<DownloadEvent>) -> Self {
        Self {
            url: url.into(),
            total: 0,
            channel,
        }
    }
}

// refer: https://github.com/huggingface/hf-hub/blob/c165283bf78a06ec2a227c7f40da092d59adbd87/examples/iced/src/main.rs#L42
impl hf_hub::api::tokio::Progress for Emitter {
    async fn init(&mut self, size: usize, _filename: &str) {
        self.total = size;
        let _ = self.channel.send(DownloadEvent::Started {
            url: self.url.clone(),
            total: size,
        });
    }

    async fn update(&mut self, size: usize) {
        let _ = self.channel.send(DownloadEvent::Progress {
            url: self.url.clone(),
            current: size,
            total: self.total,
        });
    }

    async fn finish(&mut self) {
        let _ = self.channel.send(DownloadEvent::Finished {
            url: self.url.clone(),
        });
    }
}
