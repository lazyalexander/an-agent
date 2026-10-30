//! Bounded, deterministic installed-package inventory; no imports or hook execution.
use super::*;
use std::collections::BTreeMap;

pub(super) fn digest(root: &Path) -> Result<String> {
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    let mut entries = 0usize;
    walk(root, root, &mut files, &mut total, &mut entries, 0)?;
    Ok(store::digest(&serde_json::to_vec(&files)?))
}
fn walk(
    root: &Path,
    dir: &Path,
    files: &mut BTreeMap<String, String>,
    total: &mut usize,
    entries: &mut usize,
    depth: usize,
) -> Result<()> {
    if depth > 16 {
        return Err("Python dependency inventory is too deep".into());
    }
    for entry in fs::read_dir(dir)? {
        *entries += 1;
        if *entries > 8192 {
            return Err("Python dependency directory is too large".into());
        }
        let path = entry?.path();
        store::no_links(&path)?;
        if path.is_dir() {
            walk(root, &path, files, total, entries, depth + 1)?;
        } else {
            if files.len() >= 8192 {
                return Err("too many Python dependency files".into());
            }
            let bytes = regular(&path, 20 * 1024 * 1024)?;
            *total += bytes.len();
            if *total > 256 * 1024 * 1024 {
                return Err("Python dependency inventory exceeds 256 MiB".into());
            }
            let name = path
                .strip_prefix(root)?
                .to_str()
                .ok_or("non-UTF-8 dependency path")?
                .to_owned();
            files.insert(name, store::digest(&bytes));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_pins_nested_bytes_and_names_and_refuses_links() -> Result<()> {
        let temp = an_agent_core::testkit::TempDir::new("native-python-inventory");
        let root = temp.path().canonicalize()?;
        assert_eq!(digest(&root)?, store::digest(b"{}"));
        fs::create_dir(root.join("module"))?;
        fs::write(root.join("module/a.py"), b"value = 1")?;
        let before = digest(&root)?;
        fs::rename(root.join("module/a.py"), root.join("module/b.py"))?;
        let renamed = digest(&root)?;
        assert_ne!(renamed, before);
        fs::write(root.join("module/b.py"), b"value = 2")?;
        let changed = digest(&root)?;
        assert_ne!(changed, renamed);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("module/b.py"), root.join("alias"))?;
            assert!(digest(&root).is_err());
        }
        Ok(())
    }
}
