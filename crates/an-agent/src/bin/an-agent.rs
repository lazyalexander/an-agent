use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;

use an_agent::act::{Tool, ToolCtx};
use an_agent::agent::{last_assistant_text, run_until_idle, AgentState, ChatMessage};
use an_agent::memstream::{AppendEvent, FromKind, JsonlStore, Kind};
use an_agent::model::{load_settings, ChatCompletions};
use an_agent::principal::{local_agent_id, stdin_counterpart_id};
use an_agent::tools::Bash;
use anyhow::{Context, Result};
use tokio::sync::watch;

#[tokio::main]
async fn main() -> Result<()> {
    let root = home_dir().join(".an-agent");
    let agent_id = local_agent_id(&root)?;
    let stdin_from = stdin_counterpart_id(agent_id);
    let session = uuid::Uuid::new_v4();
    let mem_path = root
        .join("agents")
        .join(agent_id.to_string())
        .join("memory.jsonl");
    let store = JsonlStore::open(&mem_path)?;
    let settings = load_settings().context("model settings")?;
    let model = ChatCompletions::new(settings);
    let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(Bash::default())];

    let mut state = AgentState {
        messages: vec![ChatMessage {
            role: "system".into(),
            content: Some("You are a helpful assistant.".into()),
            tool_calls: None,
            tool_call_id: None,
        }],
    };

    println!("an-agent (rust). /exit to quit.");
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        if text == "/exit" {
            break;
        }
        store.append(AppendEvent {
            from: stdin_from.to_string(),
            from_kind: FromKind::Unknown,
            kind: Kind::Utterance,
            session: session.to_string(),
            content: text.into(),
            tags: vec![],
            refs: vec![],
            act: None,
        })?;
        state.messages.push(ChatMessage {
            role: "user".into(),
            content: Some(text.into()),
            tool_calls: None,
            tool_call_id: None,
        });
        let (_tx, rx) = watch::channel(false);
        let ctx = ToolCtx { signal: Some(rx) };
        match run_until_idle(
            state,
            Some(&store),
            &agent_id.to_string(),
            &session.to_string(),
            &model,
            &tools,
            &ctx,
            16,
        )
        .await
        {
            Ok(next) => {
                println!("{}", last_assistant_text(&next));
                state = next;
            }
            Err(e) => {
                writeln!(io::stderr(), "{e}")?;
                break;
            }
        }
    }
    Ok(())
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
