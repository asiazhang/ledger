use super::*;

#[test]
fn 钩子未注册即码化错误_错误码可判读() {
    let err = investment_hook_missing_error("prepare");
    assert_eq!(err.code(), Some("transaction.investment-hook-unregistered"));
}

#[test]
fn 命令字段部件全缺省构造_none() {
    let parts = PlanCommandParts::none();
    assert!(parts.investment.is_none());
    assert!(parts.split.is_none());
    assert!(parts.convert.is_none());
}
