use crate::proto::logproto::{EntryAdapter, LabelPairAdapter};
use crate::{EventWrapper, CAPACITY};
use parking_lot::Mutex;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::io::Write;
use std::sync::Arc;
use std::thread::LocalKey;
use std::time::SystemTime;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Record};
use tracing::{Event, Id, Subscriber};
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::{FormatEvent, FormatFields, MakeWriter};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

#[derive(Clone, Copy)]
struct BufWriter;

impl BufWriter {
    fn buf(&self) -> &'static LocalKey<RefCell<Vec<u8>>> {
        thread_local! {
            static BUF: RefCell<Vec<u8>> = const { RefCell::new(vec![]) };
        }

        &BUF
    }
}

impl<'a> MakeWriter<'a> for BufWriter {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        *self
    }
}

impl Write for BufWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buf().with_borrow_mut(|vec| vec.write(buf))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct Layer<S, N, E> {
    buf: Arc<Mutex<VecDeque<EventWrapper>>>,
    fmt_layer: fmt::Layer<S, N, E, BufWriter>,
}

impl<S, N, E> Layer<S, N, E> {
    pub(crate) fn new(
        sender: Arc<Mutex<VecDeque<EventWrapper>>>,
        fmt_layer: fmt::Layer<S, N, E>,
    ) -> Self {
        Self {
            buf: sender,
            fmt_layer: fmt_layer.with_writer(BufWriter),
        }
    }
}

#[derive(Default)]
struct Visitor {
    fields: Vec<LabelPairAdapter>,
}

impl Visit for Visitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields.push(LabelPairAdapter {
            name: field.name().into(),
            value: value.into(),
        })
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.record_str(field, &format!("{:?}", value));
    }
}

impl<S, N, E> tracing_subscriber::Layer<S> for Layer<S, N, E>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'writer> FormatFields<'writer> + 'static,
    E: FormatEvent<S, N> + 'static,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        {
            let span = ctx.span(id).expect("Span not found");
            if span.extensions_mut().get_mut::<Visitor>().is_none() {
                let mut visitor = Visitor::default();
                attrs.record(&mut visitor);
                span.extensions_mut().insert(visitor);
            }
        }

        self.fmt_layer.on_new_span(attrs, id, ctx);
    }

    fn on_record(&self, span: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        {
            let span = ctx.span(span).expect("Span not found");
            values.record(
                span.extensions_mut()
                    .get_mut::<Visitor>()
                    .expect("Visitor not found"),
            );
        }

        self.fmt_layer.on_record(span, values, ctx);
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if event
            .metadata()
            .module_path()
            .unwrap_or("")
            .starts_with(env!("CARGO_CRATE_NAME"))
        {
            return;
        }

        let mut visitor = Visitor { fields: vec![] };
        event.record(&mut visitor);

        let current_span = ctx.current_span();
        event
            .parent()
            .or(current_span.id())
            .and_then(|id| ctx.span_scope(id))
            .into_iter()
            .flat_map(|scope| scope.from_root())
            .for_each(|span| {
                visitor.fields.extend(
                    span.extensions()
                        .get::<Visitor>()
                        .map(|v| v.fields.clone())
                        .expect("Visitor not found"),
                )
            });

        self.fmt_layer.on_event(event, ctx);
        let line = String::from_utf8(self.fmt_layer.writer().buf().take()).unwrap();

        let entry = EntryAdapter {
            timestamp: Some(SystemTime::now().into()),
            line,
            structured_metadata: visitor.fields,
            parsed: vec![],
        };

        let mut buf = self.buf.lock();
        if buf.len() == CAPACITY {
            buf.pop_front();
        }
        buf.push_back(EventWrapper {
            entry,
            level: *event.metadata().level(),
        });
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        self.fmt_layer.on_enter(id, ctx)
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        self.fmt_layer.on_exit(id, ctx)
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        self.fmt_layer.on_close(id, ctx)
    }
}
