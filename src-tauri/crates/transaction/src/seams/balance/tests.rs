use super::*;

#[test]
fn 未注册即码化错误_错误码可判读() {
    let conn = tauri_app_lib::test_support::open();
    let err = dispatch_balance_refresh(None, &conn, None, None).expect_err("未注册应报错");
    assert_eq!(
        err.code(),
        Some("transaction.balance-refresh-hook-unregistered")
    );
}

#[test]
fn 钩子在场即委派_新旧引用集原样透传() {
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
