//! 投资接缝单元测试（issue #1092）：未注册即码化错误、命令字段部件缺省构造。

use super::*;

#[test]
fn 钩子未注册即码化错误_错误码可判读() {
    // 建库经统一测试工厂（ADR-0084 规则 1）；工厂顺带注册的全局钩子在场，
    // 本断言只对准错误码构造纯函数（不读槽），与全局状态无涉。
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
