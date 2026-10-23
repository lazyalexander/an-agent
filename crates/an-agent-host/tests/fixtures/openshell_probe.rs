//! Linux ARM64 executable used only by the opt-in VM lifecycle test.
use std::{fs, process::Command, thread, time::Duration};
fn main() {
    if std::env::args().nth(1).as_deref() == Some("late") {
        thread::sleep(Duration::from_secs(8));
        fs::write("late.txt", "must not be accepted").unwrap();
        return;
    }
    let input = fs::read_to_string("input.txt").unwrap();
    fs::write("output.txt", "early untrusted candidate").unwrap();
    let _child = Command::new("/sandbox/program").arg("late").spawn().unwrap();
    if input.trim() == "hang" {
        thread::sleep(Duration::from_secs(120));
    }
}
