//! Actual local execution of the trusted single-descriptor collector.

use super::*;
use std::{
    fs,
    os::unix::fs::symlink,
    process::{Command, Output},
};
#[test]
fn actual_collector_reads_regular_files_and_refuses_links_and_oversize() -> Result<()> {
    let temp = an_agent_core::testkit::TempDir::new("openshell-collector");
    let path = temp.path().join("output.txt");
    let invoke = || -> std::io::Result<Output> {
        Command::new("/usr/bin/python3")
            .args(["-I", "-c", COLLECTOR])
            .arg(&path)
            .output()
    };
    fs::write(&path, b"candidate")?;
    let output = invoke()?;
    assert!(output.status.success());
    assert_eq!(output.stdout, b"candidate");
    fs::rename(&path, temp.path().join("original"))?;
    symlink(temp.path().join("original"), &path)?;
    assert!(!invoke()?.status.success());
    fs::remove_file(&path)?;
    fs::hard_link(temp.path().join("original"), &path)?;
    assert!(!invoke()?.status.success());
    fs::remove_file(&path)?;
    fs::write(&path, vec![b'x'; 65537])?;
    assert!(!invoke()?.status.success());
    fs::write(&path, b"")?;
    assert!(invoke()?.status.success());
    Ok(())
}
