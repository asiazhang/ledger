//! 本位币基准读取接缝单元测试（issue #1092）：未注册即码化错误。

use super::*;

#[test]
fn 钩子未注册即码化错误_错误码可判读() {
    let err = base_currency_reader_missing_error();
    assert_eq!(
        err.code(),
        Some("transaction.base-currency-reader-unregistered")
    );
}
