use crate::layer::Layer;
use crate::proto::logproto::EntryAdapter;
use crate::task::SenderTask;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::Level;
use tracing_subscriber::fmt;
use url::Url;

mod layer;
mod task;

pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/proto.rs"));
}

const CAPACITY: usize = 1024;

struct EventWrapper {
    entry: EntryAdapter,
    level: Level,
}

pub struct Builder<S, N, E> {
    url: Url,
    fmt_layer: fmt::Layer<S, N, E>,
    labels: HashMap<String, String>,
    fields: HashMap<String, String>,
}

impl<S, N, E> Builder<S, N, E> {
    pub fn new(
        url: impl AsRef<str>,
        fmt_layer: fmt::Layer<S, N, E>,
    ) -> Result<Self, url::ParseError> {
        Ok(Self {
            url: Url::parse(url.as_ref())?,
            fmt_layer,
            labels: HashMap::new(),
            fields: HashMap::new(),
        })
    }

    pub fn add_label(mut self, key: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        self.add_label_mut(key, value);
        self
    }

    pub fn add_label_mut(&mut self, key: impl AsRef<str>, value: impl AsRef<str>) -> &mut Self {
        self.labels
            .insert(key.as_ref().into(), value.as_ref().into());
        self
    }

    pub fn add_field(mut self, key: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        self.add_field_mut(key, value);
        self
    }

    pub fn add_field_mut(&mut self, key: impl AsRef<str>, value: impl AsRef<str>) -> &mut Self {
        self.fields
            .insert(key.as_ref().into(), value.as_ref().into());
        self
    }

    pub fn build(self) -> (Layer<S, N, E>, SenderTask) {
        let buf = Arc::new(Mutex::new(Default::default()));

        (
            Layer::new(buf.clone(), self.fmt_layer),
            SenderTask::new(buf, self.url, self.labels, self.fields),
        )
    }
}
