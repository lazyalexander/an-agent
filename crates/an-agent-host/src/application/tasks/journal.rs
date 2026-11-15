//! Recover exact original queue references from write-ahead intent, never enqueue.
use super::*;
pub(super) fn recover(service: &Path, root: &Path, state: &mut Value) -> Result<()> {
    let directory = root.join("intents");
    if !directory.exists() {
        return Ok(());
    }
    let snapshot = crate::daemon::request(service, &json!({"op":"show"}))?;
    let tasks = snapshot["tasks"]
        .as_array()
        .ok_or("original task inventory unavailable")?;
    let credential: Value =
        serde_json::from_slice(&store::read(&root.join("credential.json"), 65536)?)?;
    recover_from(
        root,
        state,
        tasks,
        credential["id"].as_str().ok_or("original authorization")?,
    )
}

fn recover_from(root: &Path, state: &mut Value, tasks: &[Value], grant: &str) -> Result<()> {
    let directory = root.join("intents");
    let mut recovered = Vec::new();
    for name in ["primary", "intermediate", "final"] {
        let path = directory.join(format!("{name}.json"));
        if !path.exists() {
            continue;
        }
        let intent: Value = serde_json::from_slice(&store::read(&path, 131072)?)?;
        if intent["kind"]
            != if name == "primary" {
                "harness"
            } else {
                "spool"
            }
            || intent["grant"] != grant
            || !store::valid_digest(
                intent["request"]
                    .as_str()
                    .ok_or("original request digest")?,
            )
        {
            return Err("invalid original intent".into());
        }
        let matches = tasks
            .iter()
            .filter(|task| {
                task["kind"] == intent["kind"]
                    && task["request"] == intent["request"]
                    && task["authorization"]["grant"]["id"] == intent["grant"]
                    && (name == "primary"
                        || (task["instance"] == intent["instance"] && task["run"] == intent["run"]))
            })
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            return Err("original intent has ambiguous executions".into());
        }
        if let Some(task) = matches.first() {
            let id = task["id"].as_str().ok_or("original execution identity")?;
            if uuid::Uuid::parse_str(id)?.to_string() != id {
                return Err("invalid original execution identity".into());
            }
            let slot = if name == "primary" {
                &mut state["session"]
            } else {
                &mut state["steps"][name]
            };
            if !slot.is_null() && *slot != task["id"] {
                return Err("original execution binding changed".into());
            }
            *slot = task["id"].clone();
            recovered.push(task["id"].clone());
        } else if !(if name == "primary" {
            &state["session"]
        } else {
            &state["steps"][name]
        })
        .is_null()
        {
            return Err("original execution absent from inventory".into());
        }
    }
    if tasks.iter().any(|task| {
        task["authorization"]["grant"]["id"] == grant && !recovered.contains(&task["id"])
    }) {
        return Err("original grant has an execution without matching intent".into());
    }
    Ok(())
}

/// Revoke on the same serialized daemon before taking the queue inventory.
pub(super) fn fence(service: &Path, root: &Path, id: &str) -> Result<()> {
    if !super::setup::versioned(root)? {
        if root.join("intents").exists() {
            return Err("original recovery version unavailable".into());
        }
        return Ok(());
    }
    let path = root.join("credential.json");
    if !path.exists() {
        if root.join("intents").exists() {
            return Err("original authorization record unavailable".into());
        }
        return Ok(());
    }
    let credential: Value = serde_json::from_slice(&store::read(&path, 65536)?)?;
    let grant_id = credential["id"].as_str().ok_or("original grant identity")?;
    let binding = crate::task_permissions::binding(service, grant_id)?;
    if crate::task_permissions::load(&binding)?.scope.task != id {
        return Err("original grant task mismatch".into());
    }
    let request = json!({"op":"revoke-grant","id":grant_id});
    let preview = crate::daemon::request(service, &request)?;
    crate::daemon::request(
        service,
        &json!({"op":"revoke-grant","id":grant_id,"approve":preview["approval_sha256"]}),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lost_primary_and_validator_replies_recover_only_unique_originals() -> Result<()> {
        let (_temporary, _service, _id, root) = super::super::tests::fixture()?;
        store::directory(&root.join("intents"))?;
        let mut tasks = Vec::new();
        let mut state = json!({"session":null,"steps":{}});
        for name in ["primary", "intermediate", "final"] {
            let kind = if name == "primary" {
                "harness"
            } else {
                "spool"
            };
            let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
            let intent = json!({"kind":kind,"request":"a".repeat(64),"grant":"original",
                "instance":name,"run":"deliver"});
            store::write_new(
                &root.join("intents").join(format!("{name}.json")),
                &serde_json::to_vec(&intent)?,
            )?;
            tasks.push(json!({"id":id,"kind":kind,"request":intent["request"],
                "authorization":{"grant":{"id":"original"}},"instance":name,"run":"deliver"}));
        }
        recover_from(&root, &mut state, &tasks, "original")?;
        assert_eq!(state["session"], tasks[0]["id"]);
        assert_eq!(state["steps"]["intermediate"], tasks[1]["id"]);
        assert_eq!(state["steps"]["final"], tasks[2]["id"]);
        let retained = state.clone();
        recover_from(&root, &mut state, &tasks, "original")?;
        assert_eq!(state, retained);
        tasks.push(tasks[0].clone());
        assert!(recover_from(&root, &mut state, &tasks, "original").is_err());
        Ok(())
    }

    #[test]
    fn wrong_request_grant_instance_and_run_cannot_rebind_an_original() -> Result<()> {
        let (_temporary, _service, _id, root) = super::super::tests::fixture()?;
        store::directory(&root.join("intents"))?;
        let intent = json!({"kind":"spool","request":"a".repeat(64),"grant":"original",
            "instance":"accepted-instance","run":"deliver"});
        store::write_new(
            &root.join("intents/intermediate.json"),
            &serde_json::to_vec(&intent)?,
        )?;
        let task = json!({"id":an_agent_core::det_seam::Entropy::os().uuid_v4().to_string(),
            "kind":"spool","request":intent["request"],"authorization":{"grant":{"id":"original"}},
            "instance":intent["instance"],"run":intent["run"]});
        for field in ["request", "grant", "instance", "run", "kind"] {
            let mut changed = task.clone();
            if field == "grant" {
                changed["authorization"]["grant"]["id"] = json!("foreign");
            } else {
                changed[field] = json!("foreign");
            }
            let mut state = json!({"steps":{}});
            if field == "grant" {
                recover_from(&root, &mut state, &[changed.clone()], "original")?;
            } else {
                assert!(recover_from(&root, &mut state, &[changed.clone()], "original").is_err());
            }
            assert!(state["steps"].as_object().unwrap().is_empty());
            state["steps"]["intermediate"] = task["id"].clone();
            assert!(recover_from(&root, &mut state, &[changed], "original").is_err());
        }
        let mut state = json!({"steps":{"intermediate":"another-original"}});
        assert!(recover_from(&root, &mut state, &[task], "original").is_err());
        Ok(())
    }

    #[test]
    fn missing_or_foreign_authorization_does_not_allow_recovery() -> Result<()> {
        let (_temporary, service, id, root) = super::super::tests::fixture()?;
        store::write_new(&root.join("recovery-version.json"), b"{\"version\":1}")?;
        store::directory(&root.join("intents"))?;
        assert!(fence(&service, &root, &id).is_err());
        store::write_new(&root.join("input"), b"input")?;
        let scope = crate::task_permissions::Scope {
            task: "foreign-task".into(),
            subject: "primary".into(),
            delegate: false,
            expires_ms: an_agent_core::det_seam::Clock::wall().now_ms() + 60000,
            permissions: vec![crate::task_permissions::Permission::ReadInput {
                path: root.join("input"),
                sha256: "a".repeat(64),
            }],
        };
        let (binding, credential) = crate::task_permissions::issue(&service, scope, None)?;
        store::write_new(
            &root.join("credential.json"),
            &serde_json::to_vec(&credential)?,
        )?;
        assert!(fence(&service, &root, &id).is_err());
        crate::task_permissions::active(&binding)?;
        Ok(())
    }
}
