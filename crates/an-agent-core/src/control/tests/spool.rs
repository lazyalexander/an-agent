//! Offer and dispatch. A failed body is unmounted. The event stays.

use super::*;
use super::{Boom, Clip, Ink, listed, stroke_card};

#[test]
fn offer_hands_each_spool_the_recorded_event() {
    let tmp = TempDir::new("control-offer");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(&[("stroke", &["clip", "ink"])], &[], &[]))
        .unwrap();
    let id = control.open_thread(&stroke_card()).unwrap();
    let stroke = control
        .push_event("stroke", "stroke 20,20 600,40 480,700")
        .unwrap();
    assert!(matches!(
        control.offer(id, "clip"),
        Err(ControlError::NotMounted(_))
    ));
    assert!(matches!(
        control.mount_spool("clip", Arc::new(Clip)),
        Ok(())
    ));
    assert!(matches!(
        control.mount_spool("clip", Arc::new(Clip)),
        Err(ControlError::AlreadyMounted(_))
    ));

    let clipped = control.offer(id, "clip").unwrap();
    assert_eq!(clipped.spool, "clip");
    assert_eq!(clipped.event_id, stroke.id);
    assert_eq!(clipped.reply, "stroke 20,20 511,40 480,511");
    let corrected = control.push_event("stroke", &clipped.reply).unwrap();
    let ink = Arc::new(Ink {
        seen: Mutex::new(None),
    });
    control
        .mount_spool("ink", Arc::clone(&ink) as Arc<dyn SpoolBeat>)
        .unwrap();
    let painted = control.offer(id, "ink").unwrap();
    assert_eq!(painted.spool, "ink");
    assert_eq!(painted.event_id, corrected.id);
    assert_eq!(
        ink.seen
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_deref(),
        Some("stroke 20,20 511,40 480,511")
    );

    let log: Vec<_> = control
        .workspace_log()
        .unwrap()
        .into_iter()
        .filter(|record| record.kind == "event")
        .filter_map(|record| record.body)
        .collect();
    assert_eq!(
        log,
        vec![
            "stroke 20,20 600,40 480,700".to_string(),
            "stroke 20,20 511,40 480,511".to_string(),
        ]
    );
    let tape = control.events(id).unwrap();
    assert!(tape.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "spool")
            && event.content.contains("\"spool\":\"clip\"")
            && event.content.contains(&stroke.id)
    }));
    assert!(tape.iter().any(|event| {
        event.tags.iter().any(|tag| tag == "spool")
            && event.content.contains("\"spool\":\"ink\"")
            && event.content.contains(&corrected.id)
            && event.content.contains("painted")
    }));
}

#[test]
fn a_failed_spool_is_unmounted_and_the_event_stays() {
    let tmp = TempDir::new("control-offer-fail");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(&[("stroke", &["boom", "ink"])], &[], &[]))
        .unwrap();
    let id = control.open_thread(&stroke_card()).unwrap();
    let stroke = control.push_event("stroke", "stroke 20,20 600,40").unwrap();
    let ink = Arc::new(Ink {
        seen: Mutex::new(None),
    });
    control.mount_spool("boom", Arc::new(Boom)).unwrap();
    control
        .mount_spool("ink", Arc::clone(&ink) as Arc<dyn SpoolBeat>)
        .unwrap();
    assert!(matches!(
        control.offer(id, "boom"),
        Err(ControlError::SpoolFailed { reason, .. }) if reason == "crashed"
    ));
    assert!(matches!(
        control.offer(id, "boom"),
        Err(ControlError::NotMounted(_))
    ));
    let painted = control.offer(id, "ink").unwrap();
    assert_eq!(painted.spool, "ink");
    assert_eq!(painted.event_id, stroke.id);
    assert_eq!(
        ink.seen
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_deref(),
        Some("stroke 20,20 600,40")
    );
    let log: Vec<_> = control
        .workspace_log()
        .unwrap()
        .into_iter()
        .filter(|record| record.kind == "event")
        .filter_map(|record| record.body)
        .collect();
    assert_eq!(log, vec!["stroke 20,20 600,40".to_string()]);
    assert!(control.events(id).unwrap().iter().any(|event| {
        event.tags.iter().any(|tag| tag == "spool") && event.content.contains("crashed")
    }));
}

#[test]
fn dispatch_follows_the_route_and_continues_after_a_crash() {
    let tmp = TempDir::new("control-dispatch");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    let id = control.open_thread(&stroke_card()).unwrap();
    assert!(matches!(control.dispatch(id), Err(ControlError::NoEvent)));
    control
        .register(&listed(
            &[("stroke", &["clip", "gone", "boom", "ink"])],
            &[],
            &[],
        ))
        .unwrap();
    let stroke = control
        .push_event("stroke", "stroke 20,20 600,40 480,700")
        .unwrap();
    let ink = Arc::new(Ink {
        seen: Mutex::new(None),
    });
    control.mount_spool("clip", Arc::new(Clip)).unwrap();
    control.mount_spool("boom", Arc::new(Boom)).unwrap();
    control
        .mount_spool("ink", Arc::clone(&ink) as Arc<dyn SpoolBeat>)
        .unwrap();

    let replies = control.dispatch(id).unwrap();
    assert_eq!(
        replies,
        vec![
            SpoolReply {
                spool: "clip".into(),
                event_id: stroke.id.clone(),
                reply: "stroke 20,20 511,40 480,511".into(),
            },
            SpoolReply {
                spool: "ink".into(),
                event_id: stroke.id.clone(),
                reply: "painted".into(),
            },
        ]
    );
    assert_eq!(
        ink.seen
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_deref(),
        Some("stroke 20,20 600,40 480,700")
    );
    let log: Vec<_> = control
        .workspace_log()
        .unwrap()
        .into_iter()
        .filter(|record| record.kind == "event")
        .filter_map(|record| record.body)
        .collect();
    assert_eq!(log, vec!["stroke 20,20 600,40 480,700".to_string()]);
    let tape = control.events(id).unwrap();
    for needle in ["511,40", "missing", "crashed", "painted"] {
        assert!(
            tape.iter().any(|event| {
                event.tags.iter().any(|tag| tag == "spool")
                    && event.content.contains(&stroke.id)
                    && event.content.contains(needle)
            }),
            "{needle}"
        );
    }
    assert!(matches!(
        control.mount_spool("boom", Arc::new(Boom)),
        Ok(())
    ));
    assert!(matches!(
        control.mount_spool(
            "ink",
            Arc::new(Ink {
                seen: Mutex::new(None)
            })
        ),
        Err(ControlError::AlreadyMounted(_))
    ));

    control
        .register(&listed(&[("other", &[])], &[], &[]))
        .unwrap();
    assert!(matches!(
        control.dispatch(id),
        Err(ControlError::Unregistered(name)) if name == "stroke"
    ));
}
