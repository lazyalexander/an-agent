//! Fail-closed effective policy and same-descriptor guest candidate collection.
use crate::execution::Result;
use serde_json::{Value, json};

pub(super) const COLLECTOR: &str = r#"import os, stat, sys
fd = os.open(sys.argv[1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
try:
    before = os.fstat(fd)
    if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_size > 65536:
        raise RuntimeError('candidate is not a bounded singly linked regular file')
    chunks, size = [], 0
    while size <= 65536:
        chunk = os.read(fd, 65537 - size)
        if not chunk:
            break
        chunks.append(chunk)
        size += len(chunk)
    after = os.fstat(fd)
    if size > 65536 or after.st_nlink != 1 or before.st_size != after.st_size or size != after.st_size:
        raise RuntimeError('candidate changed or exceeded its limit')
    sys.stdout.buffer.write(b''.join(chunks))
finally:
    os.close(fd)
"#;

pub(super) fn validate_policy(policy: &Value) -> Result<()> {
    let filesystem = json!({"include_workdir":true,
        "read_only":["/usr","/lib","/lib64","/etc","/proc","/dev/urandom"],
        "read_write":["/tmp","/dev/null"]});
    if policy["landlock"]["compatibility"] != "hard_requirement"
        || !policy["network_policies"]
            .as_object()
            .is_some_and(|rules| rules.is_empty())
        || policy["filesystem_policy"] != filesystem
    {
        return Err("sandbox policy does not match the offline file-kit boundary".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_malformed_network_or_changed_filesystem_is_refused() {
        let valid = json!({"landlock":{"compatibility":"hard_requirement"},
            "network_policies":{},"filesystem_policy":{"include_workdir":true,
            "read_only":["/usr","/lib","/lib64","/etc","/proc","/dev/urandom"],
            "read_write":["/tmp","/dev/null"]}});
        validate_policy(&valid).unwrap();
        for value in [Value::Null, json!([]), json!(""), json!({"egress":{}})] {
            let mut changed = valid.clone();
            changed["network_policies"] = value;
            assert!(validate_policy(&changed).is_err());
        }
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove("network_policies");
        assert!(validate_policy(&missing).is_err());
        let mut changed = valid;
        changed["filesystem_policy"]["read_write"] = json!(["/"]);
        assert!(validate_policy(&changed).is_err());
    }
}

#[cfg(all(test, unix))]
#[path = "custody_tests.rs"]
mod collector_tests;
