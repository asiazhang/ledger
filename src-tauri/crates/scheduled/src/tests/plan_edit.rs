use super::super::*;
use super::common::create_subscription;
use rusqlite::params;
use tauri_app_lib::test_support;

#[test]
fn update_subscription_rejects_amount_field_including_explicit_null() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let plan_id = create_subscription(&conn, "acc", "CNY", 3000, Some("视频会员"));

    let payloads = [
        r#"{"id":"{id}","account_id":"acc","note":"x","amount_cents":5000}"#,
        r#"{"id":"{id}","account_id":"acc","note":"x","amount_cents":null}"#,
        r#"{"id":"{id}","account_id":"acc","note":"x","total_amount_cents":null}"#,
    ];
    for payload in payloads {
        let json = payload.replace("{id}", &plan_id);
        let input: UpdateSubscriptionInput = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("反序列化应成功（拒绝发生在领域层）: {e}"));
        let err =
            update_subscription(&conn, input).expect_err("携带金额字段的编辑请求应被显式拒绝");
        assert!(
            err.to_string().contains("改价 = 取消旧计划 + 新建"),
            "拒绝信息应提示改价路径: {err}"
        );
    }

    let (note, amount): (Option<String>, i64) = conn
        .query_row(
            "SELECT note,amount_cents FROM scheduled_transactions WHERE id=?1",
            params![plan_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(note.as_deref(), Some("视频会员"), "备注不应被改动");
    assert_eq!(amount, 3000, "金额不应被改动");
}
