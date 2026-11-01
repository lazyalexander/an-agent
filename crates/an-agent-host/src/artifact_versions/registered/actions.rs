//! Serialized admission and collection. A durable intent fences any lost operation outcome.
use super::*;

pub enum Operation<'a> {
    Show,
    Close,
    Checkout(&'a str, &'a Path),
    Release(&'a str),
    Delete,
}

pub(crate) fn operate(
    catalog: &Path,
    binding: &Value,
    owner: Value,
    action: Operation<'_>,
    approved: Option<&str>,
) -> Result<Value> {
    match action {
        Operation::Show => transaction(catalog, binding, false, |tx, l| inspect(tx, l, binding)),
        Operation::Checkout(name, dest) => checkout(catalog, binding, &owner, name, dest, approved),
        Operation::Delete => delete(catalog, binding, &owner, approved),
        Operation::Close | Operation::Release(_) => {
            transaction(catalog, binding, approved.is_some(), |tx, l| {
                if l.intent.is_some() {
                    return Err("collection already submitted; inspect original evidence".into());
                }
                payload(tx, binding)?;
                let plan = match action {
                    Operation::Close => {
                        ensure_open(l)?;
                        json!({"operation":"close-readers","owner":owner,"revision":l.revision})
                    }
                    Operation::Release(name) => {
                        let r = l.readers.get(name).ok_or("unknown registered reader")?;
                        if r.phase != "copied" || r.release.is_some() {
                            return Err("release requires original completed copy".into());
                        }
                        json!({"operation":"release-reader","owner":owner,"copy":r.plan,"revision":l.revision})
                    }
                    _ => unreachable!(),
                };
                let (digest, execute) = approval(&plan, approved)?;
                if !execute {
                    return Ok(preview(plan, digest));
                }
                match action {
                    Operation::Close => {
                        l.closure = Some(json!({"plan":plan,"approval_sha256":digest}))
                    }
                    Operation::Release(name) => {
                        l.readers.get_mut(name).ok_or("missing reader")?.release =
                            Some(digest.clone())
                    }
                    _ => unreachable!(),
                }
                Ok(json!({"status":"succeeded","approval_sha256":digest}))
            })
        }
    }
}

fn approval(plan: &Value, approved: Option<&str>) -> Result<(String, bool)> {
    let digest = hash(plan)?;
    if approved.is_some_and(|h| h != digest) {
        return Err("registered operation approval mismatch".into());
    }
    Ok((digest, approved.is_some()))
}

fn preview(plan: Value, digest: String) -> Value {
    json!({"executed":false,"plan":plan,"approval_sha256":digest})
}

fn checkout(
    catalog: &Path,
    binding: &Value,
    owner: &Value,
    name: &str,
    dest: &Path,
    approved: Option<&str>,
) -> Result<Value> {
    store::no_links(dest)?;
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let target = parent.join(dest.file_name().ok_or("destination name")?);
    if target.starts_with(Path::new(owner["root"].as_str().ok_or("producer root")?))
        || target.try_exists()?
    {
        return Err("copy requires a new file outside the producer instance".into());
    }
    let plan = json!({"operation":"registered-copy","owner":owner,"reader":name,"destination":target,"binding":binding});
    let (digest, execute) = approval(&plan, approved)?;
    transaction(catalog, binding, execute, |_, l| {
        ensure_open(l)?;
        if l.readers.len() >= 128 || l.readers.contains_key(name) {
            return Err("reader identity already reserved or ledger full".into());
        }
        if execute {
            l.readers.insert(
                name.into(),
                Reader {
                    plan: plan.clone(),
                    phase: "opening".into(),
                    release: None,
                },
            );
        }
        Ok(())
    })?;
    if !execute {
        return Ok(preview(plan, digest));
    }
    // Read only after the original reservation commits. Closing admission cannot strand an admitted copy.
    let bytes = transaction(catalog, binding, false, |tx, l| {
        let r = l.readers.get(name).ok_or("reader reservation missing")?;
        if r.phase != "opening" || r.plan != plan {
            return Err("copy reservation mismatch".into());
        }
        payload(tx, binding)
    })?;
    store::write_new(&target, &bytes)?;
    fs::File::open(&parent)?.sync_all()?;
    transaction(catalog, binding, true, |_, l| {
        let r = l
            .readers
            .get_mut(name)
            .ok_or("reader reservation missing")?;
        if r.phase != "opening" || r.plan != plan {
            return Err("copy completion identity mismatch".into());
        }
        r.phase = "copied".into();
        Ok(())
    })?;
    Ok(json!({"status":"succeeded","copy":plan,"approval_sha256":digest}))
}

fn deletion_plan(l: &Ledger, owner: &Value, binding: &Value) -> Result<Value> {
    let closed = l.closure.as_ref().ok_or("close reader admission first")?;
    if l.readers
        .values()
        .any(|r| r.phase != "copied" || r.release.is_none())
    {
        return Err("registered checkpoint or unreleased/uncertain reader blocks deletion".into());
    }
    // A checkpoint pin is recorded in this ledger before its source-side mirror.
    // Contradictory external evidence must never turn an empty ledger into deletion authority.
    let mirrors = l
        .catalog
        .join("revisions")
        .join(&l.revision)
        .join("native-input-references");
    store::no_links(&mirrors)?;
    if mirrors.try_exists()? && fs::read_dir(&mirrors)?.next().is_some() {
        return Err("additional source reference evidence blocks deletion".into());
    }
    let close_plan = json!({"operation":"close-readers","owner":owner,"revision":l.revision});
    if *closed != json!({"plan":close_plan,"approval_sha256":hash(&close_plan)?}) {
        return Err("registered closure evidence mismatch".into());
    }
    for (name, reader) in &l.readers {
        let release_plan = json!({"operation":"release-reader","owner":owner,"copy":reader.plan,"revision":l.revision});
        if reader.plan["operation"] != "registered-copy"
            || reader.plan["owner"] != *owner
            || reader.plan["binding"] != *binding
            || reader.plan["reader"] != *name
            || reader.release.as_deref() != Some(hash(&release_plan)?.as_str())
        {
            return Err("registered reader completion evidence mismatch".into());
        }
    }
    Ok(
        json!({"schema":"an-native-registered-delete-v1","owner":owner,"binding":binding,
        "closure":closed,"reader_evidence_sha256":hash(&l.readers)?,"registered_readers":l.readers.len()}),
    )
}

fn delete(catalog: &Path, binding: &Value, owner: &Value, approved: Option<&str>) -> Result<Value> {
    let plan = transaction(catalog, binding, approved.is_some(), |tx, l| {
        if l.intent.is_some() || l.receipt.is_some() {
            return Err(
                "collection already submitted; inspect original receipt, never replay".into(),
            );
        }
        let plan = deletion_plan(l, owner, binding)?;
        payload(tx, binding)?;
        let (digest, execute) = approval(&plan, approved)?;
        if execute {
            l.intent = Some(json!({"plan":plan,"approval_sha256":digest}));
        }
        Ok(plan)
    })?;
    let digest = hash(&plan)?;
    if approved.is_none() {
        return Ok(preview(plan, digest));
    }
    transaction(catalog, binding, true, |tx, l| {
        let intent = json!({"plan":plan,"approval_sha256":digest});
        if l.intent.as_ref() != Some(&intent)
            || l.receipt.is_some()
            || deletion_plan(l, owner, binding)? != plan
        {
            return Err("collection evidence changed; retain original intent".into());
        }
        payload(tx, binding)?;
        let receipt = json!({"schema":"an-native-registered-deletion-receipt-v1","revision":l.revision,
            "approval_sha256":digest,"content_sha256":binding["content_sha256"],"logical_bytes":binding["bytes"]});
        tx.execute("UPDATE custody SET payload=NULL WHERE id=1", [])?;
        l.receipt = Some(receipt.clone());
        Ok(json!({"status":"succeeded","receipt":receipt}))
    })
}

pub(super) fn inspect(tx: &Transaction<'_>, l: &Ledger, binding: &Value) -> Result<Value> {
    let phase = if let Some(receipt) = &l.receipt {
        let intent = l.intent.as_ref().ok_or("missing deletion intent")?;
        let plan = deletion_plan(l, &intent["plan"]["owner"], binding)?;
        let digest = hash(&plan)?;
        let absent: bool =
            tx.query_row("SELECT payload IS NULL FROM custody WHERE id=1", [], |r| {
                r.get(0)
            })?;
        if !absent
            || *intent != json!({"plan":plan,"approval_sha256":digest})
            || *receipt
                != json!({"schema":"an-native-registered-deletion-receipt-v1","revision":l.revision,
            "approval_sha256":digest,"content_sha256":binding["content_sha256"],"logical_bytes":binding["bytes"]})
        {
            return Err("deletion receipt or payload disposition mismatch".into());
        }
        "deleted"
    } else if l.intent.is_some() {
        "deletion-unconfirmed"
    } else {
        payload(tx, binding)?;
        if l.closure.is_some() {
            "closed"
        } else {
            "open"
        }
    };
    Ok(
        json!({"phase":phase,"revision":l.revision,"readers":l.readers,"closure":l.closure,
        "intent":l.intent,"receipt":l.receipt,"automatic_retries":0}),
    )
}
