//! 来源列反查接缝单元测试（issue #1090）：未注册即码化错误。

use super::*;

#[test]
fn 未注册错误_错误码可判读() {
    let err = plan_source_resolver_missing_error();
    assert_eq!(
        err.code(),
        Some("transaction.plan-source-resolver-unregistered")
    );
}
