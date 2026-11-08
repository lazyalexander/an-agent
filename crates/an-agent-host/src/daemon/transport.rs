//! Same-user Unix socket; bounded messages and one flock-owned service instance.
use super::*;
#[cfg_attr(not(target_os = "macos"), allow(unused_imports))]
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{FileTypeExt, OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    sync::atomic::Ordering,
    time::Duration,
};

const LIMIT: u64 = 131072;

pub(super) fn request(root: &Path, value: &Value, scoped: bool) -> Result<Value> {
    let root = storage::private(root)?;
    let mut socket =
        UnixStream::connect(root.join(if scoped { "tasks.sock" } else { "control.sock" }))?;
    socket.set_read_timeout(Some(Duration::from_secs(15)))?;
    socket.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut bytes = serde_json::to_vec(value)?;
    if bytes.len() as u64 > LIMIT {
        return Err("daemon request exceeds 128 KiB".into());
    }
    bytes.push(b'\n');
    socket.write_all(&bytes)?;
    let mut bytes = Vec::new();
    BufReader::new(socket)
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("daemon response exceeds 4 MiB".into());
    }
    let response: Value = serde_json::from_slice(&bytes)?;
    if let Some(error) = response["error"].as_str() {
        return Err(error.to_owned().into());
    }
    Ok(response["result"].clone())
}

#[cfg(target_os = "macos")]
pub(super) fn serve(root: &Path, cancelled: &AtomicBool) -> Result<Value> {
    let root = storage::private(root)?;
    storage::config(&root)?;
    let lock_path = root.join("service.lock");
    store::no_links(&lock_path)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(lock_path)?;
    if unsafe {
        libc::flock(
            std::os::fd::AsRawFd::as_raw_fd(&lock),
            libc::LOCK_EX | libc::LOCK_NB,
        )
    } != 0
    {
        return Err("another daemon already owns this directory".into());
    }
    let listener = listen(&root, "control.sock")?;
    let task_listener = listen(&root, "tasks.sock")?;
    let mut web = web::Server::start(&root)?;
    let mut children = BTreeMap::new();
    let mut settled = BTreeSet::new();
    let mut ticks = 0u8;
    while !cancelled.load(Ordering::Acquire) {
        // Both endpoints share this service, queue and authoritative owners.
        for (listener, scoped) in [(&listener, false), (&task_listener, true)] {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false)?;
                    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
                    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
                    let response = handle(&root, &mut stream, scoped);
                    let value = match response {
                        Ok(value) => json!({"result":value}),
                        Err(e) => json!({"error":e.to_string()}),
                    };
                    let _ = stream.write_all(&serde_json::to_vec(&value)?);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(error) => return Err(error.into()),
            }
        }
        ticks = (ticks + 1) % 8;
        if ticks == 0 {
            scheduler::tick(&root, &mut children, &mut settled)?;
        }
        web.poll(&root)?;
        std::thread::sleep(Duration::from_millis(25));
    }
    // Finite owners retain their deadlines and receipts across service shutdown.
    fs::remove_file(root.join("control.sock"))?;
    fs::remove_file(root.join("tasks.sock"))?;
    fs::remove_file(root.join("endpoint.json"))?;
    drop(lock);
    Ok(json!({"stopped":true,"active_work":"original supervisors retain ownership; no replay"}))
}

#[cfg(target_os = "macos")]
fn listen(root: &Path, name: &str) -> Result<UnixListener> {
    let path = root.join(name);
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if !metadata.file_type().is_socket() {
            return Err("control path is not a socket".into());
        }
        fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

#[cfg(target_os = "macos")]
fn handle(root: &Path, socket: &mut UnixStream, scoped: bool) -> Result<Value> {
    let mut uid = 0;
    let mut gid = 0;
    if unsafe { libc::getpeereid(std::os::fd::AsRawFd::as_raw_fd(socket), &mut uid, &mut gid) } != 0
        || uid != unsafe { libc::geteuid() }
    {
        return Err("daemon accepts only the current user".into());
    }
    let mut bytes = Vec::new();
    BufReader::new(socket)
        .take(LIMIT + 1)
        .read_until(b'\n', &mut bytes)?;
    if bytes.len() as u64 > LIMIT || bytes.last() != Some(&b'\n') {
        return Err("invalid or oversized daemon request".into());
    }
    let request = serde_json::from_slice(&bytes)?;
    if scoped {
        permissions::dispatch(root, &request)
    } else {
        control::dispatch(root, &request)
    }
}
