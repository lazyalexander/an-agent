use super::*;
use crate::execution::store;
use an_agent_core::testkit::TempDir;
use std::{fs, path::PathBuf};

fn package(base: &Path, name: &str, schema: &str) -> Result<PathBuf> {
    let root = base.canonicalize()?.join(name);
    fs::create_dir_all(root.join("silk"))?;
    fs::create_dir(root.join("kit"))?;
    let code = b"exit 0\n";
    for file in ["silk/main.sh", "kit/edit.sh"] {
        fs::write(root.join(file), code)?;
    }
    let mut value = json!({"schema":schema,"name":"sample","version":"1.0.0",
        "entry":"silk/main.sh","files":{"silk/main.sh":store::digest(code),
        "kit/edit.sh":store::digest(code)},"outputs":["report.txt"]});
    if schema == "an-native-spool-v2" {
        value["requirements"] = json!({"runtime":"macos-native-process-v1","os":"macos",
            "arch":"portable-source","interaction":"headless","gpu":"none",
            "inputs":["input.txt"],"editable":["kit/edit.sh"],
            "capabilities":["read-input","write-declared-outputs"],"dependencies":[]});
    }
    fs::write(root.join(bundle::FILE), serde_json::to_vec(&value)?)?;
    Ok(root)
}

fn edit(root: &Path, change: impl FnOnce(&mut Value)) -> Result<()> {
    let file = root.join(bundle::FILE);
    let mut value = serde_json::from_slice(&fs::read(&file)?)?;
    change(&mut value);
    fs::write(file, serde_json::to_vec(&value)?)?;
    Ok(())
}

#[test]
fn exact_identity_and_byte_only_changes_are_distinct() -> Result<()> {
    let temp = TempDir::new("native-release-identity");
    let a = package(temp.path(), "a", "an-native-spool-v1")?;
    let b = package(temp.path(), "b", "an-native-spool-v1")?;
    let identical = compare_releases(&a, &b)?;
    assert_eq!(identical["same_release"], true);
    assert_eq!(identical["review_required"], false);
    assert_eq!(identical["execution_granted"], false);
    assert_eq!(
        identical["candidate"]["runtime_supported"],
        cfg!(target_os = "macos")
    );
    let file = b.join(bundle::FILE);
    let original = fs::read(&file)?;
    let mut padded = original.clone();
    padded.push(b'\n');
    fs::write(&file, &padded)?;
    let changed = compare_releases(&a, &b)?;
    assert_eq!(changed["same_release"], false);
    assert_eq!(changed["reused_coordinates"], true);
    assert_eq!(changed["changes"]["code"], json!([]));
    assert_eq!(changed["changes"]["manifest"], json!([]));
    assert_eq!(changed["review_required"], true);
    assert_eq!(fs::read(a.join(bundle::FILE))?, original);
    assert_eq!(fs::read(file)?, padded);
    Ok(())
}

#[test]
fn unsupported_requests_are_visible_without_weakening_load() -> Result<()> {
    let temp = TempDir::new("native-release-requests");
    let a = package(temp.path(), "a", "an-native-spool-v2")?;
    let b = package(temp.path(), "b", "an-native-spool-v2")?;
    let bytes = b"printf updated\n";
    fs::write(b.join("kit/edit.sh"), bytes)?;
    edit(&b, |m| {
        m["files"]["kit/edit.sh"] = json!(store::digest(bytes));
        m["requirements"]["capabilities"] = json!(["network"]);
        m["requirements"]["dependencies"] = json!(["unresolved-library"]);
        m["requirements"]["editable"] = json!([]);
        m["outputs"] = json!(["new.txt"]);
    })?;
    let result = compare_releases(&a, &b)?;
    assert_eq!(result["candidate"]["runtime_supported"], false);
    assert!(result["candidate"]["runtime_refusal"].is_string());
    assert_eq!(result["changes"]["code"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["changes"]["requirements"].as_array().unwrap().len(),
        3
    );
    assert_eq!(result["successor_authorized"], false);
    let target = temp.path().canonicalize()?.join("instance");
    assert!(super::super::load(&b, &target).is_err());
    assert!(!target.exists());
    Ok(())
}

#[test]
fn legacy_requirements_remain_absent_and_no_lineage_is_inferred() -> Result<()> {
    let temp = TempDir::new("native-release-legacy");
    let a = package(temp.path(), "a", "an-native-spool-v1")?;
    let b = package(temp.path(), "b", "an-native-spool-v2")?;
    let result = compare_releases(&a, &b)?;
    assert!(result["previous"]["manifest"].get("requirements").is_none());
    assert_eq!(result["previous"]["legacy_implicit_kit_editability"], true);
    assert_eq!(
        result["candidate"]["legacy_implicit_kit_editability"],
        false
    );
    assert_eq!(
        result["changes"]["requirements"].as_array().unwrap().len(),
        9
    );
    assert_eq!(result["domain_compatibility"], "not_evaluated");
    assert_eq!(result["state_migration"], "not_evaluated");
    Ok(())
}

#[test]
fn integrity_and_schema_checks_are_required_even_for_inspection() -> Result<()> {
    let temp = TempDir::new("native-release-refusals");
    let a = package(temp.path(), "a", "an-native-spool-v1")?;
    let b = package(temp.path(), "b", "an-native-spool-v1")?;
    fs::write(b.join("extra.txt"), "unlisted")?;
    assert!(compare_releases(&a, &b).is_err());
    fs::remove_file(b.join("extra.txt"))?;
    fs::write(b.join("kit/edit.sh"), "tampered")?;
    assert!(compare_releases(&a, &b).is_err());
    fs::write(b.join("kit/edit.sh"), "exit 0\n")?;
    edit(&b, |m| m["schema"] = json!("an-native-spool-v2"))?;
    assert!(compare_releases(&a, &b).is_err());
    edit(&b, |m| {
        m["schema"] = json!("an-native-spool-v1");
        m["grant"] = json!(true);
    })?;
    assert!(compare_releases(&a, &b).is_err());
    Ok(())
}
