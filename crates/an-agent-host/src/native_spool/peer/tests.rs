use super::*;

#[test]
fn phase_identity_is_separate_and_context_binds_both_endpoints() {
    let mut record = json!({"approval_sha256":"request-identity","plan":{
        "source":{"instance":"a","generation":0},"target":{"instance":"b","generation":0}}});
    assert_ne!(
        run_name(&record, "request").unwrap(),
        run_name(&record, "response").unwrap()
    );
    let initial = context(&record, "response");
    record["plan"]["source"]["generation"] = json!(1);
    assert_ne!(initial, context(&record, "response"));
    record["plan"]["source"]["generation"] = json!(0);
    record["plan"]["target"]["generation"] = json!(1);
    assert_ne!(initial, context(&record, "response"));
}

#[test]
fn reply_cannot_accept_a_missing_execution_receipt() {
    let missing = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    let record = json!({"plan":{"target":{"root":missing}}});
    assert!(result(&missing, &record, "request").is_err());
}
