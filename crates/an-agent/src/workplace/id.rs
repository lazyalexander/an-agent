use sha2::{Digest, Sha256};

/// SHA-256 of a typed object payload.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjectId(pub [u8; 32]);

impl ObjectId {
    pub fn from_payload(kind: &str, payload: &[u8]) -> Self {
        let mut h = Sha256::new();
        h.update(kind.as_bytes());
        h.update(b" ");
        h.update(payload.len().to_string().as_bytes());
        h.update([0]);
        h.update(payload);
        let d = h.finalize();
        let mut id = [0u8; 32];
        id.copy_from_slice(&d);
        Self(id)
    }

    pub fn hex(&self) -> String {
        hex(&self.0)
    }

    pub fn from_hex(s: &str) -> Result<Self, String> {
        if s.len() != 64 {
            return Err("object id must be 64 hex chars".into());
        }
        let mut id = [0u8; 32];
        for i in 0..32 {
            id[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(|_| "invalid hex".to_string())?;
        }
        Ok(Self(id))
    }
}

impl std::fmt::Display for ObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.hex())
    }
}

impl std::fmt::Debug for ObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.hex()[..12])
    }
}

fn hex(bytes: &[u8]) -> String {
    const T: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(T[(b >> 4) as usize] as char);
        s.push(T[(b & 0xf) as usize] as char);
    }
    s
}
