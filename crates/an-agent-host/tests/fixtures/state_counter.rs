//! Deliberately small domain kit: increment a decimal counter, not host logic.
//! Its parser accepts only the host's compact envelope with a decimal state.
use std::{fs, io};

fn main() -> io::Result<()> {
    let input = fs::read_to_string("input.txt")?;
    assert!(fs::read_dir("HOST_INSTANCES").is_err());
    assert!(fs::read("HOST_SECRET").is_err());
    assert!(fs::write("../receipt.json", b"forged").is_err());
    assert!(fs::write("input.txt", b"overwritten").is_err());
    assert!(std::process::Command::new("/usr/bin/true").status().is_err());
    let state = input.split_once("\"state\":\"").and_then(|(_, tail)| tail.split_once('"'))
        .map(|(state, _)| state).ok_or_else(|| io::Error::other("missing decimal state"))?;
    let counter = state.parse::<u64>().map_err(io::Error::other)?;
    let next = counter.checked_add(1).ok_or_else(|| io::Error::other("counter overflow"))?;
    let result = format!(r#"{{"schema":"an-instance-bundle-v1","state":"{next}","product":"count={next}","report":"incremented"}}"#);
    fs::write("output.txt", result)?;
    Ok(())
}
