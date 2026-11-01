//! Resolve a registered output only through the original native acceptance.
use super::*;
use crate::artifact_versions::{self, registered};

pub use registered::Operation;

pub fn manage(
    root: &Path,
    run_name: &str,
    output: &str,
    revision: &str,
    op: Operation<'_>,
    approved: Option<&str>,
) -> Result<Value> {
    let root = location(root)?;
    let report = artifacts::selected_metadata(&root, run_name, output)?;
    let selected = &report["output"]["binding"];
    if selected["revision"] != revision || selected["reader_profile"] != registered::PROFILE {
        return Err("requires exact version enrolled at first publication".into());
    }
    let catalog = Path::new(report["catalog"].as_str().ok_or("catalog")?);
    let binding = artifact_versions::metadata(catalog, revision)?;
    let owner = json!({"root":root,"instance":report["instance"],"release":report["release"],"run":run_name,"output":output});
    match &op {
        Operation::Show if approved.is_some() => return Err("inspection takes no approval".into()),
        Operation::Checkout(name, _) | Operation::Release(name) => bundle::identifier(name)?,
        _ => {}
    };
    registered::operate(catalog, &binding, owner, op, approved)
}
