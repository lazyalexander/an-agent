//! Negative acceptance tests: a green checker must not silently skip its base.
use std::{fs, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().to_owned()
}
fn fixture() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("crates/core/src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers=['crates/core']\nresolver='2'\n",
    )
    .unwrap();
    fs::write(
        root.join("crates/core/Cargo.toml"),
        "[package]\nname='an-agent-core'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(root.join("crates/core/src/lib.rs"), "pub fn safe() {}\n").unwrap();
    let upstream = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for file in ["ARCHITECTURE.toml", "architecture-policy.toml"] {
        fs::copy(upstream.join(file), root.join(file)).unwrap();
    }
    fs::write(
        root.join("architecture-baseline.toml"),
        "[file_size]\n[pattern_ratchet.command_construction]\n[crate_names]\n",
    )
    .unwrap();
    git(root, &["init", "--quiet"]);
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=Architecture Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "baseline",
        ],
    );
    let base = git(root, &["rev-parse", "HEAD"]);
    (dir, base)
}
fn gate(root: &Path, base: &str) -> (bool, String) {
    let result = Command::new(env!("CARGO_BIN_EXE_an-agent-architecture"))
        .args(["--root", root.to_str().unwrap(), "--base", base])
        .output()
        .unwrap();
    (
        result.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        ),
    )
}
#[test]
fn unchanged_tree_passes_and_missing_base_fails() {
    let (dir, base) = fixture();
    let (ok, log) = gate(dir.path(), &base);
    assert!(ok, "{log}");
    assert!(!gate(dir.path(), "does-not-exist").0);
    fs::remove_file(dir.path().join("architecture-baseline.toml")).unwrap();
    assert!(!gate(dir.path(), &base).0);
}

#[test]
fn first_main_promotion_keeps_committed_policy_and_debt_caps() {
    // Local object sharing needs no network and leaves the source checkout alone.
    // CI fetches full history; missing pinned history intentionally fails closed.
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("checkout");
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let head = git(&source, &["rev-parse", "HEAD"]);
    let common = source
        .join(git(&source, &["rev-parse", "--git-common-dir"]))
        .canonicalize()
        .unwrap();
    let cloned = Command::new("git")
        .args(["clone", "--quiet", "--shared", "--no-checkout"])
        .arg(common)
        .arg(&root)
        .output()
        .unwrap();
    assert!(
        cloned.status.success(),
        "{}",
        String::from_utf8_lossy(&cloned.stderr)
    );
    git(&root, &["checkout", "--quiet", "--detach", &head]);
    let base = "a3ff193c7d9732f2c272125e9b9c925bc9096567";
    let (ok, log) = gate(&root, base);
    assert!(ok && log.contains("first main promotion"), "{log}");

    let rules = root.join("ARCHITECTURE.toml");
    let original = fs::read_to_string(&rules).unwrap();
    fs::write(&rules, original.replace("limit = 500", "limit = 5000")).unwrap();
    let (ok, log) = gate(&root, base);
    assert!(!ok && log.contains("policy changed"), "{log}");
    fs::write(&rules, original).unwrap();

    let baseline = root.join("architecture-baseline.toml");
    let original = fs::read_to_string(&baseline).unwrap();
    fs::write(&baseline, original.replace("= 624", "= 625")).unwrap();
    let (ok, log) = gate(&root, base);
    assert!(!ok && log.contains("no_raised_caps"), "{log}");
    fs::write(&baseline, original).unwrap();

    fs::write(
        root.join("crates/an-agent-host/src/oversized.rs"),
        "pub fn item() {}\n".repeat(501),
    )
    .unwrap();
    let (ok, log) = gate(&root, base);
    assert!(!ok && log.contains("no_crossing"), "{log}");
}
#[test]
fn growth_and_baseline_laundering_fail() {
    let (dir, base) = fixture();
    fs::write(
        dir.path().join("crates/core/src/lib.rs"),
        "pub fn item() {}\n".repeat(501),
    )
    .unwrap();
    let (ok, log) = gate(dir.path(), &base);
    assert!(!ok && log.contains("file_size.no_crossing"), "{log}");
    fs::write(dir.path().join("architecture-baseline.toml"), "[file_size]\n\"crates/core/src/lib.rs\"=501\n[pattern_ratchet.command_construction]\n[crate_names]\n").unwrap();
    let (ok, log) = gate(dir.path(), &base);
    assert!(!ok && log.contains("no_new_entries"), "{log}");
}
#[test]
fn new_command_and_policy_weakening_fail() {
    let (dir, base) = fixture();
    fs::write(
        dir.path().join("crates/core/src/lib.rs"),
        "fn bad() { std::process::Command::new(\"sh\"); }\n",
    )
    .unwrap();
    let (ok, log) = gate(dir.path(), &base);
    assert!(
        !ok && log.contains("command_construction.no_crossing"),
        "{log}"
    );
    let path = dir.path().join("ARCHITECTURE.toml");
    fs::write(
        &path,
        fs::read_to_string(&path)
            .unwrap()
            .replace("limit = 500", "limit = 5000"),
    )
    .unwrap();
    assert!(gate(dir.path(), &base).1.contains("policy changed"));
}
#[test]
fn invalid_base_is_not_treated_as_an_empty_baseline() {
    let (dir, _) = fixture();
    fs::write(
        dir.path().join("architecture-baseline.toml"),
        "not valid TOML",
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(
        dir.path(),
        &[
            "-c",
            "user.name=Architecture Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "invalid",
        ],
    );
    let base = git(dir.path(), &["rev-parse", "HEAD"]);
    fs::write(
        dir.path().join("architecture-baseline.toml"),
        "[file_size]\n[crate_names]\n",
    )
    .unwrap();
    assert!(!gate(dir.path(), &base).0);
}

#[test]
fn raised_existing_cap_and_stale_cap_fail() {
    let (dir, _) = fixture();
    let source = dir.path().join("crates/core/src/lib.rs");
    let baseline = dir.path().join("architecture-baseline.toml");
    fs::write(&source, "pub fn item() {}\n".repeat(501)).unwrap();
    fs::write(&baseline, "[file_size]\n\"crates/core/src/lib.rs\"=501\n[pattern_ratchet.command_construction]\n[crate_names]\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(
        dir.path(),
        &[
            "-c",
            "user.name=Architecture Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "existing debt",
        ],
    );
    let base = git(dir.path(), &["rev-parse", "HEAD"]);
    assert!(gate(dir.path(), &base).0);
    fs::write(&source, "pub fn item() {}\n".repeat(502)).unwrap();
    fs::write(
        &baseline,
        fs::read_to_string(&baseline).unwrap().replace("501", "502"),
    )
    .unwrap();
    assert!(gate(dir.path(), &base).1.contains("no_raised_caps"));
    fs::write(&source, "pub fn item() {}\n").unwrap();
    assert!(gate(dir.path(), &base).1.contains("stale_entries"));
}

#[test]
fn real_metadata_detects_core_to_host_dependency() {
    let (dir, base) = fixture();
    fs::create_dir_all(dir.path().join("crates/host/src")).unwrap();
    fs::write(
        dir.path().join("crates/host/Cargo.toml"),
        "[package]\nname='an-agent-host'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(dir.path().join("crates/host/src/lib.rs"), "").unwrap();
    let manifest = dir.path().join("crates/core/Cargo.toml");
    fs::write(
        &manifest,
        format!(
            "{}\n[dependencies]\nrenamed={{package='an-agent-host',path='../host'}}\n",
            fs::read_to_string(&manifest).unwrap()
        ),
    )
    .unwrap();
    let (ok, log) = gate(dir.path(), &base);
    assert!(!ok && log.contains("forbidden dependency path"), "{log}");
}
