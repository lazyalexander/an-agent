//! Read-only loopback console. A private fragment token gates every data request.
use super::*;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    time::Duration,
};

pub(super) struct Server {
    listener: TcpListener,
    token: String,
    host: String,
}
impl Server {
    pub(super) fn start(root: &Path) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let host = listener.local_addr()?.to_string();
        let token = an_agent_core::det_seam::Entropy::os().uuid_v4().to_string();
        let endpoint = root.join("endpoint.json");
        if endpoint.exists() {
            fs::remove_file(&endpoint)?;
        }
        write(
            &endpoint,
            &json!({"url":format!("http://{host}/#{token}"),"pid":std::process::id()}),
        )?;
        Ok(Self {
            listener,
            token,
            host,
        })
    }
    pub(super) fn poll(&mut self, root: &Path) -> Result<()> {
        match self.listener.accept() {
            Ok((mut socket, _)) => {
                let _ = self.respond(root, &mut socket);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }
    fn respond(&self, root: &Path, socket: &mut TcpStream) -> Result<()> {
        socket.set_nonblocking(false)?;
        socket.set_read_timeout(Some(Duration::from_millis(200)))?;
        socket.set_write_timeout(Some(Duration::from_millis(500)))?;
        let mut bytes = Vec::new();
        let mut buf = [0u8; 1024];
        while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = socket.read(&mut buf)?;
            if n == 0 || bytes.len() + n > 8192 {
                return Err("invalid HTTP headers".into());
            }
            bytes.extend_from_slice(&buf[..n]);
        }
        let head = String::from_utf8(bytes)?;
        let first = head.lines().next().ok_or("HTTP request")?;
        let headers: Vec<_> = head
            .lines()
            .skip(1)
            .filter_map(|s| s.split_once(':'))
            .collect();
        let header = |name: &str| {
            headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.trim())
        };
        let (status, mime, body) = if header("host") != Some(self.host.as_str()) {
            (403, "text/plain", "Invalid host".into())
        } else if first == "GET / HTTP/1.1" {
            (
                200,
                "text/html; charset=utf-8",
                include_str!("console.html").into(),
            )
        } else if first == "GET /app.js HTTP/1.1" {
            (
                200,
                "text/javascript; charset=utf-8",
                include_str!("console.js").into(),
            )
        } else if first == "GET /snapshot HTTP/1.1"
            && header("authorization") == Some(format!("Bearer {}", self.token).as_str())
        {
            (
                200,
                "application/json",
                serde_json::to_string(&observation::snapshot(root)?)?,
            )
        } else if let Some(id) = first
            .strip_prefix("GET /activity/")
            .and_then(|s| s.strip_suffix(" HTTP/1.1"))
            && header("authorization") == Some(format!("Bearer {}", self.token).as_str())
        {
            match activity::view(root, id) {
                Ok(value) => (200, "application/json", serde_json::to_string(&value)?),
                Err(_) => (403, "text/plain", "Activity unavailable".into()),
            }
        } else {
            (
                403,
                "text/plain",
                "Read-only console; authorization required".into(),
            )
        };
        write!(
            socket,
            "HTTP/1.1 {status} {}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'self'; style-src 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'\r\n\r\n{body}",
            if status == 200 { "OK" } else { "Forbidden" },
            body.len()
        )?;
        Ok(())
    }
}
