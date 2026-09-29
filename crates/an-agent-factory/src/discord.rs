//! Discord REST bridge: poll and send on one channel, behind a `Transport`
//! seam so tests script responses offline. Not in `tool_registry()` — bash
//! stays the only registered constructor.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use an_agent_core::act::{Tool, ToolCtx, ToolTag};

const DEFAULT_TOKEN_ENV: &str = "DISCORD_BOT_TOKEN";
const DEFAULT_BASE_URL: &str = "https://discord.com/api/v10";
// A declared-egress face must not become an unbounded memory face: bodies
// past the cap are refused instead of buffered.
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const EXCERPT_CHARS: usize = 512;
// Discord's message cap is 2000 chars.
const MAX_MESSAGE_CHARS: usize = 2000;

/// The HTTP seam: blocking, token passed per call so a fake transport in
/// tests never needs one.
pub trait Transport: Send + Sync {
    fn get(&self, url: &str, bot_token: &str) -> Result<String, String>;
    fn post(&self, url: &str, bot_token: &str, body: Value) -> Result<String, String>;
}

/// reqwest-backed transport for real Discord traffic.
pub struct ReqwestTransport {
    client: reqwest::blocking::Client,
}

impl ReqwestTransport {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("an-agent-discord/0.1")
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { client })
    }
}

fn read_body(resp: reqwest::blocking::Response) -> Result<String, String> {
    use std::io::Read;
    let mut limited = resp.take(MAX_BODY_BYTES as u64 + 1);
    let mut buf = Vec::new();
    limited.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() > MAX_BODY_BYTES {
        return Err(format!("response exceeds {MAX_BODY_BYTES} bytes"));
    }
    String::from_utf8(buf).map_err(|e| e.to_string())
}

fn check_status(status: reqwest::StatusCode, body: String) -> Result<String, String> {
    if status.is_success() {
        return Ok(body);
    }
    let excerpt: String = body.chars().take(EXCERPT_CHARS).collect();
    Err(format!("discord http {status}: {excerpt}"))
}

fn check_token(bot_token: &str) -> Result<(), String> {
    if bot_token.is_empty() {
        return Err("bot token env var is not set".into());
    }
    Ok(())
}

impl Transport for ReqwestTransport {
    fn get(&self, url: &str, bot_token: &str) -> Result<String, String> {
        check_token(bot_token)?;
        let resp = self
            .client
            .get(url)
            .header("Authorization", format!("Bot {bot_token}"))
            .send()
            .map_err(|e| e.to_string())?;
        check_status(resp.status(), read_body(resp)?)
    }

    fn post(&self, url: &str, bot_token: &str, body: Value) -> Result<String, String> {
        check_token(bot_token)?;
        let resp = self
            .client
            .post(url)
            .header("Authorization", format!("Bot {bot_token}"))
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;
        check_status(resp.status(), read_body(resp)?)
    }
}

#[derive(Debug)]
pub struct DiscordConfig {
    pub channel_id: String,
    pub token_env: String,
    pub base_url: String,
}

impl DiscordConfig {
    /// The token VALUE never appears here: the spool body is
    /// content-addressed and immutable, so secrets must stay out of it.
    /// The config carries only the env var NAME; the token itself is read
    /// from the process environment lazily at each call, so tests with a
    /// fake transport never need it.
    pub fn from_config(map: &serde_json::Map<String, Value>) -> Result<Self, String> {
        let mut channel_id: Option<String> = None;
        let mut token_env = DEFAULT_TOKEN_ENV.to_string();
        let mut base_url = DEFAULT_BASE_URL.to_string();
        for (key, value) in map {
            match key.as_str() {
                "channel_id" => {
                    channel_id = Some(match value.as_str() {
                        Some(s) if !s.is_empty() => s.to_string(),
                        _ => return Err("channel_id must be a non-empty string".into()),
                    });
                }
                "token_env" => {
                    token_env = match value.as_str() {
                        Some(s) if !s.is_empty() => s.to_string(),
                        _ => return Err("token_env must be a non-empty string".into()),
                    };
                }
                "base_url" => {
                    base_url = match value.as_str() {
                        Some(s) if !s.is_empty() => s.trim_end_matches('/').to_string(),
                        _ => return Err("base_url must be a non-empty string".into()),
                    };
                }
                other => return Err(format!("unknown config key: {other}")),
            }
        }
        let channel_id = channel_id.ok_or_else(|| "channel_id is required".to_string())?;
        Ok(Self {
            channel_id,
            token_env,
            base_url,
        })
    }
}

/// The wire fields this bridge reads; Discord's other message fields are
/// ignored. Deserialization is all-or-nothing, so a malformed batch fails
/// whole and the poll cursor is never advanced on partial data.
#[derive(Deserialize)]
struct WireMessage {
    id: String,
    #[serde(default)]
    content: String,
    author: WireAuthor,
}

#[derive(Deserialize)]
struct WireAuthor {
    id: String,
    username: Option<String>,
    #[serde(default)]
    bot: bool,
}

#[derive(Deserialize)]
struct CreatedMessage {
    id: String,
}

pub struct Discord {
    cfg: DiscordConfig,
    transport: Arc<dyn Transport>,
    after: Mutex<Option<String>>,
}

impl Discord {
    pub fn new(cfg: DiscordConfig, transport: Arc<dyn Transport>) -> Self {
        Self {
            cfg,
            transport,
            after: Mutex::new(None),
        }
    }

    fn cursor(&self) -> Result<MutexGuard<'_, Option<String>>, String> {
        self.after.lock().map_err(|_| "cursor lock poisoned".into())
    }

    fn token(&self) -> String {
        std::env::var(&self.cfg.token_env).unwrap_or_default()
    }

    fn poll_url(&self) -> Result<String, String> {
        let mut url = format!(
            "{}/channels/{}/messages?limit=50",
            self.cfg.base_url, self.cfg.channel_id
        );
        if let Some(after) = self.cursor()?.as_ref() {
            url.push_str("&after=");
            url.push_str(after);
        }
        Ok(url)
    }

    fn send_url(&self) -> String {
        format!(
            "{}/channels/{}/messages",
            self.cfg.base_url, self.cfg.channel_id
        )
    }

    async fn call_get(&self, url: String, ctx: &ToolCtx) -> Result<String, String> {
        let transport = self.transport.clone();
        let token = self.token();
        let mut task = tokio::task::spawn_blocking(move || transport.get(&url, &token));
        // A cancelled call stops waiting here; the blocking request itself
        // cannot be killed and finishes under its own 30s timeout.
        tokio::select! {
            res = &mut task => res.map_err(|e| e.to_string())?,
            _ = wait_cancel(ctx.signal.clone()) => Ok("cancelled".into()),
        }
    }

    async fn call_post(&self, url: String, body: Value, ctx: &ToolCtx) -> Result<String, String> {
        let transport = self.transport.clone();
        let token = self.token();
        let mut task = tokio::task::spawn_blocking(move || transport.post(&url, &token, body));
        tokio::select! {
            res = &mut task => res.map_err(|e| e.to_string())?,
            _ = wait_cancel(ctx.signal.clone()) => Ok("cancelled".into()),
        }
    }

    async fn poll(&self, ctx: &ToolCtx) -> Result<String, String> {
        let url = self.poll_url()?;
        let body = self.call_get(url, ctx).await?;
        if body.len() > MAX_BODY_BYTES {
            return Err(format!("response exceeds {MAX_BODY_BYTES} bytes"));
        }
        let batch: Vec<WireMessage> =
            serde_json::from_str(&body).map_err(|e| format!("poll: invalid response: {e}"))?;
        let mut out = Vec::new();
        let mut max_id: Option<u64> = None;
        for msg in &batch {
            // Snowflakes are monotonic but not lexicographically ordered;
            // compare as u64, never as strings.
            let id: u64 = msg
                .id
                .parse()
                .map_err(|_| format!("poll: message id is not a snowflake: {}", msg.id))?;
            max_id = Some(max_id.map_or(id, |m: u64| m.max(id)));
            // Drop every bot-authored message, including our own sends: this
            // is what makes agent loops and character-to-character recursion
            // impossible by construction.
            if msg.author.bot {
                continue;
            }
            let author = msg
                .author
                .username
                .clone()
                .unwrap_or_else(|| msg.author.id.clone());
            out.push(serde_json::json!({
                "id": msg.id,
                "author": author,
                "content": msg.content,
            }));
        }
        // The cursor advances only after the whole batch parsed successfully;
        // bot messages still count, or our own sends would be re-fetched
        // forever.
        if let Some(max_id) = max_id {
            *self.cursor()? = Some(max_id.to_string());
        }
        Ok(Value::Array(out).to_string())
    }

    async fn send(&self, content: String, ctx: &ToolCtx) -> Result<String, String> {
        let url = self.send_url();
        let body = self
            .call_post(url, serde_json::json!({ "content": content }), ctx)
            .await?;
        let created: CreatedMessage =
            serde_json::from_str(&body).map_err(|e| format!("send: invalid response: {e}"))?;
        Ok(created.id)
    }
}

/// Resolves once the signal fires; pends forever without one.
async fn wait_cancel(signal: Option<tokio::sync::watch::Receiver<bool>>) {
    let Some(mut rx) = signal else {
        return std::future::pending::<()>().await;
    };
    loop {
        if *rx.borrow() {
            return;
        }
        if rx.changed().await.is_err() {
            return std::future::pending::<()>().await;
        }
    }
}

#[async_trait]
impl Tool for Discord {
    fn name(&self) -> &str {
        "discord"
    }

    fn description(&self) -> &str {
        "Poll and send messages in a Discord channel. Arguments: { op: \"poll\" | \"send\", content?: string } (content required for send)."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "op": { "type": "string", "enum": ["poll", "send"] },
                "content": { "type": "string", "description": "message text, required for op=send" }
            },
            "required": ["op"]
        })
    }

    fn tag_seed(&self) -> Option<ToolTag> {
        // ToolTag's three faces (file, permit, memory) have no net/flow face;
        // the egress face is carried by the spool declaration and its closure
        // fold, not by the admission tag. That asymmetry is the current state.
        Some(ToolTag::none())
    }

    async fn execute(&self, args: Value, ctx: &ToolCtx) -> Result<String, String> {
        match args.get("op").and_then(Value::as_str) {
            Some("poll") => self.poll(ctx).await,
            Some("send") => {
                let content = args
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "content is required".to_string())?;
                // Truncate on a char boundary, never mid-codepoint.
                let content: String = content.chars().take(MAX_MESSAGE_CHARS).collect();
                self.send(content, ctx).await
            }
            other => Err(format!("unknown op: {}", other.unwrap_or("<missing>"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::VecDeque;

    struct FakeTransport {
        gets: Mutex<VecDeque<Result<String, String>>>,
        posts: Mutex<VecDeque<Result<String, String>>>,
        seen_gets: Mutex<Vec<String>>,
        seen_posts: Mutex<Vec<(String, Value)>>,
    }

    impl FakeTransport {
        fn new() -> Self {
            Self {
                gets: Mutex::new(VecDeque::new()),
                posts: Mutex::new(VecDeque::new()),
                seen_gets: Mutex::new(Vec::new()),
                seen_posts: Mutex::new(Vec::new()),
            }
        }

        fn queue_get(&self, body: &str) {
            self.gets.lock().unwrap().push_back(Ok(body.to_string()));
        }

        fn queue_post(&self, body: &str) {
            self.posts.lock().unwrap().push_back(Ok(body.to_string()));
        }

        fn get_urls(&self) -> Vec<String> {
            self.seen_gets.lock().unwrap().clone()
        }

        fn post_bodies(&self) -> Vec<Value> {
            self.seen_posts
                .lock()
                .unwrap()
                .iter()
                .map(|(_, body)| body.clone())
                .collect()
        }
    }

    impl Transport for FakeTransport {
        fn get(&self, url: &str, _bot_token: &str) -> Result<String, String> {
            self.seen_gets.lock().unwrap().push(url.to_string());
            self.gets
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "no scripted get response".to_string())?
        }

        fn post(&self, url: &str, _bot_token: &str, body: Value) -> Result<String, String> {
            self.seen_posts
                .lock()
                .unwrap()
                .push((url.to_string(), body));
            self.posts
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "no scripted post response".to_string())?
        }
    }

    fn cfg() -> DiscordConfig {
        DiscordConfig {
            channel_id: "123".into(),
            token_env: DEFAULT_TOKEN_ENV.into(),
            base_url: "https://discord.test/api".into(),
        }
    }

    fn ctx() -> ToolCtx {
        ToolCtx { signal: None }
    }

    fn tool(fake: &Arc<FakeTransport>) -> Discord {
        Discord::new(cfg(), fake.clone())
    }

    #[tokio::test]
    async fn poll_maps_messages_and_advances_cursor() {
        let fake = Arc::new(FakeTransport::new());
        fake.queue_get(
            r#"[
                {"id":"100","author":{"id":"u1","username":"amy"},"content":"hi"},
                {"id":"105","author":{"id":"u2"},"content":"yo"}
            ]"#,
        );
        let discord = tool(&fake);
        let out = discord
            .execute(json!({"op": "poll"}), &ctx())
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            parsed,
            json!([
                {"id": "100", "author": "amy", "content": "hi"},
                {"id": "105", "author": "u2", "content": "yo"}
            ])
        );
        assert_eq!(fake.get_urls().len(), 1);
        assert!(!fake.get_urls()[0].contains("after="));
    }

    #[tokio::test]
    async fn second_poll_passes_advanced_cursor() {
        let fake = Arc::new(FakeTransport::new());
        fake.queue_get(r#"[{"id":"200","author":{"id":"u1","username":"amy"},"content":"hi"}]"#);
        fake.queue_get("[]");
        let discord = tool(&fake);
        discord
            .execute(json!({"op": "poll"}), &ctx())
            .await
            .unwrap();
        discord
            .execute(json!({"op": "poll"}), &ctx())
            .await
            .unwrap();
        assert!(fake.get_urls()[1].contains("after=200"));
    }

    #[tokio::test]
    async fn bot_messages_are_dropped_but_advance_cursor() {
        let fake = Arc::new(FakeTransport::new());
        fake.queue_get(
            r#"[
                {"id":"300","author":{"id":"b1","username":"botty","bot":true},"content":"beep"},
                {"id":"301","author":{"id":"u1","username":"amy"},"content":"hi"}
            ]"#,
        );
        fake.queue_get("[]");
        let discord = tool(&fake);
        let out = discord
            .execute(json!({"op": "poll"}), &ctx())
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            parsed,
            json!([{"id": "301", "author": "amy", "content": "hi"}])
        );
        discord
            .execute(json!({"op": "poll"}), &ctx())
            .await
            .unwrap();
        assert!(fake.get_urls()[1].contains("after=301"));
    }

    #[tokio::test]
    async fn malformed_poll_leaves_cursor_unchanged() {
        let fake = Arc::new(FakeTransport::new());
        fake.queue_get("not json");
        fake.queue_get(r#"[{"id":"10","author":{"id":"u1","username":"amy"},"content":"hi"}]"#);
        fake.queue_get("[]");
        let discord = tool(&fake);
        assert!(
            discord
                .execute(json!({"op": "poll"}), &ctx())
                .await
                .is_err()
        );
        discord
            .execute(json!({"op": "poll"}), &ctx())
            .await
            .unwrap();
        discord
            .execute(json!({"op": "poll"}), &ctx())
            .await
            .unwrap();
        let urls = fake.get_urls();
        assert!(!urls[1].contains("after="));
        assert!(urls[2].contains("after=10"));
    }

    #[tokio::test]
    async fn send_truncates_at_2000_chars_and_returns_id() {
        let fake = Arc::new(FakeTransport::new());
        fake.queue_post(r#"{"id":"999"}"#);
        let discord = tool(&fake);
        let content = "😀".repeat(2001);
        let out = discord
            .execute(json!({"op": "send", "content": content}), &ctx())
            .await
            .unwrap();
        assert_eq!(out, "999");
        let bodies = fake.post_bodies();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0]["content"].as_str().unwrap().chars().count(), 2000);
    }

    #[test]
    fn from_config_defaults() {
        let mut map = serde_json::Map::new();
        map.insert("channel_id".into(), json!("42"));
        let cfg = DiscordConfig::from_config(&map).unwrap();
        assert_eq!(cfg.channel_id, "42");
        assert_eq!(cfg.token_env, DEFAULT_TOKEN_ENV);
        assert_eq!(cfg.base_url, DEFAULT_BASE_URL);
    }

    #[test]
    fn from_config_rejects_unknown_keys() {
        let mut map = serde_json::Map::new();
        map.insert("channel_id".into(), json!("42"));
        map.insert("surprise".into(), json!(true));
        let err = DiscordConfig::from_config(&map).unwrap_err();
        assert!(err.contains("unknown config key"), "{err}");
    }

    #[test]
    fn from_config_requires_channel_id() {
        let err = DiscordConfig::from_config(&serde_json::Map::new()).unwrap_err();
        assert!(err.contains("channel_id"), "{err}");
        let mut map = serde_json::Map::new();
        map.insert("channel_id".into(), json!(""));
        assert!(DiscordConfig::from_config(&map).is_err());
        map.insert("channel_id".into(), json!(7));
        assert!(DiscordConfig::from_config(&map).is_err());
    }

    #[tokio::test]
    async fn unknown_op_is_an_error() {
        let fake = Arc::new(FakeTransport::new());
        let discord = tool(&fake);
        assert!(
            discord
                .execute(json!({"op": "delete"}), &ctx())
                .await
                .is_err()
        );
        assert!(discord.execute(json!({}), &ctx()).await.is_err());
    }
}
