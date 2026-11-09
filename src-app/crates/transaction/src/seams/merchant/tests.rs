//! 商户接缝单元测试（issue #1092）：未注册即码化错误。

use super::*;

#[test]
fn 钩子未注册即码化错误_错误码可判读() {
    let err = merchant_hooks_missing_error();
    assert_eq!(err.code(), Some("transaction.merchant-hooks-unregistered"));
}
