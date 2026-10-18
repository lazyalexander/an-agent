use std::{fs, io::{self, Read}, net::TcpListener, process::Command, thread, time::Duration};

fn main() {
    let mut input=String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    match input.trim() {
        "write" => {
            for path in ["/forbidden.txt","/dev/shm/forbidden.txt","/dev/forbidden.txt","/program"] {
                assert!(fs::write(path,b"forbidden").is_err(),"write allowed: {path}");
            }
            for path in ["/program", "/image-info.txt", "/etc/hosts", "/etc/hostname"] {
                assert!(fs::OpenOptions::new().write(true).open(path).is_err(),"existing file writable: {path}");
            }
        },
        "network" => assert!(TcpListener::bind("127.0.0.1:0").is_err()),
        "spawn" => assert!(Command::new("/program").spawn().is_err()),
        "env" => {
            let mut vars=std::env::vars().collect::<Vec<_>>(); vars.sort();
            assert_eq!(vars,vec![("HOME".into(),"/nonexistent".into()),("HOSTNAME".into(),"kit".into()),("PATH".into(),"/nonexistent".into())]);
        },
        "socket" => assert!(fs::metadata("/var/run/docker.sock").is_err()),
        "violate" => fs::write("/forbidden.txt",b"bad").unwrap(),
        "hang" => loop {thread::sleep(Duration::from_millis(10));},
        "flood" => loop {println!("{}","x".repeat(4096));},
        path if path.starts_with('/') => assert!(fs::read(path).is_err()),
        _ => { print!("{}",input.trim()); return; }
    }
    println!("denied as expected");
}
