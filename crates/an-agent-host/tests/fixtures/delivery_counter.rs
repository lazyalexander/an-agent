//! Domain-only fixture: produce a count or add the delivered count to local state.
use std::{fs, io};

fn field<'a>(input: &'a str, name: &str) -> io::Result<&'a str> {
    let prefix = format!("\"{name}\":\"");
    input.split_once(&prefix).and_then(|(_, tail)| tail.split_once('"'))
        .map(|(value, _)| value).ok_or_else(|| io::Error::other("missing fixture field"))
}

fn main() -> io::Result<()> {
    assert!(fs::read_dir("HOST_INSTANCES").is_err());
    assert!(fs::read("HOST_SECRET").is_err());
    let input = fs::read_to_string("input.txt")?;
    let state = field(&input, "state")?.parse::<u64>().map_err(io::Error::other)?;
    let product = field(&input, "product")?;
    let increment = if product.is_empty() { 1 } else {
        product.strip_prefix("count=").ok_or_else(|| io::Error::other("invalid domain product"))?
            .parse::<u64>().map_err(io::Error::other)?
    };
    let next = state.checked_add(increment).ok_or_else(|| io::Error::other("overflow"))?;
    fs::write("output.txt", format!(r#"{{"schema":"an-instance-bundle-v1","state":"{next}","product":"count={next}","report":"domain transition complete"}}"#))
}
