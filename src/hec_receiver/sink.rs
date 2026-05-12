use std::path::PathBuf;
use tokio::{
    fs::{create_dir_all, OpenOptions},
    io::AsyncWriteExt,
    sync::Mutex,
};

use super::event::Event;

#[derive(Debug)]
pub enum Sink {
    Drop,
    CaptureFile {
        file: FileSink,
        write_lock: Mutex<()>,
    },
}

impl Sink {
    pub fn drop_events() -> Self {
        Self::Drop
    }

    pub fn capture_file(path: impl Into<PathBuf>) -> Self {
        Self::CaptureFile {
            file: FileSink::new(path),
            write_lock: Mutex::new(()),
        }
    }

    pub async fn submit_events(&self, events: &[Event]) -> std::io::Result<SinkOutcome> {
        match self {
            Self::Drop => Ok(SinkOutcome {
                accepted: events.len(),
                dropped: events.len(),
                written: 0,
            }),
            Self::CaptureFile { file, write_lock } => {
                let _guard = write_lock.lock().await;
                file.write_events(events).await?;
                Ok(SinkOutcome {
                    accepted: events.len(),
                    dropped: 0,
                    written: events.len(),
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkOutcome {
    pub accepted: usize,
    pub dropped: usize,
    pub written: usize,
}

#[derive(Debug)]
pub struct FileSink {
    path: PathBuf,
}

impl FileSink {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub async fn write_events(&self, events: &[Event]) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                create_dir_all(parent).await?;
            }
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        for event in events {
            let json = serde_json::to_vec(event).map_err(std::io::Error::other)?;
            file.write_all(&json).await?;
            file.write_all(b"\n").await?;
        }
        file.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hec_receiver::event::{Endpoint, Event};

    #[tokio::test]
    async fn file_sink_can_write_jsonl_when_connected_later() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let sink = FileSink::new(&path);
        let events = vec![Event::from_raw_line("hello".to_string(), Endpoint::Raw)];
        sink.write_events(&events).await.unwrap();
        let written = tokio::fs::read_to_string(path).await.unwrap();
        assert!(written.contains("\"raw\":\"hello\""));
    }

    #[tokio::test]
    async fn drop_sink_reports_dropped_events() {
        let sink = Sink::drop_events();
        let events = vec![Event::from_raw_line("hello".to_string(), Endpoint::Raw)];
        let report = sink.submit_events(&events).await.unwrap();
        assert_eq!(
            report,
            SinkOutcome {
                accepted: 1,
                dropped: 1,
                written: 0
            }
        );
    }
}
