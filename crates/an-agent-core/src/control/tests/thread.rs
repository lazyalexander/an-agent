//! Open, advance, cancel, and send.

use super::*;
use super::{card, gate_ctor, shared_gate};

#[test]
fn open_thread_births_one_tape() {
    let tmp = TempDir::new("control-open");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    let id = control
        .open_thread(&card(
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "bash",
            Permit::Deny,
        ))
        .unwrap();
    let events = control.events(id).unwrap();
    assert_eq!(events.len(), 1);
    assert!(events[0].tags.iter().any(|tag| tag == "thread"));
    assert!(events[0].content.contains(&id.to_string()));
    assert!(matches!(
        control.open_thread(&card(
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "bash",
            Permit::Deny,
        )),
        Err(ControlError::Tree(TreeError::Duplicate(_)))
    ));
}

#[test]
fn advance_then_finish_refuses_another_beat() {
    let tmp = TempDir::new("control-finish");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    let id = control
        .open_thread(&card(
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "bash",
            Permit::Deny,
        ))
        .unwrap();
    control.advance(id, "look here").unwrap();
    control.finish(id, "done").unwrap();
    assert!(matches!(
        control.advance(id, "again"),
        Err(ControlError::Finished(_))
    ));
    let events = control.events(id).unwrap();
    assert!(events.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "advance") && event.content.contains("look here")
    }));
    assert!(
        events
            .iter()
            .any(|event| event.tags.iter().any(|tag| tag == "finish"))
    );
}

#[tokio::test]
async fn cancel_consumes_one_beat_and_does_not_run_the_tool() {
    let tmp = TempDir::new("control-cancel");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    let id = control
        .open_thread(&card(
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "bash",
            Permit::Go,
        ))
        .unwrap();
    control.cancel(id).unwrap();
    let call = ToolCall {
        id: "c1".into(),
        name: "bash".into(),
        arguments: r#"{"command":"true"}"#.into(),
    };
    assert!(matches!(
        control.run(id, &call).await,
        Err(ControlError::Cancelled(_))
    ));
    control.advance(id, "next").unwrap();
    let events = control.events(id).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.tags.iter().any(|tag| tag == "cancel"))
    );
    assert!(events.iter().any(|event| event.content.contains("next")));
    assert!(events.iter().all(|event| event.content != "ran"));
}

#[tokio::test]
async fn a_running_beat_holds_the_only_slot() {
    let tmp = TempDir::new("control-slot");
    let gate = shared_gate();
    let control = AgentControl::open(tmp.path(), &[("gate", gate_ctor)]).unwrap();
    let running = control
        .open_thread(&card(
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "gate",
            Permit::Go,
        ))
        .unwrap();
    let other = control
        .open_thread(&card(
            "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            "gate",
            Permit::Go,
        ))
        .unwrap();
    let worker = control.clone();
    let task = tokio::spawn(async move {
        worker
            .run(
                running,
                &ToolCall {
                    id: "c1".into(),
                    name: "gate".into(),
                    arguments: "{}".into(),
                },
            )
            .await
    });
    gate.started.notified().await;
    assert!(matches!(
        control.advance(other, "wait"),
        Err(ControlError::Pool(PoolError::AtCapacity(1)))
    ));
    gate.release.notify_one();
    let result = task.await.unwrap().unwrap();
    assert_eq!(result.message.content, "ran");
    control.advance(other, "wait").unwrap();
}

#[test]
fn send_writes_both_tapes() {
    let tmp = TempDir::new("control-send");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    let from = control
        .open_thread(&card(
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "bash",
            Permit::Deny,
        ))
        .unwrap();
    let to = control
        .open_thread(&card(
            "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            "bash",
            Permit::Deny,
        ))
        .unwrap();
    control.send(from, to, "ping").unwrap();
    assert!(matches!(
        control.send(from, from, "no"),
        Err(ControlError::SameThread(_))
    ));
    assert!(control.events(from).unwrap().iter().any(|event| {
        event.tags.iter().any(|tag| tag == "send") && event.content.contains("ping")
    }));
    assert!(control.events(to).unwrap().iter().any(|event| {
        event.tags.iter().any(|tag| tag == "send")
            && event.content.contains("ping")
            && event.content.contains(&from.to_string())
    }));
}
