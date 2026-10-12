//! Offer and dispatch. A failed body is unmounted. The event stays.

use super::*;
use super::{Boom, Clip, Ink, declares, listed, stroke_card};

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
        control.mount_spool("clip", Arc::new(Clip), &declares(&["stroke"], &[])),
        Ok(())
    ));
    assert!(matches!(
        control.mount_spool("clip", Arc::new(Clip), &declares(&["stroke"], &[])),
        Err(ControlError::AlreadyMounted(_))
    ));

    let clipped = said(control.offer(id, "clip").unwrap());
    assert_eq!(clipped.spool, "clip");
    assert_eq!(clipped.event_id, stroke.id);
    assert_eq!(clipped.reply, "stroke 20,20 511,40 480,511");
    let corrected = control.push_event("stroke", &clipped.reply).unwrap();
    let ink = Arc::new(Ink {
        seen: Mutex::new(None),
    });
    control
        .mount_spool(
            "ink",
            Arc::clone(&ink) as Arc<dyn SpoolBeat>,
            &declares(&["stroke"], &[]),
        )
        .unwrap();
    let painted = said(control.offer(id, "ink").unwrap());
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
    control
        .mount_spool("boom", Arc::new(Boom), &declares(&["stroke"], &[]))
        .unwrap();
    control
        .mount_spool(
            "ink",
            Arc::clone(&ink) as Arc<dyn SpoolBeat>,
            &declares(&["stroke"], &[]),
        )
        .unwrap();
    assert!(matches!(
        control.offer(id, "boom"),
        Err(ControlError::SpoolFailed { reason, .. }) if reason == "crashed"
    ));
    assert!(matches!(
        control.offer(id, "boom"),
        Err(ControlError::NotMounted(_))
    ));
    let painted = said(control.offer(id, "ink").unwrap());
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
    control
        .mount_spool("clip", Arc::new(Clip), &declares(&["stroke"], &[]))
        .unwrap();
    control
        .mount_spool("boom", Arc::new(Boom), &declares(&["stroke"], &[]))
        .unwrap();
    control
        .mount_spool(
            "ink",
            Arc::clone(&ink) as Arc<dyn SpoolBeat>,
            &declares(&["stroke"], &[]),
        )
        .unwrap();

    let replies = said_all(control.dispatch(id).unwrap());
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0].spool, "clip");
    assert_eq!(replies[0].event_id, stroke.id);
    assert_eq!(replies[0].reply, "stroke 20,20 511,40 480,511");
    assert!(!replies[0].tape_id.is_empty());
    assert_eq!(replies[1].spool, "ink");
    assert_eq!(replies[1].event_id, stroke.id);
    assert_eq!(replies[1].reply, "painted");
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
        control.mount_spool("boom", Arc::new(Boom), &declares(&["stroke"], &[])),
        Ok(())
    ));
    assert!(matches!(
        control.mount_spool(
            "ink",
            Arc::new(Ink {
                seen: Mutex::new(None)
            }),
            &declares(&["stroke"], &[]),
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

/// Mount is the bind point: a body's declaration and the register must
/// agree, in both directions, or the mount is refused.
#[test]
fn mount_matches_declaration_against_the_register() {
    let tmp = TempDir::new("control-mount-match");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();

    // No registration: only a silent body mounts.
    assert!(matches!(
        control.mount_spool("clip", Arc::new(Clip), &declares(&["stroke"], &[])),
        Err(ControlError::NotRegistered)
    ));
    control
        .mount_spool(
            "quiet",
            Arc::new(Ink {
                seen: Mutex::new(None),
            }),
            &declares(&[], &[]),
        )
        .unwrap();
    control.unmount_spool("quiet").unwrap();

    control
        .register(&listed(&[("stroke", &["clip"]), ("note", &[])], &[], &[]))
        .unwrap();
    // Happy path: consumed, registered, routed to this spool.
    control
        .mount_spool("clip", Arc::new(Clip), &declares(&["stroke"], &[]))
        .unwrap();

    // Consumed but not registered.
    assert!(matches!(
        control.mount_spool("ink", Arc::new(Ink { seen: Mutex::new(None) }), &declares(&["ghost"], &[])),
        Err(ControlError::MountMismatch { name, reasons }) if name == "ink" && reasons.iter().any(|r| r.contains("unregistered event ghost"))
    ));
    // Registered, but the route delivers to someone else.
    assert!(matches!(
        control.mount_spool("ink", Arc::new(Ink { seen: Mutex::new(None) }), &declares(&["stroke"], &[])),
        Err(ControlError::MountMismatch { reasons, .. }) if reasons.iter().any(|r| r.contains("route does not name ink"))
    ));
    // Produced but not registered.
    assert!(matches!(
        control.mount_spool("ink", Arc::new(Ink { seen: Mutex::new(None) }), &declares(&[], &["ghost"])),
        Err(ControlError::MountMismatch { reasons, .. }) if reasons.iter().any(|r| r.contains("produces unregistered event ghost"))
    ));
    // The reverse direction: the route names clip, so clip cannot stay silent.
    assert!(matches!(
        control.mount_spool("clip2", Arc::new(Clip), &declares(&[], &[])),
        Ok(())
    ));
    control.unmount_spool("clip").unwrap();
    assert!(matches!(
        control.mount_spool("clip", Arc::new(Clip), &declares(&[], &[])),
        Err(ControlError::MountMismatch { reasons, .. }) if reasons.iter().any(|r| r.contains("route stroke names clip"))
    ));
    // All mismatches are reported together.
    assert!(matches!(
        control.mount_spool("ink", Arc::new(Ink { seen: Mutex::new(None) }), &declares(&["ghost", "stroke"], &["ghost"])),
        Err(ControlError::MountMismatch { reasons, .. }) if reasons.len() == 3
    ));
}

/// Mount matching is an early diagnostic, not a lasting authorization. A
/// later registration generation must still pass the body boundary.
#[test]
fn delivery_rechecks_the_current_registration_against_the_mounted_claim() {
    let tmp = TempDir::new("control-delivery-semantic");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(&[("stroke", &["clip"]), ("other", &[])], &[], &[]))
        .unwrap();
    let id = control.open_thread(&stroke_card()).unwrap();
    let clip = Arc::new(Ink {
        seen: Mutex::new(None),
    });
    control
        .mount_spool(
            "clip",
            Arc::clone(&clip) as Arc<dyn SpoolBeat>,
            &declares(&["stroke"], &[]),
        )
        .unwrap();

    // The new snapshot routes an event the mounted body never claimed.
    control
        .register(&listed(&[("stroke", &[]), ("other", &["clip"])], &[], &[]))
        .unwrap();
    let other = control.push_event("other", "not for clip").unwrap();
    assert!(matches!(
        control.dispatch_on(id, &other.id),
        Err(ControlError::SemanticDenied { operation, subject, reason })
            if operation == "flow.consume"
                && subject == "clip"
                && reason == "capability_not_claimed"
    ));
    assert!(
        clip.seen
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .is_none()
    );
    let tape = control.events(id).unwrap();
    let denied = tape
        .iter()
        .find(|event| event.tags.iter().any(|tag| tag == "semantic"))
        .expect("semantic denial");
    assert!(denied.content.contains("capability_not_claimed"));
    assert!(denied.content.contains("workspace.registration"));
    assert!(denied.refs.iter().any(|reference| reference == &other.id));
}

#[test]
fn successful_deliveries_are_audited_with_distinct_execution_ids() {
    let tmp = TempDir::new("control-delivery-audit");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(&[("stroke", &["ink"])], &[], &[]))
        .unwrap();
    let id = control.open_thread(&stroke_card()).unwrap();
    let event = control.push_event("stroke", "same fact").unwrap();
    control
        .mount_spool(
            "ink",
            Arc::new(Ink {
                seen: Mutex::new(None),
            }),
            &declares(&["stroke"], &[]),
        )
        .unwrap();
    control.offer_on(id, "ink", &event.id).unwrap();
    control.offer_on(id, "ink", &event.id).unwrap();
    let tape = control.events(id).unwrap();
    let audits: Vec<Value> = tape
        .iter()
        .filter(|e| e.tags.iter().any(|tag| tag == "semantic"))
        .map(|e| serde_json::from_str(&e.content).unwrap())
        .collect();
    assert_eq!(audits.len(), 2);
    assert_ne!(audits[0]["attempt"]["id"], audits[1]["attempt"]["id"]);
    for audit in &audits {
        assert_eq!(audit["decision"]["disposition"], "allow");
        assert_eq!(audit["attempt"]["refs"][0], event.id);
        let declaration = audit["attempt"]["facts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|fact| fact["kind"] == "spool.declaration")
            .unwrap();
        let bytes = std::fs::read(
            control
                .directory(id)
                .unwrap()
                .join("blobs")
                .join(declaration["revision"].as_str().unwrap()),
        )
        .unwrap();
        let snapshot: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(snapshot["consumes"], json!(["stroke"]));
    }
}

/// The reply loop closes: a named event is delivered, the host lands the
/// reply in software, and `applied` tapes that landing with a citation
/// back to the spool note.
#[test]
fn the_host_lands_a_reply_and_the_world_moves() {
    let tmp = TempDir::new("control-applied");
    let control = AgentControl::open(tmp.path(), &bash_registry()).unwrap();
    control
        .register(&listed(&[("stroke", &["clip"])], &[], &[]))
        .unwrap();
    let id = control.open_thread(&stroke_card()).unwrap();
    let stroke = control
        .push_event("stroke", "stroke 20,20 600,40 480,700")
        .unwrap();
    control
        .mount_spool("clip", Arc::new(Clip), &declares(&["stroke"], &[]))
        .unwrap();

    // Delivery can name an event, not just take the latest.
    assert!(matches!(
        control.dispatch_on(id, "missing-event"),
        Err(ControlError::UnknownEvent(_))
    ));
    let replies = said_all(control.dispatch_on(id, &stroke.id).unwrap());
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].reply, "stroke 20,20 511,40 480,511");

    // The host lands the reply in software, then tapes that landing.
    let applied = control
        .applied(id, &replies[0], "painted the clipped stroke")
        .unwrap();
    let landed = control.push_event("stroke", &replies[0].reply).unwrap();
    assert_ne!(landed.id, stroke.id);

    let tape = control.events(id).unwrap();
    let land = tape
        .iter()
        .find(|event| event.tags.iter().any(|tag| tag == "applied"))
        .expect("applied");
    assert_eq!(land.id, applied);
    assert!(land.content.contains("painted the clipped stroke"));
    assert!(land.refs.iter().any(|r| r == &replies[0].tape_id));
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
}
