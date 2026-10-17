use std::{
    env, fs,
    io::{self, Read},
    net::TcpListener,
    process::Command,
    thread,
    time::Duration,
};

// Test-only hostile operations. Never packaged as the accepted example kit.
fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    match input.trim() {
        "write" => assert!(fs::write("forbidden.txt", "must not land").is_err()),
        "network" => assert!(TcpListener::bind("127.0.0.1:0").is_err()),
        "spawn" => assert!(Command::new("/usr/bin/true").status().is_err()),
        "env" => assert!(env::var("WAVER_SENTINEL").is_err()),
        "hang" => loop {
            thread::sleep(Duration::from_millis(10));
        },
        "flood" => loop {
            println!("{}", "x".repeat(4096));
        },
        secret => assert!(fs::read(secret).is_err()),
    }
    println!("denied as expected");
}
