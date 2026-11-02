use super::*;
use an_agent_core::testkit::TempDir;

fn fixture() -> Result<(TempDir, Value)> {
    let tmp = TempDir::new("native-reference");
    let catalog = tmp.path().canonicalize()?.join("catalog");
    let published = publish(&catalog, b"fixed", None, json!({"kind":"test"}))?;
    let (_, binding) = read(&catalog, published["revision"].as_str().unwrap())?;
    let reference = json!({"schema":"an-native-input-reference-v1","catalog":catalog,
        "binding":binding,"owner":{"instance":"a","run":"one"},"start_sha256":"a".repeat(64)});
    Ok((tmp, reference))
}
#[test]
fn claims_are_owned_and_never_reacquired_or_silently_released() -> Result<()> {
    let (_tmp, reference) = fixture()?;
    acquire(&reference)?;
    assert!(inspect(&reference)?["release"].is_null());
    assert!(acquire(&reference).is_err());
    let mut foreign = reference.clone();
    foreign["owner"]["instance"] = json!("b");
    assert!(inspect(&foreign).is_err());
    let catalog = Path::new(reference["catalog"].as_str().unwrap());
    {
        let _writer = store::lock(catalog)?;
        assert!(release(&reference, &"b".repeat(64)).is_err());
        assert!(inspect(&reference)?["release"].is_null());
    }
    release(&reference, &"b".repeat(64))?;
    assert!(!inspect(&reference)?["release"].is_null());
    assert!(release(&reference, &"b".repeat(64)).is_err());
    assert_eq!(
        read(catalog, reference["binding"]["revision"].as_str().unwrap())?.0,
        b"fixed"
    );
    Ok(())
}
#[test]
fn retirement_and_torn_claims_refuse_acquisition_without_repair() -> Result<()> {
    let (_tmp, reference) = fixture()?;
    let (claim, released) = paths(&reference)?;
    store::directory(claim.parent().unwrap())?;
    store::write_new(&claim, b"{torn")?;
    assert!(acquire(&reference).is_err());
    assert!(inspect(&reference).is_err());
    assert_eq!(fs::read(&claim)?, b"{torn");
    let mut next = reference.clone();
    next["owner"]["run"] = json!("next");
    let closed = claim
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("admission-retired.json");
    store::write_new(&closed, b"{uncertain")?;
    assert!(acquire(&next).is_err());
    assert!(!paths(&next)?.0.exists());
    assert!(!released.exists());
    Ok(())
}

#[test]
fn two_input_roles_can_retain_the_same_revision_independently() -> Result<()> {
    let (_tmp, mut first) = fixture()?;
    first["slot"] = json!(0);
    let mut second = first.clone();
    second["slot"] = json!(1);
    acquire(&first)?;
    acquire(&second)?;
    release(&first, &"a".repeat(64))?;
    assert!(inspect(&second)?["release"].is_null());
    Ok(())
}

fn observation(reference: &Value) -> Result<Value> {
    observe(
        Path::new(reference["catalog"].as_str().unwrap()),
        reference["binding"]["revision"].as_str().unwrap(),
    )
}

#[test]
fn source_observation_works_under_writer_lock_without_granting_collection() -> Result<()> {
    let (_tmp, first) = fixture()?;
    let empty = observation(&first)?;
    assert_eq!(empty["registered_claims"], 0);
    assert_eq!(empty["reference_census_complete"], false);
    assert_eq!(empty["collection_authorized"], false);
    acquire(&first)?;
    let mut second = first.clone();
    second["owner"]["run"] = json!("second");
    acquire(&second)?;
    release(&first, &"b".repeat(64))?;
    let catalog = Path::new(first["catalog"].as_str().unwrap());
    let _writer = store::lock(catalog)?;
    let view = observation(&first)?;
    assert_eq!(view["registered_claims"], 2);
    assert_eq!(view["source_release_records"], 1);
    assert_eq!(view["consumer_receipts_checked"], false);
    assert_eq!(view["collection_authorized"], false);
    assert!(
        view["claims"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["reference"] == first && c["observation"] == "source_release_recorded")
    );
    assert!(view["claims"].as_array().unwrap().iter().any(|c|
        c["reference"] == second && c["observation"] == "claim_without_source_release"));
    Ok(())
}

#[test]
fn source_observation_refuses_orphan_torn_and_foreign_records_without_repair() -> Result<()> {
    let (_tmp, reference) = fixture()?;
    acquire(&reference)?;
    release(&reference, &"c".repeat(64))?;
    let (claim, marker) = paths(&reference)?;
    let original = fs::read(&claim)?;
    fs::remove_file(&claim)?;
    assert!(observation(&reference).is_err());
    assert!(marker.exists());
    store::write_new(&claim, b"{torn")?;
    assert!(observation(&reference).is_err());
    assert_eq!(fs::read(&claim)?, b"{torn");
    fs::write(&claim, &original)?;
    let foreign = claim.with_file_name(format!("{}.json", "f".repeat(64)));
    fs::rename(&claim, &foreign)?;
    assert!(observation(&reference).is_err());
    assert_eq!(fs::read(&foreign)?, original);
    Ok(())
}

#[test]
fn source_observation_binds_selected_bytes_and_refuses_excess_history() -> Result<()> {
    let (_tmp, reference) = fixture()?;
    let mut wrong = reference.clone();
    wrong["binding"]["bytes"] = json!(999);
    let (path, _) = paths(&wrong)?;
    store::directory(path.parent().unwrap())?;
    store::write_new(&path, &serde_json::to_vec(&wrong)?)?;
    assert!(observation(&reference).is_err());
    fs::remove_file(&path)?;
    for n in 0..1025 {
        store::write_new(&path.with_file_name(format!("{n:064x}.json")), b"{}")?;
    }
    let error = observation(&reference).unwrap_err().to_string();
    assert!(error.contains("inventory is invalid or exceeds"), "{error}");
    Ok(())
}

#[cfg(unix)]
#[test]
fn source_observation_never_follows_reference_links() -> Result<()> {
    let (tmp, reference) = fixture()?;
    acquire(&reference)?;
    let (path, _) = paths(&reference)?;
    let outside = tmp.path().join("saved");
    fs::rename(&path, &outside)?;
    std::os::unix::fs::symlink(&outside, &path)?;
    assert!(observation(&reference).is_err());
    assert!(fs::symlink_metadata(&path)?.file_type().is_symlink());
    Ok(())
}

#[test]
fn checkpoint_claim_cannot_be_released_as_an_attempt() -> Result<()> {
    let (_tmp, mut reference) = fixture()?;
    reference["schema"] = json!("an-native-checkpoint-reference-v1");
    reference["owner"] = json!({"checkpoint_revision":"c".repeat(64)});
    acquire(&reference)?;
    assert!(release(&reference, &"b".repeat(64)).is_err());
    assert!(inspect(&reference)?["release"].is_null());
    let (_, released) = paths(&reference)?;
    store::write_new(&released, b"{}")?;
    assert!(inspect(&reference).is_err());
    assert!(
        observe(
            Path::new(reference["catalog"].as_str().unwrap()),
            reference["binding"]["revision"].as_str().unwrap()
        )
        .is_err()
    );
    Ok(())
}
