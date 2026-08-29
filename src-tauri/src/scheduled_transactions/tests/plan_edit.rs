//! 订阅编辑校验（issue #162，ADR-0023 决策三）：仅非金额字段可编辑，
//! 金额字段（含显式 null）显式拒绝——改价 = 取消旧计划 + 新建。

use super::super::*;
use super::common::{create_subscription, insert_account, setup_db};
use rusqlite::params;

// ---------------------------------------------------------------------------
// 订阅编辑——仅非金额字段（issue #162，ADR-0023 决策三）
// ---------------------------------------------------------------------------

/// 金额哨兵边界：请求携带 `amount_cents` / `total_amount_cents`（含显式 null）
/// 一律显式拒绝，且拒绝后计划字段不被改动。
#[test]
fn update_subscription_rejects_amount_field_including_explicit_null() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
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

    // 拒绝后计划未被改动
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
