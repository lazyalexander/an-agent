use super::*;
use serde_json::{Value, json};
fn manifest() -> Value {
    json!({"schema":"an-native-spool-v2","name":"example","version":"0.1.0",
        "entry":"silk/main.sh","files":{"silk/main.sh":"a".repeat(64),"kit/edit.sh":"b".repeat(64)},"outputs":["report.txt"],
        "requirements":{"runtime":"macos-native-process-v1","os":"macos","arch":"portable-source",
        "interaction":"headless","gpu":"none","inputs":["input.txt"],"editable":["kit/edit.sh"],
        "capabilities":["read-input","write-declared-outputs"],"dependencies":[]}})
}
#[test]
fn schemas_and_unqualified_requirements_refuse_without_inventing_defaults() -> Result<()> {
    let value = manifest();
    let parse = |v: Value| -> Result<()> { validate(&serde_json::from_value(v)?) };
    assert_eq!(parse(value.clone()).is_ok(), cfg!(target_os = "macos"));
    for (key, value2) in [
        ("os", json!("linux")),
        ("gpu", json!("required")),
        ("capabilities", json!(["network"])),
        ("editable", json!(["silk/main.sh"])),
        ("dependencies", json!(["python"])),
        ("inputs", json!([])),
    ] {
        let mut v = value.clone();
        v["requirements"][key] = value2;
        assert!(parse(v).is_err());
    }
    let mut missing = value.clone();
    missing.as_object_mut().unwrap().remove("requirements");
    assert!(parse(missing.clone()).is_err());
    missing["schema"] = json!("an-native-spool-v1");
    assert!(parse(missing).is_ok());
    let mut wrong = value;
    wrong["schema"] = json!("an-native-spool-v1");
    assert!(parse(wrong).is_err());
    Ok(())
}
#[test]
fn editable_subset_is_independent_of_payload_presence() -> Result<()> {
    let m: bundle::Manifest = serde_json::from_value(manifest())?;
    assert!(editable(&m, "kit/edit.sh").is_ok());
    assert!(editable(&m, "silk/main.sh").is_err());
    Ok(())
}
