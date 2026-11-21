use std::sync::Arc;

use ledger_infra::events;
use ledger_infra::test_utils::GatedEmitter;

use crate::common::{
    batch_body, count_rows, create_account_via_api, delete_account_via_api, post_batch,
    setup_app_with_emitter,
};

#[tokio::test]
async fn account_create_returns_before_signal_and_signal_eventually_arrives() {
    let emitter = GatedEmitter::gated();
    let (app, _conn) = setup_app_with_emitter(Arc::new(emitter.clone()));

    let account_id = create_account_via_api(&app, "信号测试账户").await;
    assert!(!account_id.is_empty(), "写请求应正常返回账户 id");

    assert_eq!(
        emitter.posted(),
        vec![events::LEDGER_CHANGED],
        "账户创建应经映射交接恰好一条参考失效信号"
    );
    assert!(
        emitter.delivered().is_empty(),
        "闸门未放行前信号尚未送达：写请求先于信号送达而返回（延迟可接受）"
    );

    emitter.open_gate();
    assert_eq!(
        emitter.wait_delivered(),
        vec![events::LEDGER_CHANGED],
        "写请求返回后信号必须最终到达"
    );
}

#[tokio::test]
async fn batch_import_signal_eventually_arrives_and_reuse_batch_stays_silent() {
    let emitter = GatedEmitter::gated();
    let (app, conn) = setup_app_with_emitter(Arc::new(emitter.clone()));
    let account_id = create_account_via_api(&app, "现金账户").await;

    let expense_with_new_merchant = format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-01","merchant_name":"信号测试商户"}}"#
    );
    post_batch(&app, batch_body(&[&expense_with_new_merchant], None)).await;
    assert_eq!(
        emitter.posted(),
        vec![events::LEDGER_CHANGED, events::LEDGER_CHANGED],
        "账户创建与批内即建商户各交接恰好一条参考失效信号"
    );
    assert!(
        emitter.delivered().is_empty(),
        "闸门未放行前信号尚未送达：批量导入写请求先于信号送达而返回"
    );

    let expense_reusing_merchant = format!(
        r#"{{"kind":"expense","amount_cents":2000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-02","merchant_name":"信号测试商户"}}"#
    );
    post_batch(&app, batch_body(&[&expense_reusing_merchant], None)).await;
    assert_eq!(
        emitter.posted().len(),
        2,
        "命中复用商户的批次是零信号行，不得交接任何事件"
    );

    emitter.open_gate();
    assert_eq!(
        emitter.wait_delivered(),
        vec![events::LEDGER_CHANGED, events::LEDGER_CHANGED],
        "写请求返回后信号必须最终到达，且复用批次不多发"
    );
    assert_eq!(
        conn.lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM merchants WHERE is_deleted=0",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap(),
        1,
        "两批同商户名应精确复用为同一商户行（零信号断言的前提）"
    );
}

#[tokio::test]
async fn queued_signals_arrive_in_posting_order_after_gate_opens() {
    let emitter = GatedEmitter::gated();
    let (app, _conn) = setup_app_with_emitter(Arc::new(emitter.clone()));

    let account_id = create_account_via_api(&app, "顺序测试账户").await;
    let (status, _) = delete_account_via_api(&app, &account_id).await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);

    assert_eq!(
        emitter.posted(),
        vec![events::LEDGER_CHANGED, events::LEDGER_CHANGED],
        "创建与删除各交接一条参考失效信号，先后入队"
    );

    emitter.open_gate();
    assert_eq!(
        emitter.wait_delivered(),
        vec![events::LEDGER_CHANGED, events::LEDGER_CHANGED],
        "放行后按入队顺序送达、不重不漏"
    );
}

#[tokio::test]
async fn batch_import_transfer_with_merchant_signal_eventually_arrives() {
    let emitter = GatedEmitter::gated();
    let (app, conn) = setup_app_with_emitter(Arc::new(emitter.clone()));
    let from = create_account_via_api(&app, "现金账户").await;
    let to = create_account_via_api(&app, "银行账户").await;

    let transfer_with_new_merchant = format!(
        r#"{{"kind":"transfer","amount_cents":1000,"currency_code":"CNY","account_id":"{from}","to_account_id":"{to}","date":"2026-05-03","merchant_name":"借贷信号商户"}}"#
    );
    post_batch(&app, batch_body(&[&transfer_with_new_merchant], None)).await;
    assert_eq!(
        emitter.posted(),
        vec![
            events::LEDGER_CHANGED,
            events::LEDGER_CHANGED,
            events::LEDGER_CHANGED
        ],
        "两账户创建 + transfer 即建商户各交接一条参考失效信号"
    );

    let transfer_reusing = format!(
        r#"{{"kind":"transfer","amount_cents":2000,"currency_code":"CNY","account_id":"{from}","to_account_id":"{to}","date":"2026-05-04","merchant_name":"借贷信号商户"}}"#
    );
    post_batch(&app, batch_body(&[&transfer_reusing], None)).await;
    assert_eq!(
        emitter.posted().len(),
        3,
        "命中复用商户的 transfer 批次是零信号行，不得交接任何事件"
    );

    emitter.open_gate();
    assert_eq!(
        emitter.wait_delivered(),
        vec![
            events::LEDGER_CHANGED,
            events::LEDGER_CHANGED,
            events::LEDGER_CHANGED
        ],
        "写请求返回后信号必须最终到达，复用批次不多发"
    );
    assert_eq!(
        count_rows(&conn.lock().unwrap(), "merchants"),
        1,
        "两批同商户名应精确复用为同一商户行"
    );
}
