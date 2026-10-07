//! Hook chains on the delivery path: gate gear, fail-closed, denials taped.

use super::*;
use super::{Clip, declares, listed, stroke_card};

/// A scripted runner: records calls, denies when the subject carries the
/// poison string, optionally fails outright (which must read as deny).
struct ScriptedHooks {
    calls: Mutex<Vec<String>>,
    poison: Option<&'static str>,
    crash: bool,
}

impl HookRunner for ScriptedHooks {
    fn run_before(
        &self,
        handler: &HookHandler,
        event: &WorkspaceRecord,
    ) -> Result<HookVerdict, String> {
        let label = match handler {
            HookHandler::Spool { name, .. } => format!("before:spool:{name}"),
            HookHandler::Mcp { server, tool, .. } => format!("before:mcp:{server}:{tool}"),
        };
        self.calls
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(label);
        if self.crash {
            return Err("hook crashed".into());
        }
        if let Some(poison) = self.poison
            && event.body.as_deref().unwrap_or("").contains(poison)
        {
            return Ok(HookVerdict::Deny {
                reason: format!("poisoned: {poison}"),
            });
        }
        Ok(HookVerdict::Allow)
    }

    fn run_after(&self, handler: &HookHandler, reply: &SpoolReply) -> Result<HookVerdict, String> {
        let label = match handler {
            HookHandler::Spool { name, .. } => format!("after:spool:{name}"),
            HookHandler::Mcp { server, tool, .. } => format!("after:mcp:{server}:{tool}"),
        };
        self.calls
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(label);
        if self.crash {
            return Err("hook crashed".into());
        }
        if let Some(poison) = self.poison
            && reply.reply.contains(poison)
        {
            return Ok(HookVerdict::Deny {
                reason: format!("poisoned: {poison}"),
            });
        }
        Ok(HookVerdict::Allow)
    }
}

const HOOKED: &[u8] = br#"
[hooks]
before = [{ type = "spool", name = "screen_in", version = "1.0.0" }]
after = [{ type = "mcp", server = "guard", tool = "screen_reply" }]
"#;

fn hooked_control(tmp: &TempDir, runner: Option<Arc<ScriptedHooks>>) -> AgentControl {
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(&[("stroke", &["clip"])], &["canvas"], &[]))
        .unwrap();
    control.put_config("canvas", HOOKED).unwrap();
    if let Some(runner) = runner {
        control.set_hook_runner(runner);
    }
    control
}

fn scripted(poison: Option<&'static str>, crash: bool) -> Arc<ScriptedHooks> {
    Arc::new(ScriptedHooks {
        calls: Mutex::new(Vec::new()),
        poison,
        crash,
    })
}

impl ScriptedHooks {
    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }
}

#[test]
fn a_before_deny_stops_delivery_and_is_taped() {
    let tmp = TempDir::new("hook-before-deny");
    let runner = scripted(Some("600,40"), false);
    let control = hooked_control(&tmp, Some(runner));
    let id = control.open_thread(&stroke_card()).unwrap();
    control
        .mount_spool("clip", Arc::new(Clip), &declares(&["stroke"], &[]))
        .unwrap();
    control.push_event("stroke", "stroke 600,40").unwrap();

    assert!(matches!(
        control.dispatch(id),
        Err(ControlError::HookDenied { chain, reason }) if chain == "before" && reason.contains("poisoned")
    ));
    // The event stays; the spool never ran.
    let tape = control.events(id).unwrap();
    assert!(
        !tape
            .iter()
            .any(|event| event.tags.iter().any(|tag| tag == "spool"))
    );
    // The denial is taped.
    assert!(tape.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "hook") && event.content.contains("poisoned")
    }));
}

#[test]
fn allows_are_not_taped_and_the_chain_runs_in_order() {
    let tmp = TempDir::new("hook-allow");
    let runner = scripted(None, false);
    let control = hooked_control(&tmp, Some(Arc::clone(&runner)));
    let id = control.open_thread(&stroke_card()).unwrap();
    control
        .mount_spool("clip", Arc::new(Clip), &declares(&["stroke"], &[]))
        .unwrap();
    control
        .push_event("stroke", "stroke 20,20 600,40 480,700")
        .unwrap();
    let replies = control.dispatch(id).unwrap();
    assert_eq!(replies.len(), 1);

    // Both chains ran, in declared order: before-spool then after-mcp.
    assert_eq!(
        runner.calls(),
        vec![
            "before:spool:screen_in".to_string(),
            "after:mcp:guard:screen_reply".to_string(),
        ]
    );
    // Allows tape nothing: no hook-tagged events exist.
    let tape = control.events(id).unwrap();
    assert!(
        !tape
            .iter()
            .any(|event| event.tags.iter().any(|tag| tag == "hook"))
    );
}

#[test]
fn an_after_deny_keeps_the_reply_from_the_host() {
    let tmp = TempDir::new("hook-after-deny");
    // The clip reply contains "511,40"; poison the after chain on it.
    let runner = scripted(Some("511,40"), false);
    let control = hooked_control(&tmp, Some(runner));
    let id = control.open_thread(&stroke_card()).unwrap();
    control
        .mount_spool("clip", Arc::new(Clip), &declares(&["stroke"], &[]))
        .unwrap();
    control
        .push_event("stroke", "stroke 20,20 600,40 480,700")
        .unwrap();

    // dispatch swallows the denied reply: the spool ran, the host gets nothing.
    let replies = control.dispatch(id).unwrap();
    assert!(replies.is_empty());
    let tape = control.events(id).unwrap();
    // The spool note and the hook denial are both on tape.
    assert!(tape.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "spool") && event.content.contains("511,40")
    }));
    assert!(tape.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "hook") && event.content.contains("after")
    }));

    // offer reports the denial as an error.
    control
        .push_event("stroke", "stroke 20,20 600,40 480,700")
        .unwrap();
    assert!(matches!(
        control.offer(id, "clip"),
        Err(ControlError::HookDenied { chain, .. }) if chain == "after"
    ));
}

#[test]
fn declared_hooks_without_a_runner_fail_closed() {
    let tmp = TempDir::new("hook-no-runner");
    let control = hooked_control(&tmp, None);
    let id = control.open_thread(&stroke_card()).unwrap();
    control
        .mount_spool("clip", Arc::new(Clip), &declares(&["stroke"], &[]))
        .unwrap();
    control
        .push_event("stroke", "stroke 20,20 600,40 480,700")
        .unwrap();
    assert!(matches!(
        control.dispatch(id),
        Err(ControlError::HookRunnerMissing)
    ));
}

#[test]
fn a_crashed_hook_reads_as_a_deny() {
    let tmp = TempDir::new("hook-crash");
    let runner = scripted(None, true);
    let control = hooked_control(&tmp, Some(runner));
    let id = control.open_thread(&stroke_card()).unwrap();
    control
        .mount_spool("clip", Arc::new(Clip), &declares(&["stroke"], &[]))
        .unwrap();
    control
        .push_event("stroke", "stroke 20,20 600,40 480,700")
        .unwrap();
    assert!(matches!(
        control.dispatch(id),
        Err(ControlError::HookDenied { chain, reason }) if chain == "before" && reason.contains("hook crashed")
    ));
}

#[test]
fn the_chain_stops_at_the_first_deny() {
    let tmp = TempDir::new("hook-order");
    let runner = scripted(Some("600,40"), false);
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(&[("stroke", &["clip"])], &["canvas"], &[]))
        .unwrap();
    control
        .put_config(
            "canvas",
            br#"
[hooks]
before = [
  { type = "spool", name = "first", version = "1.0.0" },
  { type = "spool", name = "second", version = "1.0.0" },
]
"#,
        )
        .unwrap();
    control.set_hook_runner(runner.clone());
    let id = control.open_thread(&stroke_card()).unwrap();
    control
        .mount_spool("clip", Arc::new(Clip), &declares(&["stroke"], &[]))
        .unwrap();
    control.push_event("stroke", "stroke 600,40").unwrap();
    assert!(matches!(
        control.dispatch(id),
        Err(ControlError::HookDenied { .. })
    ));
    // `second` never ran: the calls list holds only the first handler.
    assert_eq!(runner.calls(), vec!["before:spool:first".to_string()]);
}
