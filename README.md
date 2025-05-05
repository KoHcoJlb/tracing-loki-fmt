Layer that pushes logs formatted with `tracing-subscriber` into [Grafana Loki](https://grafana.com/oss/loki/) and
attaching all fields
as [Loki's structured metadata](https://grafana.com/docs/loki/latest/get-started/labels/structured-metadata/).

It combines both log readability and convenient manipulation (filtering ` | field = "value"`, aggregation, etc)
without the need for parsing.

Example
---

```rust
use eyre::Result;
use std::time::Duration;
use tokio::spawn;
use tokio::time::sleep;
use tracing::{info, info_span};
use tracing_loki_fmt::Builder;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, registry};

#[tokio::main]
async fn main() -> Result<()> {
    let builder = Builder::new(
        "http://grafana.proxmox/loki/api/v1/push",
        fmt::layer().without_time(),
    )?;
    let (layer, task) = builder
        .add_label("this_is_label", "test456")
        .add_label("job", "test")
        .add_field("this_is_static_field", "Test6666")
        .build();
    spawn(task.run());

    registry()
        .with(fmt::layer())
        .with(layer)
        .init();

    let _span1 = info_span!("span1", hello = "world").entered();
    let _span2 = info_span!("span2", world = "test").entered();

    info!(test = 123, test1 = "456", "hello world");

    sleep(Duration::from_secs(15)).await;

    Ok(())
}
```
