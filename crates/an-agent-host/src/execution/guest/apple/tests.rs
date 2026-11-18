use super::*;

fn fixture() -> (crate::cocoon::Profile, Profile, Value) {
    let apple = Profile {
        cli: PathBuf::from("/operator/container"),
        cli_sha256: "a".repeat(64),
        ledger: PathBuf::from("/operator/ledger"),
        image: format!("image@sha256:{}", "b".repeat(64)),
        attempt: an_agent_core::det_seam::Entropy::os().uuid_v4().to_string(),
        expires_unix: 0,
        created_date: "created".into(),
        started_date: "started".into(),
    };
    let profile = crate::cocoon::Profile {
        socket: PathBuf::new(),
        apple_container: Some(apple.clone()),
        grant: String::new(),
        caller: "primary".into(),
        vm: an_agent_core::det_seam::Entropy::os().uuid_v4().to_string(),
        generation: 1,
    };
    let state = json!([{"id":profile.vm,"configuration":{"id":profile.vm,
        "creationDate":"created","labels":{"an-agent.attempt":apple.attempt,"an-agent.generation":"1"},
        "image":{"descriptor":{"digest":format!("sha256:{}","b".repeat(64))}},
        "platform":{"os":"linux","architecture":"arm64"},"ssh":false,"virtualization":false,
        "networks":[],"mounts":[],"publishedPorts":[],"publishedSockets":[]},
        "status":{"state":"running","startedDate":"started"}}]);
    (profile, apple, state)
}

#[test]
fn different_owner_boot_image_or_attachment_is_rejected() {
    let (profile, apple, state) = fixture();
    assert!(owned(&state, &profile, &apple).is_ok());
    for pointer in [
        "/0/id",
        "/0/configuration/id",
        "/0/configuration/creationDate",
        "/0/configuration/labels/an-agent.attempt",
        "/0/configuration/labels/an-agent.generation",
        "/0/configuration/image/descriptor/digest",
        "/0/status/startedDate",
        "/0/configuration/platform/architecture",
        "/0/configuration/platform/os",
    ] {
        let mut changed = state.clone();
        *changed.pointer_mut(pointer).unwrap() = json!("different");
        assert!(owned(&changed, &profile, &apple).is_err(), "{pointer}");
    }
    for field in [
        "networks",
        "mounts",
        "publishedPorts",
        "publishedSockets",
        "ssh",
        "virtualization",
    ] {
        let mut changed = state.clone();
        changed[0]["configuration"][field] = json!(true);
        assert!(owned(&changed, &profile, &apple).is_err(), "{field}");
    }
    assert!(owned(&json!([]), &profile, &apple).is_err());
    assert!(apple.active().is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn lost_runtime_receipt_is_never_reexecuted_and_revocation_denies_new_work() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let (mut profile, mut apple, state) = fixture();
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("an-apple-test-{}", apple.attempt));
    store::directory(&root).unwrap();
    let cli = root.join("container-fixture");
    store::write_new(
        &root.join("inspection.json"),
        &serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    store::write_new(&cli, b"#!/bin/sh\nif [ \"$1\" = inspect ]; then cat inspection.json; else cat > input.json; printf '%s\\n' \"$@\" > arguments; echo effect >> effects; echo '{}'; fi\n").unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();
    apple.cli_sha256 = store::digest(&fs::read(&cli).unwrap());
    apple.cli = cli;
    apple.ledger = root.clone();
    apple.expires_unix = u64::MAX;
    profile.apple_container = Some(apple.clone());
    let selection = json!({"vm":profile.vm,"generation":profile.generation,"caller":profile.caller,
        "apple_container":apple});
    for (index, extra) in [
        json!({}),
        json!({"grant":"a".repeat(64)}),
        json!({"socket":"/operator/api.sock"}),
    ]
    .into_iter()
    .enumerate()
    {
        let mut value = selection.clone();
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let bytes = serde_json::to_vec(&value).unwrap();
        let path = root.join(format!("profile-{index}.json"));
        store::write_new(&path, &bytes).unwrap();
        assert_eq!(
            crate::cocoon::load(&path, &store::digest(&bytes)).is_ok(),
            index == 0
        );
    }
    let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let operation = json!({"operation":"guest","request":{"identity":{"attempt":apple.attempt},
        "operation":{"kind":"exec","argv":["/bin/true","sensitive-input"],"cwd":"/workspace"}}});
    assert!(exchange(&profile, &id, operation.clone()).is_ok());
    assert!(
        fs::read_to_string(root.join("input.json"))
            .unwrap()
            .contains("sensitive-input")
    );
    assert!(
        !fs::read_to_string(root.join("arguments"))
            .unwrap()
            .contains("sensitive-input")
    );
    fs::remove_file(root.join(format!("{id}.result.json"))).unwrap();
    assert!(exchange(&profile, &id, operation.clone()).is_err());
    let query = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    assert_eq!(
        exchange(
            &profile,
            &query,
            json!({"operation":"operation_status","id":id})
        )
        .unwrap()["status"],
        "unknown"
    );
    store::write_new(&root.join("revoked.json"), b"{}").unwrap();
    assert!(exchange(&profile, &query, operation).is_err());
    assert_eq!(
        fs::read_to_string(root.join("effects")).unwrap(),
        "effect\n"
    );
    fs::remove_dir_all(root).unwrap();
}
