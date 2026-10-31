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
