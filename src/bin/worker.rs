use resonate::prelude::*;
use serde_json::{json, Value};

/// Durable workflow registered as `foo`.
///
/// The gateway dispatches work to this name via `rpc(...).spawn()`. All inputs
/// and the return value must be JSON-serializable. Real workloads would call
/// `ctx.run(step_fn, ...)` for checkpointed side effects (DB writes, external
/// APIs, long computations) — each step is durably recorded so the workflow
/// can resume from the last successful step after a crash or restart.
#[resonate::function]
async fn foo(_ctx: &Context, data: Value) -> Result<Value> {
    println!("processing on worker: {data}");

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    Ok(json!({
        "result": format!("Processed: {data}"),
        "timestamp": now_ms,
    }))
}

#[tokio::main]
async fn main() {
    let resonate = Resonate::new(ResonateConfig {
        url: Some("http://localhost:8001".into()),
        group: Some("worker".into()),
        ..Default::default()
    });

    resonate.register(foo).unwrap();

    println!("worker running, waiting for tasks...");
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl-c");
}
