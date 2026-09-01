//! 「写请求返回后失效信号最终到达」HTTP 壳集成验证（spec #367，ADR-0054 对外行为防线）。
//!
//! #366 的机制层回归（`signals::emit_blocking_tests`，源码内单测）钉住「发射步骤
//! 不阻塞写路径」；本模块补 **HTTP 壳整链**维度：把闸门式假发射器装进真端点的
//! 发射槽（`ApiState.emitter`，ADR-0054 #367 修订泛化为发射器接缝），经真实写端点
//! 断言外部行为——
//!
//! - **延迟可接受**：写请求先于信号送达而返回（信号已交接、尚未送达，响应已 2xx）；
//! - **不允许永久丢失**：信号最终到达（等待带超时上界，丢失即超时失败而非挂起）。
//!
//! 覆盖端点：`POST /api/v1/accounts`（映射静态行）与
//! `POST /api/v1/transactions/batch`（AI 导入真实入口；条件信号行——批内任一行
//! 即建商户才发参考失效信号，issue #331 接线，整链「写 → 证据 → 信号」首次可见）。
//!
//! 刻意**不复现真实死锁时序**（跨线程时序问题易 flaky，与 #366 同一测试哲学）：
//! 假发射器（共享测试桩 `test_utils::GatedEmitter`，与机制层回归共用）的 `post`
//! 本身非阻塞交接（与生产 `AppHandle` 实现同一约定），只构造「发射被闸门延迟」
//! 这一外部条件。若壳层把发射槽退化回「同步等发射完成」或投递丢失，测试在
//! 超时上界后红，而不是挂起测试进程。

use std::sync::Arc;

use tauri_app_lib::events;
use tauri_app_lib::test_utils::GatedEmitter;

use crate::common::{
    batch_body, create_account_via_api, delete_account_via_api, post_batch, setup_app_with_emitter,
};

/// 核心验收（spec #367）：`POST /api/v1/accounts` 写请求返回时信号已交接、
/// 尚未送达（延迟可接受）；放行后信号最终到达、事件名正确、不丢失。
#[tokio::test]
async fn account_create_returns_before_signal_and_signal_eventually_arrives() {
    let emitter = GatedEmitter::gated();
    let (app, _conn) = setup_app_with_emitter(Arc::new(emitter.clone()));

    // 写请求完整走真端点（路由 → handler → 写库 → 发射槽交接 → 响应）并返回。
    let account_id = create_account_via_api(&app, "信号测试账户").await;
    assert!(!account_id.is_empty(), "写请求应正常返回账户 id");

    // 写请求返回时刻：信号已交接给发射器、但尚未送达——延迟中的信号不阻塞写响应。
    assert_eq!(
        emitter.posted(),
        vec![events::LEDGER_CHANGED],
        "账户创建应经映射交接恰好一条参考失效信号"
    );
    assert!(
        emitter.delivered().is_empty(),
        "闸门未放行前信号尚未送达：写请求先于信号送达而返回（延迟可接受）"
    );

    // 放行后信号最终到达（不允许丢失），且恰为交接的那一条。
    emitter.open_gate();
    assert_eq!(
        emitter.wait_delivered(),
        vec![events::LEDGER_CHANGED],
        "写请求返回后信号必须最终到达"
    );
}

/// AI 导入真实入口（spec #364 事故路径）：批量导入即建商户的条件信号
///「写 → 证据 → 信号」整链可见——写请求返回后 `ledger:changed` 最终到达；
/// 商户命中复用的批次（零信号行）不交接任何事件。多次交接按入队顺序送达、
/// 不重不漏。
#[tokio::test]
async fn batch_import_signal_eventually_arrives_and_reuse_batch_stays_silent() {
    let emitter = GatedEmitter::gated();
    let (app, conn) = setup_app_with_emitter(Arc::new(emitter.clone()));
    let account_id = create_account_via_api(&app, "现金账户").await;

    // 账户创建（同为写端点）已交接一条参考失效信号；第一批：带未命中商户名 →
    // 即建商户（第四张参考表）→ 再交接一条。
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

    // 第二批：命中复用既有商户（零信号行）→ 不交接任何事件。
    let expense_reusing_merchant = format!(
        r#"{{"kind":"expense","amount_cents":2000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-02","merchant_name":"信号测试商户"}}"#
    );
    post_batch(&app, batch_body(&[&expense_reusing_merchant], None)).await;
    assert_eq!(
        emitter.posted().len(),
        2,
        "命中复用商户的批次是零信号行，不得交接任何事件"
    );

    // 放行后信号按入队顺序最终到达、不重不漏（两批合计仅前两次交接）。
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

/// 深度防线补强：账户创建后立即接删除请求（另一写端点、另一映射静态行），
/// 两次交接按入队顺序在放行后一并到达——「同一线程先后入队的动作按入队顺序
/// 在主线程依次执行、事件间无乱序」（ADR-0054 投递语义）在 HTTP 壳整链成立。
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
