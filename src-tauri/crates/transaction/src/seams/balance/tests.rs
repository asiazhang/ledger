//! 余额刷新接缝单元测试（issue #1090）：未注册即码化错误、钩子在场的委派透传。

use super::*;

#[test]
fn 未注册即码化错误_错误码可判读() {
    // 建库经统一测试工厂（ADR-0084 规则 1）；委派入参显式传 None，
    // 与工厂顺带注册的全局钩子无涉（不读槽）。
    let conn = tauri_app_lib::test_support::open();
    let err = dispatch_balance_refresh(None, &conn, None, None).expect_err("未注册应报错");
    assert_eq!(
        err.code(),
        Some("transaction.balance-refresh-hook-unregistered")
    );
}

#[test]
fn 钩子在场即委派_新旧引用集原样透传() {
    // 断言委派实参按位透传（推导语义属账户域，本域只透传引用集；出资子行端
    // 随 ADR-0138 决策 7 纳入载荷）。
    fn stub(
        _conn: &Connection,
        old: Option<RowAccounts<'_>>,
        new: Option<RowAccounts<'_>>,
    ) -> Result<()> {
        assert_eq!(old, None, "创建形态 old=None 原样透传");
        let new = new.expect("new 应在场");
        assert_eq!(new.account_id, Some("acc"));
        assert_eq!(new.to_account_id, Some("to"));
        assert_eq!(new.funding_account_id, None);
        assert_eq!(new.funding_items, &["f1", "f2"] as &[&str]);
        Ok(())
    }
    let conn = tauri_app_lib::test_support::open();
    dispatch_balance_refresh(
        Some(stub),
        &conn,
        None,
        Some(RowAccounts {
            account_id: Some("acc"),
            to_account_id: Some("to"),
            funding_account_id: None,
            funding_items: &["f1", "f2"],
        }),
    )
    .expect("钩子在场应成功");
}
