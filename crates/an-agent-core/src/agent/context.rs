//! The thread's context composition: what the model sees. This is the
//! system-level envelope in the codex sense — who the thread is (card
//! prompt), what runs it (host facts, platform), where it lives
//! (workspace env), and the rules the workspace declares (guidance) —
//! assembled here, on the thread side. Control admits the call and tapes
//! it; it does not compose.

/// One message handed to the model client. Roles follow the chat
/// convention (system / user / assistant); assembly is the thread's job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelMessage {
    pub role: String,
    pub content: String,
}

/// System-level facts around a call. `host` is the embedding software's
/// own label; `os` / `arch` come from the platform, the same way codex
/// states its environment context. Facts are stated, not enforced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemFacts {
    pub host: String,
    pub os: String,
    pub arch: String,
}

impl SystemFacts {
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        }
    }
}

/// One tape clip selected for the envelope.
#[derive(Debug, Clone, Copy)]
pub struct Clip<'a> {
    pub tag: &'a str,
    pub content: &'a str,
}

/// The parts one model call is composed from. Every ingredient has an
/// owner outside the thread — card, workspace, host — the thread only
/// assembles.
pub struct Envelope<'a> {
    /// Card prompt: the thread's kernel identity.
    pub prompt: &'a str,
    pub facts: &'a SystemFacts,
    /// Workspace env markdown. Empty means the workspace declares none.
    pub env: &'a str,
    /// Workspace `[guard]` rules: statements the model is told as
    /// *enforced* — each binds a pinned gate on the after chain, so the
    /// model may rely on being stopped. Never list a rule here that has
    /// no gate behind it; that is what `guidance` is for.
    pub enforced: &'a [String],
    /// Workspace `[guidance]` rules: advisory statements the model is
    /// told. Nothing mechanical enforces them, and this section never
    /// claims otherwise.
    pub guidance: &'a [String],
    pub clips: &'a [Clip<'a>],
}

/// Compose the two messages of one model call: the system envelope and
/// the clip bundle. Empty sections are omitted, never stated as empty —
/// an absent declaration reads as absence, not as a blank rule.
pub fn compose(parts: &Envelope) -> Vec<ModelMessage> {
    let mut system = parts.prompt.to_string();
    system.push_str(&format!(
        "\n\n# Host\n{} on {}/{}",
        parts.facts.host, parts.facts.os, parts.facts.arch
    ));
    if !parts.env.trim().is_empty() {
        system.push_str(&format!("\n\n# Environment\n{}", parts.env.trim_end()));
    }
    if !parts.enforced.is_empty() {
        system.push_str("\n\n# Guard (enforced)");
        for rule in parts.enforced {
            system.push_str(&format!("\n- {rule}"));
        }
    }
    if !parts.guidance.is_empty() {
        system.push_str("\n\n# Guidance (advisory)");
        for rule in parts.guidance {
            system.push_str(&format!("\n- {rule}"));
        }
    }
    let mut user = String::new();
    for clip in parts.clips {
        user.push_str(&format!("[{}] {}\n", clip.tag, clip.content));
    }
    vec![
        ModelMessage {
            role: "system".into(),
            content: system,
        },
        ModelMessage {
            role: "user".into(),
            content: user,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> SystemFacts {
        SystemFacts::new("test-host")
    }

    #[test]
    fn the_envelope_states_every_declared_ingredient() {
        let clips = [Clip {
            tag: "scene",
            content: "a traveler enters",
        }];
        let enforced = vec!["replies under 280 characters".to_string()];
        let rules = vec!["stay in character".to_string()];
        let facts = facts();
        let messages = compose(&Envelope {
            prompt: "you keep the tavern",
            facts: &facts,
            env: "# The Tavern\nA quiet place.\n",
            enforced: &enforced,
            guidance: &rules,
            clips: &clips,
        });
        assert_eq!(messages.len(), 2);
        let system = &messages[0];
        assert_eq!(system.role, "system");
        for needle in [
            "you keep the tavern",
            "test-host on ",
            "# Environment\n# The Tavern\nA quiet place.",
            "# Guard (enforced)\n- replies under 280 characters",
            "# Guidance (advisory)\n- stay in character",
        ] {
            assert!(system.content.contains(needle), "{needle}");
        }
        // Enforced is stated before advisory: hard rules first.
        assert!(
            system.content.find("# Guard").unwrap() < system.content.find("# Guidance").unwrap()
        );
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[1].content, "[scene] a traveler enters\n");
    }

    #[test]
    fn empty_sections_are_omitted_not_stated() {
        let facts = facts();
        let messages = compose(&Envelope {
            prompt: "p",
            facts: &facts,
            env: "  \n",
            enforced: &[],
            guidance: &[],
            clips: &[],
        });
        assert!(!messages[0].content.contains("# Environment"));
        assert!(!messages[0].content.contains("# Guard"));
        assert!(!messages[0].content.contains("# Guidance"));
        assert!(messages[0].content.contains("# Host"));
        assert_eq!(messages[1].content, "");
    }
}
