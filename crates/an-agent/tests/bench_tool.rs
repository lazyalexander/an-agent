//! Constructor overhead bench: native builtin (bash) vs rhai vs TS
//! subprocess (bun). Methodology borrowed from hyperfine/criterion: warmup,
//! N timed iterations, report cold / min / p50 / p95 / p99 / mean.
//! Wall-clock timing here is bench harness, not taped behavior — hence the
//! disallowed-methods allowance. Run: cargo test -p an-agent --test bench_tool -- --ignored --nocapture

// Bench helpers are not #[test] fns; wall-clock Instant is intentional here.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![allow(clippy::disallowed_methods)]

#[allow(dead_code)]
mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use an_agent::act::{ActCtx, Tool, ToolCall, ToolCtx, ToolTag, run_tool_act};
use an_agent::tools::{Bash, descriptor};
use serde_json::json;
use support::rhai::RhaiTool;
use support::ts_tool::SubprocessTool;

const WARMUP: usize = 5;
const RUNS: usize = 50;

const RHAI_YAML: &str = r#"
v: 1
name: echo_plus
version: 0.1.0
constructor: rhai
script: |
  params.x + 1
summary: Add one to x
effect:
  net: none
  file: { op: none }
  proc: none
  memory: { op: ignore }
requires: []
"#;

fn ctx() -> ToolCtx {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolCtx { signal: Some(rx) }
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx]
}

async fn time_call(
    actx: &ActCtx<'_>,
    tool: &Arc<dyn Tool>,
    call: &ToolCall,
    ctx: &ToolCtx,
) -> Duration {
    let tools = [tool.clone()];
    let t = Instant::now();
    run_tool_act(actx, &tools, call, ctx).await.unwrap();
    t.elapsed()
}

async fn bench(label: &str, call_name: &str, tool: &Arc<dyn Tool>, args: serde_json::Value) {
    let actx = ActCtx {
        store: None,
        agent_id: "bench",
        session: "s",
        card: None,
    };
    let call = ToolCall {
        id: "b".into(),
        name: call_name.into(),
        arguments: args.to_string(),
    };
    let ctx = ctx();
    let cold = time_call(&actx, tool, &call, &ctx).await;
    let mut warm = Vec::with_capacity(RUNS);
    for _ in 0..WARMUP + RUNS {
        warm.push(time_call(&actx, tool, &call, &ctx).await);
    }
    let mut warm = warm.split_off(WARMUP);
    warm.sort();
    let mean = warm.iter().sum::<Duration>() / warm.len() as u32;
    eprintln!(
        "{label:<10} cold {:>8.1?}  min {:>8.1?}  p50 {:>8.1?}  p95 {:>8.1?}  p99 {:>8.1?}  mean {:>8.1?}",
        cold,
        warm[0],
        percentile(&warm, 0.50),
        percentile(&warm, 0.95),
        percentile(&warm, 0.99),
        mean,
    );
}

#[tokio::test]
#[ignore = "bench; run explicitly"]
async fn constructor_overhead() {
    let bash: Arc<dyn Tool> = Arc::new(Bash::default());
    bench("bash", "bash", &bash, json!({ "command": "true" })).await;

    let d = descriptor::parse(RHAI_YAML).unwrap();
    let rhai: Arc<dyn Tool> =
        Arc::new(RhaiTool::from_descriptor(d, json!({ "type": "object" })).unwrap());
    bench("rhai", "echo_plus", &rhai, json!({ "x": 1 })).await;

    if std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("bun not found; ts constructor skipped");
        return;
    }
    let ts: Arc<dyn Tool> = SubprocessTool::bun_script(
        "word_count",
        "Count words and chars",
        json!({ "type": "object" }),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packages/ts-tools/word_count.ts"
        ),
        ToolTag::none(),
    );
    bench(
        "ts-bun",
        "word_count",
        &ts,
        json!({ "text": "hello world" }),
    )
    .await;
}
