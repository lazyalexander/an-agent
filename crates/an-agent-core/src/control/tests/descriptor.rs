//! Descriptor identity follows real mounts and admission, not live files.

use super::supervision::{Cautious, setup};
use super::*;
use an_agent_semantic::{Attempt, FactRef};

fn descriptor() -> SpoolDescriptor {
    SpoolDescriptor {
        package: "brush".into(),
        yaml: "name: brush\nconsumes: [scene]\nproduces: []\n".into(),
    }
}

fn fact<'a>(facts: &'a [FactRef], kind: &str) -> &'a str {
    &facts
        .iter()
        .find(|fact| fact.kind == kind)
        .unwrap()
        .revision
}

fn deliveries(control: &AgentControl, child: Uuid) -> Vec<Attempt> {
    control
        .events(child)
        .unwrap()
        .iter()
        .filter(|event| event.tags.iter().any(|tag| tag == "semantic"))
        .filter_map(|event| {
            let value: Value = serde_json::from_str(&event.content).unwrap();
            let attempt: Attempt = serde_json::from_value(value["attempt"].clone()).unwrap();
            (attempt.capability.operation == "flow.consume").then_some(attempt)
        })
        .collect()
}

struct Echo;
impl SpoolBeat for Echo {
    fn receive(&self, _: &WorkspaceRecord) -> Result<String, String> {
        Ok("painted".into())
    }
}

#[test]
fn package_is_shared_but_mounts_are_distinct_and_own_their_snapshot() {
    let (_tmp, control, child, _) = setup(Permit::Go);
    control
        .register(&listed(&[("scene", &["left", "right"])], &[], &[]))
        .unwrap();
    let mut source = descriptor();
    let original = source.yaml.clone();
    let mut declaration = declares(&["scene"], &[]);
    for name in ["left", "right"] {
        control
            .mount_spool_described(name, Arc::new(Echo), &declaration, &source)
            .unwrap();
    }
    source.package = "other".into();
    source.yaml = "name: other\n".into();
    declaration.consumes.clear();
    let event = control.push_event("scene", "stroke").unwrap();
    assert_eq!(control.dispatch_on(child, &event.id).unwrap().len(), 2);
    let attempts = deliveries(&control, child);
    assert_eq!(attempts.len(), 2);
    for attempt in &attempts {
        assert_eq!(fact(&attempt.facts, "spool.package"), "brush");
        let bytes = std::fs::read(
            control
                .directory(child)
                .unwrap()
                .join("blobs")
                .join(fact(&attempt.facts, "spool.descriptor")),
        )
        .unwrap();
        assert_eq!(bytes, original.as_bytes());
    }
    assert_ne!(
        fact(&attempts[0].facts, "spool.instance"),
        fact(&attempts[1].facts, "spool.instance")
    );
    assert_eq!(
        fact(&attempts[0].facts, "spool.descriptor"),
        fact(&attempts[1].facts, "spool.descriptor")
    );
}

#[test]
fn descriptor_replacement_requires_remount_and_never_inherits_review() {
    for change_yaml in [false, true] {
        let (_tmp, control, child, main) = setup(Permit::Go);
        control
            .register(&listed(&[("scene", &["body"])], &[], &[]))
            .unwrap();
        let declaration = declares(&["scene"], &[]);
        let mut source = descriptor();
        control
            .mount_agent_described("body", Arc::new(Cautious), &declaration, &source)
            .unwrap();
        let event = control.push_event("scene", "stroke").unwrap();
        let DeliveryOutcome::NeedsReview(old) = control.offer_on(child, "body", &event.id).unwrap()
        else {
            panic!("review expected");
        };
        if change_yaml {
            source.yaml.push_str("summary: changed\n");
        }
        assert!(matches!(
            control.mount_agent_described("body", Arc::new(Cautious), &declaration, &source),
            Err(ControlError::AlreadyMounted(_))
        ));
        control.unmount_spool("body").unwrap();
        assert!(control.pending_reviews(main).unwrap().is_empty());
        control
            .mount_agent_described("body", Arc::new(Cautious), &declaration, &source)
            .unwrap();
        let DeliveryOutcome::NeedsReview(new) = control.offer_on(child, "body", &event.id).unwrap()
        else {
            panic!("fresh review expected");
        };
        assert_ne!(old.id, new.id);
        assert_ne!(
            old.scope.as_ref().unwrap().mount,
            new.scope.as_ref().unwrap().mount
        );
        assert_eq!(
            fact(&old.attempt.facts, "spool.package"),
            fact(&new.attempt.facts, "spool.package")
        );
        assert_eq!(
            fact(&old.attempt.facts, "spool.descriptor")
                != fact(&new.attempt.facts, "spool.descriptor"),
            change_yaml
        );
        assert!(matches!(
            control.resolve_review(
                main,
                &old.id,
                ReviewResolution::Retry { note: "old".into() }
            ),
            Err(ControlError::ReviewMissing(_))
        ));
    }
}

#[test]
fn descriptorless_mounts_remain_supported_without_invented_package_evidence() {
    let (_tmp, control, child, _) = setup(Permit::Go);
    control
        .register(&listed(&[("scene", &["body"])], &[], &[]))
        .unwrap();
    control
        .mount_spool("body", Arc::new(Echo), &declares(&["scene"], &[]))
        .unwrap();
    let event = control.push_event("scene", "stroke").unwrap();
    control.offer_on(child, "body", &event.id).unwrap();
    assert!(
        deliveries(&control, child)[0]
            .facts
            .iter()
            .all(|fact| fact.kind != "spool.package" && fact.kind != "spool.descriptor")
    );
}

#[test]
fn incomplete_descriptors_are_refused_before_mounting() {
    let (_tmp, control, _, _) = setup(Permit::Go);
    for source in [
        SpoolDescriptor {
            package: " ".into(),
            yaml: "name: brush".into(),
        },
        SpoolDescriptor {
            package: "brush".into(),
            yaml: " ".into(),
        },
    ] {
        assert!(matches!(
            control.mount_spool_described("body", Arc::new(Echo), &declares(&[], &[]), &source),
            Err(ControlError::MountMismatch { .. })
        ));
    }
    control
        .mount_spool("body", Arc::new(Echo), &declares(&[], &[]))
        .unwrap();
}
