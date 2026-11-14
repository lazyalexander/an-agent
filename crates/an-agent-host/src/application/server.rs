//! Authenticated product UI. Expensive operations never occupy the daemon lane.
use crate::execution::{Result, store};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

pub fn serve(service: &Path, stopped: &AtomicBool) -> Result<Value> {
    // The original daemon validates service ownership and executable binding.
    crate::daemon::request(service, &json!({"op":"show"}))?;
    let service = service.canonicalize()?;
    let root = service.join("application");
    store::directory(&root)?;
    for name in ["uploads", "environments", "tasks"] {
        store::directory(&root.join(name))?;
    }
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let authority = listener.local_addr()?.to_string();
    let token = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
    let endpoint = root.join("endpoint.json");
    if endpoint.exists() {
        fs::remove_file(&endpoint)?;
    }
    store::write_new(
        &endpoint,
        &serde_json::to_vec(&json!({"url":format!("http://{authority}/#{token}")}))?,
    )?;
    let active = Arc::new(AtomicUsize::new(0));
    while !stopped.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if active.fetch_add(1, Ordering::AcqRel) >= 8 {
                    active.fetch_sub(1, Ordering::AcqRel);
                    continue;
                }
                let (root, service, authority, token, active) = (
                    root.clone(),
                    service.clone(),
                    authority.clone(),
                    token.clone(),
                    Arc::clone(&active),
                );
                std::thread::spawn(move || {
                    let result = handle(&mut stream, &root, &service, &authority, &token);
                    if let Err(error) = result {
                        let _ = respond(
                            &mut stream,
                            400,
                            "application/json",
                            serde_json::to_vec(&json!({"error":error.to_string()}))
                                .unwrap_or_default(),
                        );
                    }
                    active.fetch_sub(1, Ordering::AcqRel);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(json!({"stopped":true}))
}
fn handle(
    stream: &mut TcpStream,
    root: &Path,
    service: &Path,
    authority: &str,
    token: &str,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut input = BufReader::new(stream.try_clone()?);
    let mut head = String::new();
    loop {
        let mut line = String::new();
        let count = (&mut input)
            .take(8193 - head.len() as u64)
            .read_line(&mut line)?;
        if count == 0 || head.len() + count > 8192 {
            return Err("invalid HTTP headers".into());
        }
        head.push_str(&line);
        if line == "\r\n" {
            break;
        }
    }
    let mut lines = head.lines();
    let request = lines.next().ok_or("request")?;
    let mut headers = std::collections::BTreeMap::new();
    for line in lines.filter(|s| !s.is_empty()) {
        let (key, value) = line.split_once(':').ok_or("header")?;
        if headers
            .insert(key.to_ascii_lowercase(), value.trim())
            .is_some()
        {
            return Err("duplicate HTTP header".into());
        }
    }
    if headers.get("host") != Some(&authority) {
        return Err("invalid host".into());
    }
    match request {
        "GET / HTTP/1.1" => {
            return respond(
                stream,
                200,
                "text/html; charset=utf-8",
                include_bytes!("index.html").to_vec(),
            );
        }
        "GET /app.js HTTP/1.1" => {
            return respond(
                stream,
                200,
                "text/javascript",
                include_bytes!("app.js").to_vec(),
            );
        }
        "GET /strings.json HTTP/1.1" => {
            return respond(
                stream,
                200,
                "application/json",
                include_bytes!("strings.json").to_vec(),
            );
        }
        _ => (),
    }
    if headers.get("authorization") != Some(&format!("Bearer {token}").as_str())
        || headers.contains_key("transfer-encoding")
    {
        return Err("authorization required".into());
    }
    if request == "GET /view HTTP/1.1" {
        return respond(
            stream,
            200,
            "application/json",
            serde_json::to_vec(&view(root, service)?)?,
        );
    }
    if headers.get("origin") != Some(&format!("http://{authority}").as_str()) {
        return Err("same-origin request required".into());
    }
    let size = headers
        .get("content-length")
        .ok_or("content length")?
        .parse::<u64>()?;
    if request == "POST /upload HTTP/1.1" {
        if size == 0 || size > 8 * 1024 * 1024 * 1024 {
            return Err("upload size limit".into());
        }
        let id = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
        let path = root.join("uploads").join(&id);
        let result = (|| -> Result<()> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            if std::io::copy(&mut input.take(size), &mut file)? != size {
                return Err("incomplete upload".into());
            }
            file.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(path);
        }
        result?;
        return respond(
            stream,
            200,
            "application/json",
            serde_json::to_vec(&json!({"upload":id}))?,
        );
    }
    if request != "POST /action HTTP/1.1" || size > 131072 {
        return Err("unsupported product request".into());
    }
    let mut bytes = Vec::new();
    input.take(size).read_to_end(&mut bytes)?;
    let action: Value = serde_json::from_slice(&bytes)?;
    let value = act(root, service, &action)?;
    respond(stream, 200, "application/json", serde_json::to_vec(&value)?)
}
fn selected(root: &Path, value: &Value, key: &str) -> Result<PathBuf> {
    let id = value[key].as_str().ok_or("selected upload")?;
    if uuid::Uuid::parse_str(id)?.to_string() != id {
        return Err("invalid upload identity".into());
    }
    Ok(root.join("uploads").join(id))
}
fn act(root: &Path, _service: &Path, value: &Value) -> Result<Value> {
    match value["op"].as_str() {
        Some("environment-preview") => {
            super::environment::preview(&selected(root, value, "upload")?)
        }
        Some("environment-import") => super::environment::import(
            &root.join("environments"),
            &selected(root, value, "upload")?,
            value["approve"].as_str().ok_or("approval")?,
        ),
        _ => Err("unsupported product action".into()),
    }
}
fn view(root: &Path, service: &Path) -> Result<Value> {
    let environments = fs::read_dir(root.join("environments"))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let id = entry.file_name().to_str()?.to_owned();
            if !store::valid_digest(&id) {
                return None;
            }
            let bytes = store::read(&entry.path().join("runtime.json"), 2 * 1024 * 1024).ok()?;
            let manifest: Value = serde_json::from_slice(&bytes).ok()?;
            Some(json!({"id":id,"manifest":manifest}))
        })
        .collect::<Vec<_>>();
    Ok(
        json!({"environments":environments,"host":crate::daemon::request(service,&json!({"op":"show"}))?}),
    )
}
fn respond(stream: &mut TcpStream, status: u16, mime: &str, body: Vec<u8>) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status} {}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'self'; style-src 'unsafe-inline'; frame-ancestors 'none'\r\n\r\n",
        if status == 200 { "OK" } else { "Bad Request" },
        body.len()
    )?;
    stream.write_all(&body)?;
    Ok(())
}
