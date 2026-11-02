//! Registered-output custody is deliberately separate from ordinary revision reads.
use super::{Path, Result, Value};
use an_agent_host::native_spool::managed_output::{self, Operation};
pub(super) fn dispatch(args: &[String]) -> Result<Value> {
    let (args, approved) = match args {
        [rest @ .., flag, hash] if flag == "--approve" => (rest, Some(hash.as_str())),
        _ => (args, None),
    };
    let [op, root, run, output, revision, rest @ ..] = args else {
        return Err("expected operation, instance, run, output and exact revision".into());
    };
    let operation = match (op.as_str(), rest) {
        ("show", []) => Operation::Show,
        ("close", []) => Operation::Close,
        ("delete", []) => Operation::Delete,
        ("checkout", [name, dest]) => Operation::Checkout(name, Path::new(dest)),
        ("release", [name]) => Operation::Release(name),
        _ => return Err("invalid registered-output command".into()),
    };
    managed_output::manage(Path::new(root), run, output, revision, operation, approved)
}
