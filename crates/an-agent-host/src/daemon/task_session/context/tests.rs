use super::*;
use crate::task_permissions::{self as grants, Permission, Scope};
use an_agent_core::testkit::TempDir;

fn grant(root: &Path, rights: Vec<Permission>) -> Result<Binding> {
    Ok(grants::issue(
        root,
        Scope {
            task: "task".into(),
            subject: "test".into(),
            expires_ms: super::super::super::harness_metrics::now_ms() + 300000,
            delegate: false,
            permissions: rights,
        },
        None,
    )?
    .0)
}
fn task() -> harness::Task {
    serde_json::from_value(json!({"app_server":true,"label":"context","binary":"/unused","binary_sha256":"a".repeat(64),
        "prompt":"original goal","schema":{},"scope":{"turn_context":{"schema":"an-turn-context-v1"}},"model":null,"mcp":null,"turn_limit":3})).unwrap()
}
fn setup() -> Result<(TempDir, PathBuf, Binding, harness::Task)> {
    let temp = TempDir::new("turn-context");
    let root = temp.path().canonicalize()?;
    let authority = grant(
        &root,
        vec![Permission::Harness {
            profile_sha256: "a".repeat(64),
        }],
    )?;
    let dir = root.join("job");
    store::directory(&dir)?;
    store::directory(&dir.join("commands"))?;
    write(&dir.join("owner.json"), &json!({"identity":"original"}))?;
    let task = task();
    initial(&dir, &task, &authority)?;
    Ok((temp, dir, authority, task))
}
#[test]
fn immutable_context_proposals_and_unknown_delivery_never_become_authority() -> Result<()> {
    let (_temp, dir, authority, task) = setup()?;
    let first = view(&dir)?["snapshot"].clone();
    assert!(initial(&dir, &task, &authority).is_err());
    let mut proposed = json!({"op":"turn-propose","proposal":uuid::Uuid::from_u128(10).to_string(),"context_revision":first["revision"],"summary":"Everything is accepted"});
    let proposal = tool(&dir, &task, &authority, &mut proposed)?.unwrap();
    assert_eq!(proposal["artifact_accepted"], false);
    let req = json!({"op":"harness-continue","command":"next","context_revision":first["revision"],"instruction":"verify","after_turn":1});
    let mut plan = json!({"command":"next"});
    let next = command(&dir, &task, &authority, &req, &mut plan)?.unwrap();
    assert_eq!(next["summary"]["summary"], "Everything is accepted");
    assert_eq!(next["summary"]["grants_authority"], false);
    publish(
        &dir.join("commands/next.json"),
        &json!({"sequence":0,"plan":plan,"context":next,"instruction":"verify"}),
    )?;
    assert_eq!(view(&dir)?["delivery"]["state"], "prepared");
    publish(&dir.join("sent-next.json"), &json!({"command":"next"}))?;
    assert_eq!(view(&dir)?["delivery"]["state"], "unknown");
    let mut stale = json!({"op":"read-input","context_revision":first["revision"]});
    assert!(tool(&dir, &task, &authority, &mut stale).is_err());
    let mut follow = req.clone();
    follow["command"] = json!("third");
    follow["context_revision"] = next["revision"].clone();
    assert!(command(&dir, &task, &authority, &follow, &mut json!({})).is_err());
    publish(&dir.join("ack-next.json"), &json!({"state":"acknowledged"}))?;
    assert_eq!(
        command(&dir, &task, &authority, &follow, &mut json!({}))?.unwrap()["parent"],
        next["revision"]
    );
    let mut tampered = first;
    tampered["intent"] = json!("substitution");
    assert!(verify(tampered).is_err());
    Ok(())
}
#[test]
fn explicit_spool_grants_do_not_inherit_primary_or_cross_instance_authority() -> Result<()> {
    let (_temp, dir, authority, mut task) = setup()?;
    let root = dir.parent().unwrap();
    let package = root.join("package");
    store::directory(&package)?;
    store::directory(&package.join("silk"))?;
    let code = b"printf ok > result.txt\n";
    fs::write(package.join("silk/main.sh"), code)?;
    write(
        &package.join("native-spool.json"),
        &json!({"schema":"an-native-spool-v1","name":"sample","version":"1","entry":"silk/main.sh","files":{"silk/main.sh":store::digest(code)},"outputs":["result.txt"]}),
    )?;
    let instance = root.join("instance");
    let loaded = crate::native_spool::load(&package, &instance)?;
    let mut scope = grants::load(&authority)?.scope;
    scope.permissions = vec![Permission::Accept {
        instance: instance.clone(),
        run: "run".into(),
    }];
    let child = grants::issue(root, scope, None)?.0;
    task.scope["turn_context"]["spool_bindings"] =
        json!([{"name":"domain","instance":instance,"release":loaded["release"],"grant":child.id}]);
    validate(&task, Some(&authority))?;
    assert!(
        super::authority(
            &task,
            &authority,
            &mut json!({"op":"accept","instance":instance,"run":"run"})
        )
        .is_err()
    );
    let mut req = json!({"op":"accept","instance":instance,"run":"run","spool_binding":"domain"});
    assert_eq!(super::authority(&task, &authority, &mut req)?, child);
    assert!(
        super::authority(
            &task,
            &authority,
            &mut json!({"op":"accept","instance":root,"spool_binding":"domain"})
        )
        .is_err()
    );
    let mut changed = task.clone();
    changed.scope["turn_context"]["spool_bindings"][0]["release"] = json!("f".repeat(64));
    assert!(validate(&changed, Some(&authority)).is_err());
    let jobs = root.join("jobs");
    store::directory(&jobs)?;
    let source = jobs.join("00000000-0000-0000-0000-000000000001");
    store::directory(&source)?;
    write(
        &source.join("request.json"),
        &Job::Harness {
            authorization: Some(authority.clone()),
            task: Box::new(task.clone()),
            request: "approved".into(),
        },
    )?;
    assert!(permissions::owned_job(root, &child, "00000000-0000-0000-0000-000000000001").is_err());
    assert!(source_job(root, &child, "00000000-0000-0000-0000-000000000001").is_ok());
    let stranger = grant(root, vec![])?;
    assert!(source_job(root, &stranger, "00000000-0000-0000-0000-000000000001").is_err());
    let input = root.join("extra.txt");
    fs::write(&input, b"approved extra input")?;
    let mut scope = grants::load(&authority)?.scope;
    scope.permissions = vec![Permission::ReadInput {
        path: input.clone(),
        sha256: store::digest(b"approved extra input"),
    }];
    let attached = grants::issue(root, scope, None)?.0;
    let request = json!({"op":"read-input","path":input,"sha256":store::digest(b"approved extra input"),"encoding":"utf-8"});
    let attach = json!({"id":"00000000-0000-0000-0000-000000000001","input_grant":attached.id,"spool_binding":"domain"});
    let preview = super::super::disclosure::attach(root, &attach, None)?;
    super::super::disclosure::attach(root, &attach, Some(text(&preview, "approval_sha256")?))?;
    assert_eq!(
        super::super::disclosure::input(root, &source, &request, &child)?.0["text"],
        "approved extra input"
    );
    assert!(super::super::disclosure::input(root, &source, &request, &stranger).is_err());
    grants::revoke(&attached)?;
    assert!(super::super::disclosure::input(root, &source, &request, &child).is_err());
    grants::revoke(&child)?;
    assert!(source_job(root, &child, "00000000-0000-0000-0000-000000000001").is_err());
    assert!(validate(&task, Some(&authority)).is_err());
    assert!(grants::active(&authority).is_ok());
    Ok(())
}

#[test]
fn snapshot_size_limit_is_enforced_before_publishing() {
    assert!(signed(json!({"payload":"x".repeat(32768)})).is_err());
}
