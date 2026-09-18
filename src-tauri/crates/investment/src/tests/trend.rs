use ledger_transaction::amount::TransactionKind;
use ledger_transaction::create_transaction_internal;
use rusqlite::Connection;
use tauri_app_lib::test_support::{
    open, seed_account, seed_fx_rate_history, seed_instrument, seed_price_history,
};

use super::super::*;
use super::common::*;

// ---------------------------------------------------------------------------
// 走势查询（issue #138 / spec #135 / ADR-0019）
// ---------------------------------------------------------------------------

// 走势查询为只读命令，价格 / 汇率历史周点用工厂种子直铺（seed_price_history /
// seed_fx_rate_history，同体上收自本文件本地副本，spec #728 / 票 #755）。

#[test]
fn instrument_price_trend_clips_range_and_starts_at_first_point() {
    let conn = open();
    seed_instrument(&conn, "inst-t1", "600519", "贵州茅台", "CNY", "unknown");
    seed_price_history(&conn, "ph-1", "inst-t1", "2026-01-05", 1_000_000, "CNY");
    seed_price_history(&conn, "ph-2", "inst-t1", "2026-01-12", 1_100_000, "CNY");
    seed_price_history(&conn, "ph-3", "inst-t1", "2026-01-19", 1_200_000, "CNY");
    seed_price_history(&conn, "ph-4", "inst-t1", "2026-02-02", 1_300_000, "CNY");

    // 区间裁剪：只返回区间内（含端点）的周点。
    let trend = trend::query_instrument_price_trend(
        &conn,
        "inst-t1",
        &TrendRange {
            start_date: Some("2026-01-10".into()),
            end_date: Some("2026-01-31".into()),
        },
    )
    .unwrap();
    let dates: Vec<&str> = trend.points.iter().map(|p| p.date.as_str()).collect();
    assert_eq!(dates, ["2026-01-12", "2026-01-19"]);
    assert_eq!(
        trend.points[0].price_cents, 1_100_000,
        "价格点直出万分之一元刻度"
    );
    assert_eq!(trend.points[0].currency_code, "CNY");
    assert_eq!(trend.instrument_id, "inst-t1");

    // 不设界（"全部"区间）：从首个有效采样点开始，升序完整返回。
    let trend =
        trend::query_instrument_price_trend(&conn, "inst-t1", &TrendRange::default()).unwrap();
    assert_eq!(trend.points.len(), 4);
    assert_eq!(trend.points[0].date, "2026-01-05");

    // 区间参数非法时报错，不静默返回曲线。
    let err = trend::query_instrument_price_trend(
        &conn,
        "inst-t1",
        &TrendRange {
            start_date: Some("2026-13-01".into()),
            end_date: None,
        },
    )
    .unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
    let err = trend::query_instrument_price_trend(
        &conn,
        "inst-t1",
        &TrendRange {
            start_date: Some("2026-02-01".into()),
            end_date: Some("2026-01-01".into()),
        },
    )
    .unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
}

#[test]
fn portfolio_trend_derives_quantity_from_buy_sell_flow() {
    let conn = open();
    seed_account(&conn, "acc-trd", "证券户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-t2", "000001", "平安银行", "CNY", "unknown");
    // 周价格点（万分之一元）：w1=10 元、w2=20 元、w3=30 元、w4=40 元（CNY，无需折算）。
    seed_price_history(&conn, "ph-w1", "inst-t2", "2026-02-02", 100_000, "CNY");
    seed_price_history(&conn, "ph-w2", "inst-t2", "2026-02-09", 200_000, "CNY");
    seed_price_history(&conn, "ph-w3", "inst-t2", "2026-02-16", 300_000, "CNY");
    seed_price_history(&conn, "ph-w4", "inst-t2", "2026-02-23", 400_000, "CNY");
    // 时序：w1 未买入（数量 0）→ w2 周内（02-06）买入 10 股 → w3 持有 10 股 → 2026-02-20（w3 内）清仓 → w4 归零。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-trd",
            "inst-t2",
            10.0,
            150_000,
            "2026-02-06",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-trd",
            "inst-t2",
            10.0,
            350_000,
            "2026-02-20",
        ),
    )
    .unwrap();

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(trend.currency_code, "CNY");
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    assert_eq!(
        values,
        [
            ("2026-02-02".to_string(), 0),     // 买入前：价格有效但持有为零
            ("2026-02-09".to_string(), 20000), // 10 × 2000
            ("2026-02-16".to_string(), 30000), // 10 × 3000（卖出在 02-20，尚未生效）
            ("2026-02-23".to_string(), 0),     // 清仓后归零
        ]
    );
}

#[test]
fn portfolio_trend_with_date_range_clips_weeks_and_does_not_lose_pre_start_flow() {
    let conn = open();
    seed_account(&conn, "acc-rng", "区间户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-rng", "600036", "招商银行", "CNY", "unknown");
    seed_price_history(&conn, "ph-r1", "inst-rng", "2026-04-06", 100_000, "CNY");
    seed_price_history(&conn, "ph-r2", "inst-rng", "2026-04-13", 200_000, "CNY");
    seed_price_history(&conn, "ph-r3", "inst-rng", "2026-04-20", 400_000, "CNY");
    // 买入在区间起点之前：起点前的流水必须累积带入，起点后各周数量才非零。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-rng",
            "inst-rng",
            3.0,
            50_000,
            "2026-04-08",
        ),
    )
    .unwrap();

    // 回归（#138 评审）：带 start_date 的组合走势查询曾因流水查询占位符
    // 与参数个数不匹配而运行时报错；此处同时锁定区间裁剪与起点前持仓带入。
    let trend = trend::query_portfolio_value_trend(
        &conn,
        &TrendRange {
            start_date: Some("2026-04-10".into()),
            end_date: Some("2026-04-21".into()),
        },
    )
    .unwrap();
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    assert_eq!(
        values,
        [
            ("2026-04-13".to_string(), 6000),  // 3 × 2000（起点前买入已带入）
            ("2026-04-20".to_string(), 12000), // 3 × 4000
        ]
    );
}

#[test]
fn portfolio_trend_converts_hkd_via_same_week_fx_with_reverse_fallback() {
    let conn = open();
    seed_account(&conn, "acc-hkd", "港美股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-hkd", "00700", "腾讯控股", "HKD", "unknown");
    // 港股以 HKD 计价（万分之一元刻度）：w1=100 HKD、w2=200 HKD、w3=300 HKD。
    seed_price_history(&conn, "ph-h1", "inst-hkd", "2026-03-02", 1_000_000, "HKD");
    seed_price_history(&conn, "ph-h2", "inst-hkd", "2026-03-09", 2_000_000, "HKD");
    seed_price_history(&conn, "ph-h3", "inst-hkd", "2026-03-16", 3_000_000, "HKD");
    // w1 有正向汇率 HKD->CNY=0.8；w2 只有反向 CNY->HKD=5.0（兜底取倒数 0.2）；w3 无任何历史汇率。
    seed_fx_rate_history(&conn, "fx-h1", "HKD", "CNY", "2026-03-03", 0.8);
    seed_fx_rate_history(&conn, "fx-h2", "CNY", "HKD", "2026-03-10", 5.0);
    // 2 股，全程持有（买入早于首条价格点）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-hkd",
            "inst-hkd",
            2.0,
            1_000_000,
            "2026-02-20",
        ),
    )
    .unwrap();

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(trend.currency_code, "CNY");
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    // w1: 2×10000×0.8=16000；w2: 2×20000×(1/5.0)=8000；w3 缺同期汇率 → 该周被跳过（不伪造数据）。
    assert_eq!(
        values,
        [
            ("2026-03-02".to_string(), 16000),
            ("2026-03-09".to_string(), 8000),
        ]
    );
}

#[test]
fn portfolio_trend_skips_weeks_missing_price_or_fx_but_keeps_other_contributors() {
    let conn = open();
    seed_account(&conn, "acc-mix", "混合户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-a", "600000", "浦发银行", "CNY", "unknown");
    seed_instrument(&conn, "inst-b", "09988", "阿里巴巴", "HKD", "unknown");
    // 各买 1 股，早于首条价格点。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-mix",
            "inst-a",
            1.0,
            100_000,
            "2026-02-20",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-mix",
            "inst-b",
            1.0,
            1_000_000,
            "2026-02-20",
        ),
    )
    .unwrap();
    // inst-a（CNY）三周全有价：10 元/周。
    seed_price_history(&conn, "ph-a1", "inst-a", "2026-03-02", 100_000, "CNY");
    seed_price_history(&conn, "ph-a2", "inst-a", "2026-03-09", 100_000, "CNY");
    seed_price_history(&conn, "ph-a3", "inst-a", "2026-03-16", 100_000, "CNY");
    // inst-b（HKD）w2 整周无价（停牌语义）；w3 有价但缺同期汇率。
    seed_price_history(&conn, "ph-b1", "inst-b", "2026-03-02", 1_000_000, "HKD");
    seed_price_history(&conn, "ph-b3", "inst-b", "2026-03-16", 1_000_000, "HKD");
    // 仅 w1 有 HKD->CNY=0.9。
    seed_fx_rate_history(&conn, "fx-m1", "HKD", "CNY", "2026-03-03", 0.9);

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    // w1: 1000 + 10000×0.9=10000；w2: inst-b 缺价被跳过，仅 inst-a 1000；w3: inst-b 缺汇率被跳过，仅 inst-a 1000。
    assert_eq!(
        values,
        [
            ("2026-03-02".to_string(), 10000),
            ("2026-03-09".to_string(), 1000),
            ("2026-03-16".to_string(), 1000),
        ]
    );
}

#[test]
fn trend_commands_return_empty_state_without_history() {
    let conn = open();
    seed_instrument(&conn, "inst-empty", "000002", "万科A", "CNY", "unknown");

    // 无任何价格历史：单标的与组合走势都返回空态结构（points 为空）。
    let trend =
        trend::query_instrument_price_trend(&conn, "inst-empty", &TrendRange::default()).unwrap();
    assert_eq!(trend.instrument_id, "inst-empty");
    assert!(trend.points.is_empty());

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(trend.currency_code, "CNY");
    assert!(trend.points.is_empty());
}

#[test]
fn portfolio_trend_excludes_soft_deleted_account_flow_including_history() {
    // 软删除账户口径（issue #247 / #217 定案 Q1「账户已删」）：组合走势逐期
    // 数量推算经时点持仓接缝排除软删账户的 buy/sell 流水——「今天」与历史
    // 周采样点全部剔除，与 v_holdings（空）对齐；删除/恢复经软删标志翻转
    // 自动进出推算，无时点存续状态。
    let conn = open();
    seed_account(&conn, "acc-sd", "待删户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-sd", "000001", "平安银行", "CNY", "unknown");
    // 周价格点（万分之一元）：w1=10 元、w2=20 元、w3=30 元（CNY，无需折算）。
    seed_price_history(&conn, "ph-s1", "inst-sd", "2026-02-02", 100_000, "CNY");
    seed_price_history(&conn, "ph-s2", "inst-sd", "2026-02-09", 200_000, "CNY");
    seed_price_history(&conn, "ph-s3", "inst-sd", "2026-02-16", 300_000, "CNY");
    // 早于首条价格点买入 10 股：软删前基线 = 各周 10 × 当周价。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sd",
            "inst-sd",
            10.0,
            150_000,
            "2026-01-20",
        ),
    )
    .unwrap();

    let values = |conn: &Connection| -> Vec<(String, i64)> {
        trend::query_portfolio_value_trend(conn, &TrendRange::default())
            .unwrap()
            .points
            .iter()
            .map(|p| (p.date.clone(), p.market_value_cents))
            .collect()
    };
    let baseline = values(&conn);
    assert_eq!(
        baseline,
        [
            ("2026-02-02".to_string(), 10000),
            ("2026-02-09".to_string(), 20000),
            ("2026-02-16".to_string(), 30000),
        ]
    );

    // 软删除账户（只翻 is_deleted 标志）：历史周采样点同样不再含该账户贡献。
    conn.execute("UPDATE accounts SET is_deleted=1 WHERE id='acc-sd'", [])
        .unwrap();
    assert_eq!(
        values(&conn),
        [
            ("2026-02-02".to_string(), 0),
            ("2026-02-09".to_string(), 0),
            ("2026-02-16".to_string(), 0),
        ]
    );

    // 绑定不变式（spec #168 定案第 6 条 / #217 Q4）：含软删夹具下
    // as-of「今天」≡ Holding——Holding 侧 v_holdings 无行，as-of 侧同日推算为 0。
    let holding_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM v_holdings WHERE instrument_id='inst-sd'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(holding_rows, 0, "Holding 侧：软删账户批次不入 v_holdings");
    let qty = holdings::holdings_as_of(&conn, Some("inst-sd"), "2026-06-01").unwrap();
    assert!((qty - 0.0).abs() < 1e-9, "as-of「今天」= {qty}");

    // 恢复账户（标志翻回）：流水自动回到推算，走势与基线逐点一致——口径可逆。
    conn.execute("UPDATE accounts SET is_deleted=0 WHERE id='acc-sd'", [])
        .unwrap();
    assert_eq!(values(&conn), baseline);
}

#[test]
fn portfolio_trend_keeps_hidden_account_flow() {
    // 隐藏 ≠ 删除（#217 定案 Q2）：隐藏账户（is_hidden）不是软删除，其 buy/sell
    // 流水照常计入逐期推算，与 v_holdings 不排隐藏账户一致。
    let conn = open();
    seed_account(&conn, "acc-hid", "隐藏户", "investment", "CNY", 0);
    conn.execute("UPDATE accounts SET is_hidden=1 WHERE id='acc-hid'", [])
        .unwrap();
    seed_instrument(&conn, "inst-hd", "600036", "招商银行", "CNY", "unknown");
    seed_price_history(&conn, "ph-hd1", "inst-hd", "2026-02-02", 100_000, "CNY");
    seed_price_history(&conn, "ph-hd2", "inst-hd", "2026-02-09", 200_000, "CNY");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-hid",
            "inst-hd",
            10.0,
            150_000,
            "2026-01-20",
        ),
    )
    .unwrap();

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    assert_eq!(
        values,
        [
            ("2026-02-02".to_string(), 10000),
            ("2026-02-09".to_string(), 20000),
        ]
    );
    // Holding 侧同口径：v_holdings 不排隐藏账户，持仓行仍在。
    let holding_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM v_holdings WHERE instrument_id='inst-hd'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(holding_rows, 1);
}

// ---------------------------------------------------------------------------
// 走势空态三态（ADR-0122 决策 5 / issue #1377）：补全中（带计数）/ 补全失败
// 待重试 / 无数据。判定消费派生事实（通道 + 历史）与后台补全的运行态快照
// （进程内全局态），状态触达用例以测试锁串行化，避免并行用例互踩同一份快照。
// ---------------------------------------------------------------------------

/// 运行态快照是进程级单例：触达它的用例先取这把锁（先例：共享单例的用例间
/// 串行化，与「每个用例独立数据库」不同层——锁只管快照，不管库）。
fn backfill_state_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

fn backfill_status(
    state: TrendBackfillState,
    done: Option<usize>,
    total: Option<usize>,
) -> TrendBackfillStatus {
    TrendBackfillStatus { state, done, total }
}

#[test]
fn instrument_trend_empty_carries_backfill_field_only_for_collectable_without_history() {
    let _guard = backfill_state_lock();
    let conn = open();

    // 行情通道标的（stock + sh + 6 位代码）无历史：空态带「补全中」——无在途
    // 轮次时的诚实缺省（后台任务会采它，派生事实与队列同源）。
    insert_instrument_with_market(
        &conn,
        "inst-bf-q",
        "600519",
        "贵州茅台",
        "CNY",
        "sh",
        "stock",
    );
    let trend =
        trend::query_instrument_price_trend(&conn, "inst-bf-q", &TrendRange::default()).unwrap();
    assert!(trend.points.is_empty());
    assert_eq!(
        trend.backfill,
        Some(backfill_status(TrendBackfillState::Running, None, None)),
        "有通道无历史 = 补全中（无在途轮次时无计数）"
    );

    // 有历史序列者无空态可言：区间裁剪导致的空不携带该字段。
    seed_price_history(
        &conn,
        "ph-bf-1",
        "inst-bf-q",
        "2026-01-05",
        1_000_000,
        "CNY",
    );
    let trend = trend::query_instrument_price_trend(
        &conn,
        "inst-bf-q",
        &TrendRange {
            start_date: Some("2027-01-01".into()),
            end_date: None,
        },
    )
    .unwrap();
    assert!(trend.points.is_empty());
    assert_eq!(trend.backfill, None, "有历史者的空（区间裁剪）不带补全状态");

    // 手动报价通道（other + unknown + 非代码形态）：不在补全面，空态不带字段
    //（前端按既有「录价」引导渲染）。
    insert_instrument_with_market(
        &conn,
        "inst-bf-m",
        "稳稳地幸福",
        "且慢组合",
        "CNY",
        "unknown",
        "other",
    );
    let trend =
        trend::query_instrument_price_trend(&conn, "inst-bf-m", &TrendRange::default()).unwrap();
    assert!(trend.points.is_empty());
    assert_eq!(trend.backfill, None, "手动通道不归后台补全");

    // 无来源（stock + unknown）：同样不带字段（既有「没有价格来源」边界说明）。
    insert_instrument_with_market(
        &conn,
        "inst-bf-n",
        "ghost1",
        "幽灵股票",
        "CNY",
        "unknown",
        "stock",
    );
    let trend =
        trend::query_instrument_price_trend(&conn, "inst-bf-n", &TrendRange::default()).unwrap();
    assert!(trend.points.is_empty());
    assert_eq!(trend.backfill, None, "无来源标的没有可采序列，不冒充补全中");
}

#[test]
fn instrument_trend_backfill_reflects_round_lifecycle_and_attempt_outcomes() {
    let _guard = backfill_state_lock();
    let conn = open();
    insert_fund_instrument(&conn, "inst-bf-f", "000001", "中国蓝图");

    // 一轮在途：补全中带计数，随标的级推进（与进度事件同口径）。
    backfill::round_started(2);
    let trend =
        trend::query_instrument_price_trend(&conn, "inst-bf-f", &TrendRange::default()).unwrap();
    assert_eq!(
        trend.backfill,
        Some(backfill_status(
            TrendBackfillState::Running,
            Some(0),
            Some(2)
        ))
    );
    backfill::round_progress(1);
    let trend =
        trend::query_instrument_price_trend(&conn, "inst-bf-f", &TrendRange::default()).unwrap();
    assert_eq!(
        trend.backfill,
        Some(backfill_status(
            TrendBackfillState::Running,
            Some(1),
            Some(2)
        ))
    );

    // 本轮尝试后仍无历史，结局 = 无数据（查无此码）：空态答「无数据」。
    backfill::record_attempt("inst-bf-f", backfill::BackfillAttempt::NoData);
    let trend =
        trend::query_instrument_price_trend(&conn, "inst-bf-f", &TrendRange::default()).unwrap();
    assert_eq!(
        trend.backfill,
        Some(backfill_status(TrendBackfillState::NoData, None, None))
    );

    // 结局 = 失败（网络错误 / 窗口不完整）：空态答「待重试」。
    backfill::record_attempt("inst-bf-f", backfill::BackfillAttempt::Failed);
    let trend =
        trend::query_instrument_price_trend(&conn, "inst-bf-f", &TrendRange::default()).unwrap();
    assert_eq!(
        trend.backfill,
        Some(backfill_status(
            TrendBackfillState::RetryPending,
            None,
            None
        ))
    );

    // 轮次收起后在途计数消失，但结局表保留——轮间窗口的空态依然可答。
    backfill::round_finished();
    let trend =
        trend::query_instrument_price_trend(&conn, "inst-bf-f", &TrendRange::default()).unwrap();
    assert_eq!(
        trend.backfill,
        Some(backfill_status(
            TrendBackfillState::RetryPending,
            None,
            None
        ))
    );

    // 新一轮开始：旧结局清空（队列按派生事实重收集），回「补全中」带新计数。
    backfill::round_started(1);
    let trend =
        trend::query_instrument_price_trend(&conn, "inst-bf-f", &TrendRange::default()).unwrap();
    assert_eq!(
        trend.backfill,
        Some(backfill_status(
            TrendBackfillState::Running,
            Some(0),
            Some(1)
        ))
    );
    backfill::round_finished();
}

#[test]
fn portfolio_trend_backfill_aggregates_pending_instruments() {
    let _guard = backfill_state_lock();
    let conn = open();
    insert_fund_instrument(&conn, "inst-pf-a", "110022", "易方达消费行业");
    insert_fund_instrument(&conn, "inst-pf-b", "000001", "中国蓝图混合");

    // 都未尝试：补全中（无在途轮次时无计数）。
    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert!(trend.points.is_empty());
    assert_eq!(
        trend.backfill,
        Some(backfill_status(TrendBackfillState::Running, None, None))
    );

    // 一只在途轮次中：带计数。
    backfill::round_started(2);
    backfill::round_progress(1);
    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(
        trend.backfill,
        Some(backfill_status(
            TrendBackfillState::Running,
            Some(1),
            Some(2)
        ))
    );
    backfill::round_finished();

    // 尝试后一只有数据、一只失败：任一待重试即「待重试」（对用户可行动）。
    backfill::record_attempt("inst-pf-a", backfill::BackfillAttempt::NoData);
    backfill::record_attempt("inst-pf-b", backfill::BackfillAttempt::Failed);
    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(
        trend.backfill,
        Some(backfill_status(
            TrendBackfillState::RetryPending,
            None,
            None
        ))
    );

    // 全部无可采：无数据（不再显示永远等不来的「补全中」）。
    backfill::record_attempt("inst-pf-b", backfill::BackfillAttempt::NoData);
    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(
        trend.backfill,
        Some(backfill_status(TrendBackfillState::NoData, None, None))
    );

    // 历史齐全者不进聚合：两基金都有历史后，字段消失（组合空态另有原因）。
    seed_price_history(&conn, "ph-pf-a", "inst-pf-a", "2026-01-05", 10_000, "CNY");
    seed_price_history(&conn, "ph-pf-b", "inst-pf-b", "2026-01-05", 10_000, "CNY");
    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(trend.backfill, None, "没有待补全标的不携带聚合状态");
}

// ---------------------------------------------------------------------------
// 恒定价格标的的读侧取值（ADR-0126 决策 6 / issue #1450）：响应内按常量合成，
// 历史表无行也出数；存量平坦序列不再被消费。「删除即变红」——删掉常量合成，
// 下方用例分别退化为空序列或吃到平坦行的错值。
// ---------------------------------------------------------------------------

/// 打上恒定标记（恒定 1.0000）。
fn mark_constant(conn: &Connection, instrument_id: &str) {
    conn.execute(
        "UPDATE instruments SET constant_unit_price = 10000 WHERE id = ?1",
        [instrument_id],
    )
    .unwrap();
}

/// 固定「今天」：常量合成的序列右界夹点（周一）。
fn fixed_today() -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 3, 9).unwrap()
}

#[test]
fn instrument_trend_for_constant_price_synthesizes_weekly_constant_line() {
    let conn = open();
    // 建档时刻 = FIXED_NOW（2026-01-01）→ 序列锚点 2026-01-01，首周周一 2025-12-29。
    insert_fund_instrument(&conn, "inst-const", "000198", "天弘余额宝");
    mark_constant(&conn, "inst-const");
    // 存量平坦序列（历史采集的遗留行）：值刻意偏离常量，读侧消费它即红。
    seed_price_history(&conn, "ph-flat", "inst-const", "2026-02-02", 20_000, "CNY");

    let trend = trend::query_instrument_price_trend_on(
        &conn,
        "inst-const",
        &TrendRange::default(),
        fixed_today(),
    )
    .unwrap();
    assert!(trend.backfill.is_none(), "恒定标的没有补全空态可言");
    // 周键序列：锚点周（2025-12-29）到今天（2026-03-09）共 11 周，价格恒 1.0000。
    let points: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.price_cents))
        .collect();
    assert_eq!(points.len(), 11, "每周一条常量点");
    assert!(
        points.iter().all(|(_, v)| *v == 10_000),
        "价格恒 1.0000，存量平坦行（20000）不被消费"
    );
    assert_eq!(
        points[0].0, "2026-01-04",
        "序列从建档锚点所在周开始（采样日为该周周日）"
    );
    assert_eq!(
        points.last().unwrap().0,
        "2026-03-09",
        "末点采样日不越过今天"
    );
    assert_eq!(trend.points[0].currency_code, "CNY");
}

#[test]
fn instrument_trend_constant_without_any_history_rows_is_not_empty() {
    let conn = open();
    insert_fund_instrument(&conn, "inst-const", "000198", "天弘余额宝");
    mark_constant(&conn, "inst-const");
    // 无任何价格历史行（新货基建档即打标，不再有采集面为它落行）。

    let trend = trend::query_instrument_price_trend_on(
        &conn,
        "inst-const",
        &TrendRange::default(),
        fixed_today(),
    )
    .unwrap();
    assert!(
        !trend.points.is_empty(),
        "货基走势由读侧常量合成，不因无历史行为空"
    );
    assert!(trend.backfill.is_none());
}

#[test]
fn instrument_trend_constant_respects_range_clipping() {
    let conn = open();
    insert_fund_instrument(&conn, "inst-const", "000198", "天弘余额宝");
    mark_constant(&conn, "inst-const");

    let trend = trend::query_instrument_price_trend_on(
        &conn,
        "inst-const",
        &TrendRange {
            start_date: Some("2026-02-04".into()),
            end_date: Some("2026-02-20".into()),
        },
        fixed_today(),
    )
    .unwrap();
    let dates: Vec<&str> = trend.points.iter().map(|p| p.date.as_str()).collect();
    // 含端点的周裁剪：起点周的采样日（02-08）落在区间内；终点周（02-16 起）
    // 的采样日被区间终点（02-20）夹住，不越过终点。
    assert_eq!(dates, ["2026-02-08", "2026-02-15", "2026-02-20"]);
}

#[test]
fn portfolio_trend_includes_constant_fund_contribution_without_price_rows() {
    let conn = open();
    seed_account(&conn, "acc-const", "货基户", "investment", "CNY", 0);
    insert_fund_instrument(&conn, "inst-const", "000198", "天弘余额宝");
    mark_constant(&conn, "inst-const");
    // 02-05（周三）申购 1000 份：组合市值 = 时点份额 × 常量 1.0000。
    let mut buy = make_trade_input(
        TransactionKind::Buy,
        "acc-const",
        "inst-const",
        1000.0,
        10_000,
        "2026-02-05",
    );
    // 基金申赎以确认单金额为权威（金额必填、单价反算不可携带）：
    // 1000 份 × 1.0000 = 1000 元。
    buy.amount_cents = 100_000;
    buy.price_cents = None;
    create_transaction_internal(&conn, buy).unwrap();
    // 无任何价格历史行——组合走势不因「只有恒定标的」而空。

    let trend = trend::query_portfolio_value_trend_on(&conn, &TrendRange::default(), fixed_today())
        .unwrap();
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    // 建档锚点周起每周一条：买入生效（02-08 采样日 ≥ 02-05）前为 0，之后
    // 1000 份 × 1.0000 = 1000 元 = 100000 分。
    assert_eq!(
        values,
        [
            ("2025-12-29".to_string(), 0),
            ("2026-01-05".to_string(), 0),
            ("2026-01-12".to_string(), 0),
            ("2026-01-19".to_string(), 0),
            ("2026-01-26".to_string(), 0),
            ("2026-02-02".to_string(), 100_000),
            ("2026-02-09".to_string(), 100_000),
            ("2026-02-16".to_string(), 100_000),
            ("2026-02-23".to_string(), 100_000),
            ("2026-03-02".to_string(), 100_000),
            ("2026-03-09".to_string(), 100_000),
        ]
    );
}

#[test]
fn portfolio_trend_constant_fund_ignores_flat_history_rows() {
    let conn = open();
    seed_account(&conn, "acc-const", "货基户", "investment", "CNY", 0);
    insert_fund_instrument(&conn, "inst-const", "000198", "天弘余额宝");
    mark_constant(&conn, "inst-const");
    let mut buy = make_trade_input(
        TransactionKind::Buy,
        "acc-const",
        "inst-const",
        1000.0,
        10_000,
        "2026-02-05",
    );
    // 基金申赎以确认单金额为权威（金额必填、单价反算不可携带）：
    // 1000 份 × 1.0000 = 1000 元。
    buy.amount_cents = 100_000;
    buy.price_cents = None;
    create_transaction_internal(&conn, buy).unwrap();
    // 存量平坦序列（值偏离常量）：既不能被消费（错值），也不能与常量合成行
    // 叠加（双重计入）。
    seed_price_history(&conn, "ph-flat", "inst-const", "2026-02-16", 20_000, "CNY");

    let trend = trend::query_portfolio_value_trend_on(&conn, &TrendRange::default(), fixed_today())
        .unwrap();
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    assert_eq!(
        values,
        [
            ("2025-12-29".to_string(), 0),
            ("2026-01-05".to_string(), 0),
            ("2026-01-12".to_string(), 0),
            ("2026-01-19".to_string(), 0),
            ("2026-01-26".to_string(), 0),
            ("2026-02-02".to_string(), 100_000),
            ("2026-02-09".to_string(), 100_000),
            ("2026-02-16".to_string(), 100_000,),
            ("2026-02-23".to_string(), 100_000),
            ("2026-03-02".to_string(), 100_000),
            ("2026-03-09".to_string(), 100_000),
        ],
        "平坦行（20000）既不被消费也不叠加，恒定标的按常量 1.0000 出数"
    );
}
