use super::*;

#[test]
fn 未注册错误_错误码可判读() {
    let err = plan_source_resolver_missing_error();
    assert_eq!(
        err.code(),
        Some("transaction.plan-source-resolver-unregistered")
    );
}
