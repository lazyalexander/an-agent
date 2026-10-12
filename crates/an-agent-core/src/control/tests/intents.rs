//! Reply intents: the string reply carries structured intent JSON by
//! convention; `produces` is the raise whitelist enforced at delivery.

use super::*;

/// A body with a canned reply string.
struct Scripted(&'static str);

impl SpoolBeat for Scripted {
    fn receive(&self, _event: &WorkspaceRecord) -> Result<String, String> {
        Ok(self.0.into())
    }
}

/// Registration with the `stroke` route to `voice` and a routeless
/// `stroke.done` event for `produces` to name.
fn intent_control(tmp: &TempDir) -> AgentControl {
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(
            &[("stroke", &["voice"]), ("stroke.done", &[])],
            &[],
            &[],
        ))
        .unwrap();
    control
}

fn mount_voice(control: &AgentControl, out: &'static str, produces: &[&str]) {
    control
        .mount_spool(
            "voice",
            Arc::new(Scripted(out)),
            &declares(&["stroke"], produces),
        )
        .unwrap();
}

fn push_stroke(control: &AgentControl) {
    control.push_event("stroke", "stroke 20,20").unwrap();
}

#[test]
fn a_plain_reply_is_one_utter_intent() {
    let tmp = TempDir::new("intent-plain");
    let control = intent_control(&tmp);
    mount_voice(&control, "moved the stroke", &[]);
    let id = control.open_thread(&stroke_card()).unwrap();
    push_stroke(&control);
    let reply = said(control.offer(id, "voice").unwrap());

    assert_eq!(
        reply.intents,
        vec![ReplyIntent::Utter {
            text: "moved the stroke".into()
        }]
    );
    // Nothing was denied: no intent-tagged events on tape.
    let tape = control.events(id).unwrap();
    assert!(
        !tape
            .iter()
            .any(|event| event.tags.iter().any(|tag| tag == "intent"))
    );
}

#[test]
fn a_structured_reply_carries_utter_and_declared_raise() {
    let tmp = TempDir::new("intent-structured");
    let control = intent_control(&tmp);
    mount_voice(
        &control,
        r#"{"utter":"done","raise":[{"event":"stroke.done","body":"511"}]}"#,
        &["stroke.done"],
    );
    let id = control.open_thread(&stroke_card()).unwrap();
    push_stroke(&control);
    let replies = said_all(control.dispatch(id).unwrap());

    assert_eq!(
        replies[0].intents,
        vec![
            ReplyIntent::Utter {
                text: "done".into()
            },
            ReplyIntent::Raise {
                event: "stroke.done".into(),
                body: "511".into()
            },
        ]
    );
    let tape = control.events(id).unwrap();
    assert!(
        !tape
            .iter()
            .any(|event| event.tags.iter().any(|tag| tag == "intent"))
    );
}

#[test]
fn an_undeclared_raise_is_stripped_and_the_denial_taped() {
    let tmp = TempDir::new("intent-undeclared");
    let control = intent_control(&tmp);
    mount_voice(
        &control,
        r#"{"utter":"done","raise":[{"event":"stroke.done","body":"511"}]}"#,
        &[],
    );
    let id = control.open_thread(&stroke_card()).unwrap();
    push_stroke(&control);
    let reply = said(control.offer(id, "voice").unwrap());

    // The declared utter survives; the raise does not reach the host.
    assert_eq!(
        reply.intents,
        vec![ReplyIntent::Utter {
            text: "done".into()
        }]
    );
    // The denial is taped, naming the stripped intent.
    let tape = control.events(id).unwrap();
    assert!(tape.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "intent") && event.content.contains("stroke.done")
    }));
}

#[test]
fn json_outside_the_convention_is_a_plain_utter() {
    let tmp = TempDir::new("intent-other-json");
    let control = intent_control(&tmp);
    mount_voice(&control, r#"{"note":"hi"}"#, &[]);
    let id = control.open_thread(&stroke_card()).unwrap();
    push_stroke(&control);
    let reply = said(control.offer(id, "voice").unwrap());

    assert_eq!(
        reply.intents,
        vec![ReplyIntent::Utter {
            text: r#"{"note":"hi"}"#.into()
        }]
    );
}

#[test]
fn a_near_miss_with_a_mistyped_field_is_a_plain_utter() {
    let tmp = TempDir::new("intent-mistyped");
    let control = intent_control(&tmp);
    mount_voice(&control, r#"{"utter":5}"#, &[]);
    let id = control.open_thread(&stroke_card()).unwrap();
    push_stroke(&control);
    let reply = said(control.offer(id, "voice").unwrap());

    // The boundary is a full clean parse: a malformed intents doc
    // degrades to visible text rather than vanishing.
    assert_eq!(
        reply.intents,
        vec![ReplyIntent::Utter {
            text: r#"{"utter":5}"#.into()
        }]
    );
}
