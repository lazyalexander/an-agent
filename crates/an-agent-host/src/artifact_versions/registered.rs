//! Opt-in native custody. Payload reads require a committed copy or native attempt reservation.
use super::*;
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior, params};
use std::collections::BTreeMap;
mod actions;
mod attempts;
pub use actions::Operation;
pub(crate) use actions::{drained, operate};

pub(crate) const PROFILE: &str = "registered-copy-v1";
pub(crate) const FILE: &str = store::REGISTERED_CUSTODY_FILE;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    catalog: PathBuf,
    revision: String,
    readers: BTreeMap<String, Reader>,
    closure: Option<Value>,
    intent: Option<Value>,
    receipt: Option<Value>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reader {
    plan: Value,
    phase: String,
    release: Option<String>,
}

fn hash(value: &impl Serialize) -> Result<String> {
    Ok(store::digest(&serde_json::to_vec(value)?))
}

pub(super) fn create(dir: &Path, catalog: &Path, revision: &str, bytes: &[u8]) -> Result<()> {
    let path = dir.join(FILE);
    // This is a private staging directory, renamed into the revision namespace only after commit.
    store::write_new(&path, b"")?;
    let mut conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    conn.execute_batch(
        "PRAGMA auto_vacuum=FULL; PRAGMA synchronous=FULL; PRAGMA journal_mode=DELETE;",
    )?;
    let tx = conn.transaction()?;
    tx.execute_batch("CREATE TABLE custody (id INTEGER PRIMARY KEY CHECK(id=1), ledger TEXT NOT NULL, digest TEXT NOT NULL, payload BLOB);")?;
    let ledger = Ledger {
        catalog: catalog.canonicalize()?,
        revision: revision.into(),
        readers: BTreeMap::new(),
        closure: None,
        intent: None,
        receipt: None,
    };
    tx.execute(
        "INSERT INTO custody VALUES(1,?1,?2,?3)",
        params![serde_json::to_string(&ledger)?, hash(&ledger)?, bytes],
    )?;
    tx.commit()?;
    drop(conn);
    fs::File::open(path)?.sync_all()?;
    Ok(())
}

fn transaction<T>(
    catalog: &Path,
    binding: &Value,
    write: bool,
    op: impl FnOnce(&Transaction<'_>, &mut Ledger) -> Result<T>,
) -> Result<T> {
    let revision = binding["revision"]
        .as_str()
        .filter(|r| store::valid_digest(r))
        .ok_or("revision")?;
    if binding["reader_profile"] != PROFILE {
        return Err("legacy payload has no closed reader registry".into());
    }
    let catalog = catalog.canonicalize()?;
    let path = catalog.join("revisions").join(revision).join(FILE);
    store::no_links(&path)?;
    let m = fs::symlink_metadata(&path)?;
    if !m.is_file() {
        return Err("missing registered custody database".into());
    }
    #[cfg(unix)]
    if std::os::unix::fs::MetadataExt::nlink(&m) != 1 {
        return Err("linked custody database".into());
    }
    let flags = if write {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let mut conn = Connection::open_with_flags(path, flags | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    conn.busy_timeout(std::time::Duration::ZERO)?;
    conn.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA synchronous=FULL;")?;
    let tx = conn.transaction_with_behavior(if write {
        TransactionBehavior::Immediate
    } else {
        TransactionBehavior::Deferred
    })?;
    let (record, digest): (String, String) =
        tx.query_row("SELECT ledger,digest FROM custody WHERE id=1 AND length(CAST(ledger AS BLOB))<=2097152 AND length(digest)=64", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    if store::digest(record.as_bytes()) != digest {
        return Err("registered ledger digest mismatch".into());
    }
    let mut ledger: Ledger = serde_json::from_str(&record)?;
    if ledger.catalog != catalog || ledger.revision != revision || ledger.readers.len() > 128 {
        return Err("registered custody identity mismatch".into());
    }
    let out = op(&tx, &mut ledger)?;
    if write {
        if serde_json::to_vec(&ledger)?.len() > 2 * 1024 * 1024 {
            return Err("registered ledger size limit".into());
        }
        tx.execute(
            "UPDATE custody SET ledger=?1,digest=?2 WHERE id=1",
            params![serde_json::to_string(&ledger)?, hash(&ledger)?],
        )?;
    }
    tx.commit()?;
    Ok(out)
}

fn payload(tx: &Transaction<'_>, binding: &Value) -> Result<Vec<u8>> {
    let expected = binding["bytes"]
        .as_u64()
        .ok_or("registered binding length")?;
    let length: Option<u64> =
        tx.query_row("SELECT length(payload) FROM custody WHERE id=1", [], |r| {
            r.get(0)
        })?;
    if length.is_none() {
        return Err("payload missing; deletion requires a verified receipt".into());
    }
    if length != Some(expected) {
        return Err("registered payload length mismatch".into());
    }
    let bytes: Vec<u8> =
        tx.query_row("SELECT payload FROM custody WHERE id=1", [], |r| r.get(0))?;
    if json!(bytes.len()) != binding["bytes"] || store::digest(&bytes) != binding["content_sha256"]
    {
        return Err("registered payload integrity mismatch".into());
    }
    Ok(bytes)
}

fn ensure_open(ledger: &Ledger) -> Result<()> {
    if ledger.closure.is_some() || ledger.intent.is_some() || ledger.receipt.is_some() {
        return Err("registered reader admission is closed".into());
    }
    Ok(())
}

/// A routing observation checks the admission fence without materializing a payload.
pub(crate) fn admission_open(catalog: &Path, binding: &Value) -> Result<()> {
    if binding["reader_profile"] == PROFILE {
        transaction(catalog, binding, false, |_, ledger| ensure_open(ledger))
    } else {
        admission::ensure_active(catalog, binding["revision"].as_str().ok_or("revision")?)
    }
}

pub(crate) fn verify(catalog: &Path, revision: &str) -> Result<Value> {
    let binding = metadata(catalog, revision)?;
    if binding["reader_profile"] != PROFILE {
        return Ok(read(catalog, revision)?.1);
    }
    transaction(catalog, &binding, false, |tx, l| {
        if l.intent.is_some() {
            return Err("collection recorded; inspect original receipt".into());
        }
        payload(tx, &binding).map(drop)
    })?;
    Ok(binding)
}

pub(crate) fn verify_or_deleted(catalog: &Path, revision: &str) -> Result<Value> {
    let binding = metadata(catalog, revision)?;
    if binding["reader_profile"] != PROFILE {
        return Ok(read(catalog, revision)?.1);
    }
    transaction(catalog, &binding, false, |tx, l| {
        let view = actions::inspect(tx, l, &binding)?;
        if view["phase"] == "deletion-unconfirmed" {
            return Err("unconfirmed deletion blocks collection".into());
        }
        Ok(())
    })?;
    Ok(binding)
}

/// Retention-only data. No content is returned to the caller.
pub(crate) fn pinnable(catalog: &Path, revision: &str) -> Result<Value> {
    let binding = metadata(catalog, revision)?;
    if binding["reader_profile"] != PROFILE {
        return Ok(admission::read_input(catalog, revision)?.1);
    }
    transaction(catalog, &binding, false, |tx, l| {
        ensure_open(l)?;
        payload(tx, &binding).map(drop)
    })?;
    Ok(binding)
}
fn claim_kind(reference: &Value) -> Result<&'static str> {
    match reference["schema"].as_str() {
        Some("an-native-checkpoint-reference-v1") => Ok("checkpoint"),
        Some("an-native-input-reference-v1") => Ok("attempt"),
        _ => Err("unsupported registered reader".into()),
    }
}
pub(crate) fn pin(catalog: &Path, reference: &Value) -> Result<()> {
    let kind = claim_kind(reference)?;
    let binding = &reference["binding"];
    transaction(catalog, binding, true, |tx, l| {
        ensure_open(l)?;
        payload(tx, binding)?;
        let key = format!("{kind}:{}", hash(reference)?);
        if l.readers.contains_key(&key) || l.readers.len() >= 128 {
            return Err("source claim already reserved or ledger full".into());
        }
        l.readers.insert(
            key,
            Reader {
                phase: kind.into(),
                plan: reference.clone(),
                release: None,
            },
        );
        Ok(())
    })
}
pub(crate) fn check_pin(catalog: &Path, reference: &Value) -> Result<()> {
    transaction(catalog, &reference["binding"], false, |_, l| {
        check_claim(l, reference)
    })
}

fn check_claim(l: &Ledger, reference: &Value) -> Result<()> {
    let kind = claim_kind(reference)?;
    let record = l
        .readers
        .get(&format!("{kind}:{}", hash(reference)?))
        .ok_or("missing authoritative source claim")?;
    if record.phase != kind || record.plan != *reference || record.release.is_some() {
        return Err("registered source claim mismatch".into());
    }
    Ok(())
}

pub(crate) fn snapshot(reference: &Value) -> Result<Vec<u8>> {
    if claim_kind(reference)? != "attempt" {
        return Err("snapshot requires a native attempt reader".into());
    }
    let catalog = Path::new(reference["catalog"].as_str().ok_or("registered catalog")?);
    transaction(catalog, &reference["binding"], false, |tx, l| {
        check_claim(l, reference)?;
        if l.intent.is_some() || !references::mirror(reference)?["release"].is_null() {
            return Err("released or uncertain attempt cannot acquire another snapshot".into());
        }
        payload(tx, &reference["binding"])
    })
}

pub(super) fn admission_status(catalog: &Path, binding: &Value) -> Result<Value> {
    transaction(catalog, binding, false, |tx, l| {
        let view = actions::inspect(tx, l, binding)?;
        Ok(json!({"mode":view["phase"],"reader_profile":PROFILE}))
    })
}
