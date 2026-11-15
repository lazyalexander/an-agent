use super::*;

#[test]
fn accepted_socket_waits_for_delayed_http_headers() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let (ready, wait) = std::sync::mpsc::channel();
    let client = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut stream = TcpStream::connect(address)?;
        wait.recv().unwrap();
        std::thread::sleep(Duration::from_millis(50));
        write!(stream, "GET / HTTP/1.1\r\nHost: {address}\r\n\r\n")?;
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes)?;
        Ok(bytes)
    });
    let (mut stream, _) = loop {
        match listener.accept() {
            Ok(value) => break value,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(1))
            }
            Err(error) => return Err(error.into()),
        }
    };
    ready.send(())?;
    let result = handle(
        &mut stream,
        Path::new("/unused"),
        Path::new("/unused"),
        &address.to_string(),
        "test",
    );
    drop(stream);
    let bytes = client.join().map_err(|_| "HTTP client panicked")??;
    result?;
    assert!(bytes.starts_with(b"HTTP/1.1 200 OK"));
    Ok(())
}
