//! The discord parley case, scripted end to end: two published spools —
//! `chat_npc` (a rhai character body, mounted twice as two personas) and
//! `discord` (a host channel body standing in for the bridge). A user
//! message arrives as `discord.message`; both personas reply; the host
//! bridge turns each reply into a `discord.post`; the channel body sends
//! them to a scripted outbox. Every hop lands on tape.
//!
//! The host bridge is the stand-in for real discord I/O. One boundary is
//! deliberate: bot-authored posts are never pushed back as
//! `discord.message` — that way lies the self-loop.

// Integration tests are a separate crate: the workspace's
// `allow-unwrap-in-tests` only covers `#[cfg(test)]` code, so test-code
// unwraps are re-allowed here.
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};
use uuid::Uuid;

use an_agent_core::act::{Permit, ToolTag};
use an_agent_core::control::{
    AgentControl, ControlError, EventRoute, Registration, SpoolBeat, WorkspaceConfig,
};
use an_agent_core::principal::card::{AgentCard, ModelSpec, ToolGrant, Topology};
use an_agent_core::testkit::{TempDir, bash_registry};
use an_agent_core::workspace::WorkspaceRecord;
use an_agent_spool::beat::RhaiBeat;
use an_agent_spool::library;
use an_agent_spool::spool::Registry;

fn card() -> AgentCard {
    AgentCard {
        v: 1,
        id: Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap(),
        model: ModelSpec {
            base_url: "https://example.com".into(),
            model: "m".into(),
            extra_body: None,
        },
        prompt: "p".into(),
        tools: vec![ToolGrant {
            name: "bash".into(),
            tag: ToolTag::none_permit(Permit::Deny),
        }],
        topology: Topology::Leaf,
        kernel: "0.1.0".into(),
        supersedes: None,
    }
}

/// The scripted bridge: a post to "send" lands in the outbox.
struct Outbox(Mutex<Vec<String>>);

impl SpoolBeat for Outbox {
    fn receive(&self, event: &WorkspaceRecord) -> Result<String, String> {
        self.0
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(event.body.clone().unwrap_or_default());
        Ok("sent".into())
    }
}

#[test]
fn discord_parley_two_personas_one_body() {
    let tmp = TempDir::new("discord-parley");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();

    // The workspace declares what exists: inbound messages go to both
    // personas, outbound posts go to the channel.
    control
        .register(&Registration {
            events: vec![
                EventRoute {
                    name: "discord.message".into(),
                    spools: vec!["annie".into(), "bob".into()],
                },
                EventRoute {
                    name: "discord.post".into(),
                    spools: vec!["discord".into()],
                },
            ],
            config: vec!["discord".into()],
            env: vec![],
        })
        .unwrap();

    // Personas are mount parameters, not code: one body, two mounts.
    control
        .put_config(
            "discord",
            br#"
[[spool]]
name = "annie"
version = "1.0.0"
config = { persona = "Annie the cartographer" }

[[spool]]
name = "bob"
version = "1.0.0"
config = { persona = "Bob the lighthouse keeper" }
"#,
        )
        .unwrap();

    // Publish both bodies from the shelf.
    let shelf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spools");
    let registry = Registry::open(tmp.path().join("registry")).unwrap();
    let npc = library::publish(&registry, &shelf.join("chat_npc/chat_npc.yaml")).unwrap();
    let channel = library::publish(&registry, &shelf.join("discord/discord.yaml")).unwrap();

    // The host reads mount config back out of the workspace, keyed by the
    // mount name — the same bytes cite and seal see.
    let cite = control.workspace_cite().unwrap();
    let config_bytes = control.config_bytes(&cite.config_sha256.unwrap()).unwrap();
    let config: WorkspaceConfig =
        toml::from_str(std::str::from_utf8(&config_bytes).unwrap()).unwrap();
    let mount_config = |name: &str| -> Map<String, Value> {
        let table = &config
            .spool
            .iter()
            .find(|req| req.name == name)
            .unwrap_or_else(|| panic!("no mount config for {name}"))
            .config;
        serde_json::to_value(table)
            .unwrap()
            .as_object()
            .unwrap()
            .clone()
    };

    // A stranger matching nothing is refused at the bind point.
    assert!(matches!(
        control.mount_spool(
            "mallory",
            Arc::new(RhaiBeat::from_spool(&npc, Map::new()).unwrap()),
            &npc.declaration(),
        ),
        Err(ControlError::MountMismatch { .. })
    ));

    control
        .mount_spool(
            "annie",
            Arc::new(RhaiBeat::from_spool(&npc, mount_config("annie")).unwrap()),
            &npc.declaration(),
        )
        .unwrap();
    control
        .mount_spool(
            "bob",
            Arc::new(RhaiBeat::from_spool(&npc, mount_config("bob")).unwrap()),
            &npc.declaration(),
        )
        .unwrap();
    let outbox = Arc::new(Outbox(Mutex::new(Vec::new())));
    control
        .mount_spool(
            "discord",
            Arc::clone(&outbox) as Arc<dyn SpoolBeat>,
            &channel.declaration(),
        )
        .unwrap();

    let id = control.open_thread(&card()).unwrap();

    // Turn one: a user speaks; both personas answer; the host posts both
    // replies and the channel sends them.
    control
        .push_event("discord.message", "alice: land ho!")
        .unwrap();
    let replies = control.dispatch(id).unwrap();
    assert_eq!(
        replies
            .iter()
            .map(|r| r.reply().unwrap().reply.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Annie the cartographer @alice heard «land ho!»",
            "Bob the lighthouse keeper @alice heard «land ho!»",
        ]
    );
    for reply in &replies {
        control
            .push_event("discord.post", &reply.reply().unwrap().reply)
            .unwrap();
        let sent = control.dispatch(id).unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].reply().unwrap().reply, "sent");
    }
    assert_eq!(
        outbox
            .0
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone(),
        vec![
            "Annie the cartographer @alice heard «land ho!»".to_string(),
            "Bob the lighthouse keeper @alice heard «land ho!»".to_string(),
        ]
    );

    // Turn two: the parley continues; each beat hands the latest event.
    control
        .push_event("discord.message", "alice: thanks, both!")
        .unwrap();
    let replies = control.dispatch(id).unwrap();
    assert!(
        replies
            .iter()
            .all(|r| r.reply().unwrap().reply.contains("«thanks, both!»"))
    );

    // Everything is on tape: each persona beat and each send, keyed to
    // the workspace event that caused it.
    let tape = control.events(id).unwrap();
    for needle in ["annie", "bob", "discord", "land ho!", "thanks, both!"] {
        assert!(
            tape.iter().any(|event| {
                event.tags.iter().any(|tag| tag == "spool") && event.content.contains(needle)
            }),
            "{needle}"
        );
    }
    let log: Vec<_> = control
        .workspace_log()
        .unwrap()
        .into_iter()
        .filter(|record| record.kind == "event")
        .map(|record| (record.name.unwrap(), record.body.unwrap()))
        .collect();
    assert_eq!(log[0], ("discord.message".into(), "alice: land ho!".into()));
    assert_eq!(
        log.iter()
            .filter(|(name, _)| name == "discord.post")
            .count(),
        2
    );
}
