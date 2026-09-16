//! Disposable live ReAct probe: real DeepSeek API + live web_search tool +
//! active `remember`, observing the memstream tape (linkage, order, and how
//! much of the run the tape alone can reproduce).
//!
//! DeepSeek's API currently ignores its built-in `web_search` tool (official
//! Responses API compatibility table), so search is a local tool over keyless
//! endpoints (DuckDuckGo Instant Answer, Wikipedia fallback).
//!
//! Run explicitly (needs network and a DeepSeek key):
//!   cargo test -p an-agent --test react_live -- --ignored --nocapture
//! Key resolution: $MODEL_API_KEY, else MODEL_API_KEY from the workspace .env.
//! The key is loaded into process memory and never printed.
//! Probe materials: set AN_AGENT_PROBE_DIR to keep the tape at a chosen
//! location; otherwise a self-cleaning temp dir is used (the tape is still
//! printed to stderr).

// Helper fns here are not #[test] fns, so allow-*-in-tests does not reach them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

#[allow(dead_code)]
mod support;

use an_agent::act::{
    ActCtx, ActEnvelope, ActKind, ActSentence, BareFile, Ingest, Permit, Tool, ToolCtx, ToolTag,
};
use an_agent::det_seam::Entropy;
use an_agent::memstream::{ActOnEvent, AppendEvent, FromKind, JsonlStore, Kind, Memevent};
use serde_json::{Value, json};
use support::{
    AgentState, ChatCompletions, ChatMessage, ModelSettings, TempDir, last_assistant_text,
    run_until_idle,
};

const SYSTEM: &str = "You are a ReAct agent. Think briefly, then act. \
Available tools: web_search (search the live web), remember (store a durable \
fact into long-term memory). Rules: for questions about current or factual \
matters, call web_search first; when you have learned a durable fact, call \
remember before giving your final answer.";

const QUESTION: &str = "Who created the Rust programming language, when, and \
while working at which company? Search the web to confirm, remember the key \
fact, then answer.";

fn api_key() -> Option<String> {
    if let Ok(k) = std::env::var("MODEL_API_KEY")
        && !k.is_empty()
    {
        return Some(k);
    }
    let env_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.env");
    let raw = std::fs::read_to_string(env_path).ok()?;
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some(v) = line.strip_prefix("MODEL_API_KEY=") {
            let v = v.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn settings(api_key: String) -> ModelSettings {
    let cfg_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/model.json");
    let cfg: Value =
        serde_json::from_str(&std::fs::read_to_string(cfg_path).expect("read config/model.json"))
            .expect("parse config/model.json");
    ModelSettings {
        base_url: cfg
            .get("baseUrl")
            .and_then(Value::as_str)
            .expect("baseUrl")
            .into(),
        model: cfg
            .get("model")
            .and_then(Value::as_str)
            .expect("model")
            .into(),
        api_key: Some(api_key),
        extra_body: cfg.get("extraBody").cloned(),
    }
}

fn probe_dir() -> (PathBuf, Option<TempDir>) {
    if let Ok(dir) = std::env::var("AN_AGENT_PROBE_DIR") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("create probe dir");
        return (dir, None);
    }
    let tmp = TempDir::new("react");
    (tmp.path().to_path_buf(), Some(tmp))
}

struct WebSearch;

#[async_trait::async_trait]
impl Tool for WebSearch {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the live web and return a short digest. Use it for current \
events, fresh facts, or anything beyond your training knowledge."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "search query" }
            },
            "required": ["query"]
        })
    }

    fn tag_seed(&self) -> Option<ToolTag> {
        Some(ToolTag::none())
    }

    async fn execute(&self, args: Value, _ctx: &ToolCtx) -> Result<String, String> {
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .ok_or("missing query")?;
        search_digest(query).await
    }
}

async fn search_digest(query: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("an-agent-react-probe/0.1")
        .build()
        .map_err(|e| e.to_string())?;
    let ddg: Value = client
        .get("https://api.duckduckgo.com/")
        .query(&[
            ("q", query),
            ("format", "json"),
            ("no_html", "1"),
            ("skip_disambig", "1"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let mut parts = Vec::new();
    if let Some(a) = ddg
        .get("AbstractText")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        let url = ddg.get("AbstractURL").and_then(Value::as_str).unwrap_or("");
        parts.push(format!("{a} ({url})"));
    }
    if let Some(ans) = ddg
        .get("Answer")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("answer: {ans}"));
    }
    if let Some(topics) = ddg.get("RelatedTopics").and_then(Value::as_array) {
        for t in topics.iter().take(3) {
            if let Some(text) = t.get("Text").and_then(Value::as_str) {
                parts.push(text.to_string());
            }
        }
    }
    if !parts.is_empty() {
        return Ok(parts.join("\n"));
    }
    let hits: Value = client
        .get("https://en.wikipedia.org/w/api.php")
        .query(&[
            ("action", "query"),
            ("list", "search"),
            ("srsearch", query),
            ("srlimit", "3"),
            ("format", "json"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let mut parts = Vec::new();
    if let Some(search) = hits
        .get("query")
        .and_then(|q| q.get("search"))
        .and_then(Value::as_array)
    {
        for hit in search.iter().take(3) {
            let title = hit.get("title").and_then(Value::as_str).unwrap_or("");
            let snippet = hit
                .get("snippet")
                .and_then(Value::as_str)
                .unwrap_or("")
                .replace("<span class=\"searchmatch\">", "")
                .replace("</span>", "");
            if !snippet.is_empty() {
                parts.push(format!("{title}: {snippet}"));
            }
        }
    }
    if parts.is_empty() {
        return Err("no results".into());
    }
    Ok(parts.join("\n"))
}

struct Remember {
    store: Arc<JsonlStore>,
    agent_id: String,
    session: String,
}

#[async_trait::async_trait]
impl Tool for Remember {
    fn name(&self) -> &str {
        "remember"
    }

    fn description(&self) -> &str {
        "Deliberately store a durable fact into long-term memory. Call this \
when you learned something worth keeping."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "the fact to remember" }
            },
            "required": ["text"]
        })
    }

    fn tag_seed(&self) -> Option<ToolTag> {
        Some(ToolTag::none())
    }

    async fn execute(&self, args: Value, _ctx: &ToolCtx) -> Result<String, String> {
        let text = args
            .get("text")
            .and_then(Value::as_str)
            .ok_or("missing text")?;
        let env = ActEnvelope {
            kind: ActKind::Remember,
            sentence: ActSentence::bare(Permit::Go, BareFile::None, Ingest::Ignore),
            tool: Some("remember".into()),
        };
        // Kinship rule (tape-evolution contract E3): a memory clip refs its
        // evidence — here, the latest observation in this session.
        let evidence = self
            .store
            .read_all()
            .map(|evs| {
                evs.iter()
                    .rev()
                    .find(|e| {
                        e.kind == Kind::Observation
                            && e.session.as_deref() == Some(self.session.as_str())
                    })
                    .map(|e| vec![e.id.clone()])
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let ev = self
            .store
            .append(AppendEvent {
                from: self.agent_id.clone(),
                from_kind: FromKind::Agent,
                kind: Kind::Utterance,
                session: self.session.clone(),
                content: text.to_string(),
                tags: vec!["memory".into()],
                refs: evidence,
                act: Some(ActOnEvent::intent(&env)),
                card: None,
            })
            .map_err(|e| e.to_string())?;
        Ok(format!("remembered as {}", ev.id))
    }
}

#[tokio::test]
#[ignore = "live DeepSeek probe; needs network and MODEL_API_KEY"]
async fn react_search_and_remember() {
    let Some(key) = api_key() else {
        eprintln!("MODEL_API_KEY not set and no .env at workspace root; skipping live probe");
        return;
    };
    let st = settings(key);
    let model_name = st.model.clone();
    let model = ChatCompletions::new(st);
    let (dir, _guard) = probe_dir();
    eprintln!("probe dir: {}", dir.display());
    let store = Arc::new(JsonlStore::open(dir.join("memory.jsonl")).unwrap());
    let agent_id = "react-probe".to_string();
    let session = Entropy::os().uuid_v4().to_string();
    let tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(WebSearch),
        Arc::new(Remember {
            store: store.clone(),
            agent_id: agent_id.clone(),
            session: session.clone(),
        }),
    ];
    let (_tx, rx) = tokio::sync::watch::channel(false);
    let ctx = ToolCtx { signal: Some(rx) };

    // The operator's question is an utterance on the tape.
    store
        .append(AppendEvent {
            from: "probe-operator".into(),
            from_kind: FromKind::Human,
            kind: Kind::Utterance,
            session: session.clone(),
            content: QUESTION.into(),
            tags: vec![],
            refs: vec![],
            act: None,
            card: None,
        })
        .unwrap();

    let state = AgentState {
        messages: vec![
            ChatMessage {
                role: "system".into(),
                content: Some(SYSTEM.into()),
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "user".into(),
                content: Some(QUESTION.into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ],
    };
    let actx = ActCtx {
        store: Some(&store),
        agent_id: &agent_id,
        session: &session,
        card: None,
    };
    let state = run_until_idle(state, &actx, &model, &tools, &ctx, 8)
        .await
        .unwrap();
    let answer = last_assistant_text(&state);
    eprintln!("\n=== final answer ===\n{answer}");

    let events = store.read_all().unwrap();
    eprintln!("\n=== tape ({} events) ===", events.len());
    for e in &events {
        eprintln!("{}", serde_json::to_string(e).unwrap());
    }

    // 1. Search and remember actually happened, as tool acts on the tape.
    let uses: Vec<&str> = events
        .iter()
        .filter_map(|e| e.act.as_ref().and_then(|a| a.tool.as_deref()))
        .collect();
    assert!(
        uses.contains(&"web_search"),
        "expected a web_search action on the tape, got {uses:?}"
    );
    assert!(
        uses.contains(&"remember"),
        "expected a remember action on the tape, got {uses:?}"
    );

    // 2. The deliberate memory clip exists (ActKind::remember, tagged) and,
    //    per kinship rule E3, refs the observation that evidenced it.
    let clip = events
        .iter()
        .find(|e| {
            e.tags.iter().any(|t| t == "memory")
                && e.act.as_ref().map(|a| a.kind.as_str()) == Some("remember")
        })
        .expect("no memory-tagged clip on the tape");
    assert!(!clip.refs.is_empty(), "memory clip must refs its evidence");
    for r in &clip.refs {
        let target = events
            .iter()
            .find(|e| &e.id == r)
            .expect("clip ref dangles");
        assert_eq!(
            target.kind,
            Kind::Observation,
            "clip refs must cite observations"
        );
    }

    // 3. Causal linkage: every observation cites an existing action.
    let ids: HashSet<&str> = events.iter().map(|e| e.id.as_str()).collect();
    for e in events.iter().filter(|e| e.kind == Kind::Observation) {
        assert!(
            !e.refs.is_empty() && e.refs.iter().all(|r| ids.contains(r.as_str())),
            "observation {} has dangling refs {:?}",
            e.id,
            e.refs
        );
    }

    // 4. Order and identity: seq is 1..=n without gaps; ids unique.
    assert_eq!(events.len(), ids.len(), "duplicate event ids");
    for (i, e) in events.iter().enumerate() {
        assert_eq!(e.seq, (i + 1) as u64, "seq gap at index {i}");
    }

    // 5. Reproducibility: the tape determines the run.
    //    a. Re-reading the tape is deterministic (two folds identical).
    let fold = |evs: &[Memevent]| {
        evs.iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        fold(&store.read_all().unwrap()),
        fold(&store.read_all().unwrap()),
        "re-reading the tape is not deterministic"
    );
    //    b. Tape completeness: every assistant text and every tool result in
    //       the final conversation is present on the tape.
    for m in &state.messages {
        match (m.role.as_str(), m.content.as_deref()) {
            ("assistant", Some(c)) => assert!(
                events
                    .iter()
                    .any(|e| e.kind == Kind::Utterance && e.content == c),
                "assistant text missing from tape"
            ),
            ("tool", Some(c)) => assert!(
                events
                    .iter()
                    .any(|e| e.kind == Kind::Observation && e.content == c),
                "tool result missing from tape"
            ),
            _ => {}
        }
    }
    //    c. The final answer is derivable from the tape alone: it is the last
    //       agent utterance that is not a memory clip.
    let last_agent_utterance = events
        .iter()
        .rev()
        .find(|e| {
            e.kind == Kind::Utterance
                && e.from_kind == FromKind::Agent
                && !e.tags.iter().any(|t| t == "memory")
        })
        .map(|e| e.content.clone());
    assert_eq!(last_agent_utterance.as_deref(), Some(answer.as_str()));

    // 6. Model calls are taped as invoke acts (vocabulary note 2026-09-17).
    //    Tool acts share the invoke domain; the model side is told apart by
    //    an absent tool field. One intent + one observation per step,
    //    observation refs exactly its intent, intent names the configured
    //    model, effect carries usage.
    let is_model_invoke = |e: &Memevent| {
        e.act.as_ref().map(|a| a.kind.as_str()) == Some("invoke")
            && e.act.as_ref().and_then(|a| a.tool.as_deref()).is_none()
    };
    let invoke_actions: Vec<&Memevent> = events
        .iter()
        .filter(|e| e.kind == Kind::Action && is_model_invoke(e))
        .collect();
    let invoke_obs: Vec<&Memevent> = events
        .iter()
        .filter(|e| e.kind == Kind::Observation && is_model_invoke(e))
        .collect();
    let steps = state
        .messages
        .iter()
        .filter(|m| m.role == "assistant")
        .count();
    assert_eq!(
        invoke_actions.len(),
        steps,
        "every step must tape its model call"
    );
    assert_eq!(invoke_obs.len(), steps);
    for a in &invoke_actions {
        assert!(
            a.content.contains(&model_name),
            "invoke intent must name the model: {}",
            a.content
        );
    }
    for o in &invoke_obs {
        assert_eq!(
            o.refs.len(),
            1,
            "invoke observation refs exactly its intent"
        );
        let intent = events
            .iter()
            .find(|e| e.id == o.refs[0])
            .expect("intent dangles");
        assert!(is_model_invoke(intent) && intent.kind == Kind::Action);
        let body: Value = serde_json::from_str(&o.content).expect("invoke effect is json");
        assert!(
            body["usage"]["prompt_tokens"].is_u64(),
            "invoke effect must carry usage: {body}"
        );
        assert!(body["content_hash"].is_string());
    }
}
