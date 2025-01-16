use crate::proto::logproto;
use crate::proto::logproto::{LabelPairAdapter, StreamAdapter};
use crate::EventWrapper;
use eyre::{Context, Result};
use itertools::Itertools;
use parking_lot::Mutex;
use prost::Message;
use reqwest::{Client, Url};
use std::collections::{HashMap, VecDeque};
use std::mem;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, Level};

fn format_labels(mut labels: HashMap<String, String>, level: Level) -> String {
    let level = match level {
        Level::TRACE => "trace",
        Level::DEBUG => "debug",
        Level::INFO => "info",
        Level::WARN => "warn",
        Level::ERROR => "error",
    };

    labels.insert("level".into(), level.into());

    let labels = labels
        .into_iter()
        .map(|(k, v)| format!("{}={:?}", k, v))
        .collect::<Vec<_>>()
        .join(",");

    format!("{{{labels}}}")
}

pub struct SenderTask {
    buf: Arc<Mutex<VecDeque<EventWrapper>>>,
    url: Url,
    labels: HashMap<String, String>,
    fields: HashMap<String, String>,
    client: Client,
    current_req_data: Vec<u8>,
}

impl SenderTask {
    pub(crate) fn new(
        buf: Arc<Mutex<VecDeque<EventWrapper>>>,
        url: Url,
        labels: HashMap<String, String>,
        fields: HashMap<String, String>,
    ) -> Self {
        Self {
            buf,
            url,
            labels,
            fields,
            client: Client::new(),
            current_req_data: vec![],
        }
    }

    async fn run_once(&mut self) -> Result<()> {
        debug!("run_once");

        if self.current_req_data.is_empty() {
            let buf = mem::take(&mut *self.buf.lock());
            if buf.is_empty() {
                return Ok(());
            }

            debug!(len = buf.len(), "sending entries");

            let streams = buf
                .into_iter()
                .into_group_map_by(|e| e.level)
                .into_iter()
                .map(|(level, entry)| StreamAdapter {
                    labels: format_labels(self.labels.clone(), level),
                    entries: entry
                        .into_iter()
                        .map(|mut e| {
                            e.entry
                                .structured_metadata
                                .extend(self.fields.iter().map(|(k, v)| LabelPairAdapter {
                                    name: k.clone(),
                                    value: v.clone(),
                                }));
                            e.entry
                        })
                        .collect(),
                    ..Default::default()
                })
                .collect();

            let req = logproto::PushRequest { streams };

            self.current_req_data = snap::raw::Encoder::new()
                .compress_vec(&req.encode_to_vec())
                .unwrap();
        }

        self.client
            .post(self.url.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/x-snappy")
            .body(self.current_req_data.clone())
            .send()
            .await?
            .error_for_status()
            .context("push logs")?;

        self.current_req_data.clear();

        Ok(())
    }

    pub async fn run(mut self) {
        loop {
            if let Err(err) = self.run_once().await {
                error!(?err, "run_once");
            }
            sleep(Duration::from_secs(5)).await;
        }
    }
}
