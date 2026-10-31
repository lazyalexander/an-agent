//! Profile compatibility and admission of explicit snapshot layouts.
use super::*;
use an_agent_core::testkit::TempDir;
use std::fs;

#[test]
fn layout_is_optional_and_explicit_layouts_cannot_alias() -> Result<()> {
    let temp = TempDir::new("workspace-profile-layout");
    let base = temp.path().canonicalize()?;
    fs::create_dir(base.join("a"))?;
    fs::create_dir(base.join("b"))?;
    fs::write(base.join("a/main.sh"), b"exit 0\n")?;
    fs::write(base.join("b/main.sh"), b"exit 0\n")?;
    let mut p = json!({"schema":"an-workspace-job-v2","executable":"/bin/echo",
        "args":["{input:0}"],"environment":{},"readonly":[],
        "inputs":[base.join("a/main.sh")],"outputs":["result.txt"],
        "wall_seconds":1,"trusted_process_group":true});
    let plan = |p: &Value| Profile::plan(&serde_json::to_vec(p)?, &base.join("job"), &Value::Null);
    assert_eq!(plan(&p)?.1["inputs"][0]["name"], "main.sh");
    p["inputs"] = json!([base.join("a/main.sh"), base.join("b/main.sh")]);
    assert!(plan(&p).is_err());
    p["input_layout"] = json!(["silk/main.sh", "kit/main.sh"]);
    let accepted = plan(&p)?.1;
    assert_eq!(accepted["inputs"][0]["name"], "silk/main.sh");
    assert_eq!(accepted["inputs"][1]["name"], "kit/main.sh");
    for layout in [
        json!(["only-one"]),
        json!(["a", "a/b"]),
        json!(["a", "../escape"]),
        json!(["a", "a"]),
    ] {
        p["input_layout"] = layout;
        assert!(plan(&p).is_err());
    }
    Ok(())
}
