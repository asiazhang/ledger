//! 投资迁移链路 e2e 步骤（issue #297 / ADR-0037）。
//!
//! 端到端固化 AI 投资迁移链路：搜索无命中 → 幂等创建标的 → 批量导入 buy/sell →
//! 持仓批次 / 已实现盈亏 / 余额读回核对。各接缝与对外入口同一实现：
//!
//! - **标的创建**：`investment::create_instrument`——与创建端点同一核心接缝
//!   （find-or-create 幂等；币种推导在 HTTP handler 层，已有集成测试钉住，
//!   本层显式传币种不重复验证）。
//! - **买卖写入**：`batch::TransactionBatch::run`（dedup=true）——与 HTTP 批量
//!   导入端点同一编排（先例：迁移验证步骤「批量导入交易」）；行金额占位 0，
//!   交易行金额由行为层 prepare 按「数量 × 单价 ± 手续费」重算（CONTEXT-investment
//!   TransactionTrade 词条），与 AI 实际提交形状一致。
//! - **批次顺序锚定**：`now_iso` 精度为秒，同批连续落库的买入其批次
//!   `created_at` 相同、FIFO 将退化为 uuid 随机序——按导入先后回填递增
//!   `created_at` 确定性化（夹具手段，先例：投资域单测 trade.rs）。
//!
//! 持仓 / 盈亏断言直查投资域扩展表（先例：instruments_steps 直插直查）；
//! 余额读回复用迁移验证步骤的 `查询全部账户余额` / `账户 … 余额应为 …`。

use cucumber::gherkin::Step;
use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::commands::batch::TransactionBatch;
use tauri_app_lib::investment::create_instrument;
use tauri_app_lib::models::{InstrumentInput, InstrumentType, TransactionInput};
use tauri_app_lib::transaction::amount::TransactionKind;

use crate::world::LedgerWorld;

/// 按标的代码查 instrument id（先行「幂等创建标的」步骤落库，必存在）。
fn instrument_id_by_symbol(conn: &rusqlite::Connection, symbol: &str) -> String {
    conn.query_row(
        "SELECT id FROM instruments WHERE symbol=?1",
        params![symbol],
        |r| r.get(0),
    )
    .unwrap_or_else(|_| panic!("标的不存在，先执行幂等创建标的步骤: {symbol}"))
}

// ---------------------------------------------------------------------------
// When：幂等创建标的（创建端点同一核心接缝）
// ---------------------------------------------------------------------------

#[when(expr = "幂等创建标的 {string} 类型 {string} 名称 {string} 市场 {string} 币种 {string}")]
fn create_instrument_idempotently(
    world: &mut LedgerWorld,
    symbol: String,
    kind: String,
    name: String,
    market: String,
    currency: String,
) {
    let kind: InstrumentType = kind.parse().expect("未知金融工具类型");
    create_instrument(
        &world_conn!(world),
        InstrumentInput {
            symbol,
            kind,
            name: Some(name),
            currency_code: currency,
            market: Some(market),
        },
    )
    .expect("幂等创建标的失败");
}

// ---------------------------------------------------------------------------
// When：批量导入投资交易（与 HTTP 批量导入同一编排入口）
// ---------------------------------------------------------------------------

/// 表格列（按表头名解析，缺失可省略）：kind | 标的 | 数量 | 单价 | 手续费 | 账户 | 日期。
/// 行金额占位 0：交易行金额由行为层 prepare 按数量×单价±手续费重算（AI 无需自行计算）。
#[when(expr = "批量导入投资交易")]
fn batch_import_trades(world: &mut LedgerWorld, #[step] step: &Step) {
    let table = step.table.as_ref().expect("批量导入投资交易步骤缺少数据表");
    let headers = &table.rows[0];
    let col = |name: &str| headers.iter().position(|h| h == name);
    let get = |row: &[String], name: &str| {
        col(name)
            .and_then(|i| row.get(i).cloned())
            .unwrap_or_default()
    };

    let mut inputs: Vec<TransactionInput> = Vec::new();
    let mut kinds: Vec<TransactionKind> = Vec::new();
    for row in table.rows.iter().skip(1) {
        let symbol = get(row, "标的");
        let account_name = get(row, "账户");
        let kind = get(row, "kind");
        let kind =
            TransactionKind::parse(&kind).unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）"));
        let (instrument_id, account_id, currency_code) = {
            let conn = world_conn!(world);
            let instrument_id = instrument_id_by_symbol(&conn, &symbol);
            let account_id = world.account_id(&account_name);
            let currency_code: String = conn
                .query_row(
                    "SELECT currency_code FROM accounts WHERE id=?1",
                    params![account_id],
                    |r| r.get(0),
                )
                .unwrap_or_else(|_| panic!("账户不存在: {account_name}"));
            (instrument_id, account_id, currency_code)
        };
        kinds.push(kind);
        inputs.push(TransactionInput {
            kind,
            // 占位金额：prepare 按「数量 × 单价 ± 手续费」重算交易行金额
            amount_cents: 0,
            currency_code,
            account_id,
            to_account_id: None,
            category_id: None,
            merchant_id: None,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: get(row, "日期"),
            instrument_id: Some(instrument_id),
            quantity: Some(get(row, "数量").parse().expect("数量必须是数字")),
            price_cents: Some(get(row, "单价").parse().expect("单价必须是整数")),
            fee_cents: Some(get(row, "手续费").parse().expect("手续费必须是整数")),
            idempotency_key: None,
        });
    }
    let count = inputs.len();

    // 与 HTTP 批量导入端点同形态：经连接层统一写入口（ADR-0032）。
    let results = world
        .db
        .write(|conn| TransactionBatch::run(conn, inputs, true))
        .expect("批量导入投资交易失败")
        .results;
    assert_eq!(results.len(), count, "导入结果行数应与提交行数一致");

    // buy 行交易 id 按导入先后累积（「持仓批次按导入先后锚定顺序」步骤的输入）。
    for (kind, result) in kinds.iter().zip(&results) {
        if let (TransactionKind::Buy, Some(id)) = (kind, &result.id) {
            world.imported_buy_txn_ids.push(id.clone());
        }
    }
    world.last_batch_results = results;
}

/// 回填批次 created_at 锚定「先导入先消耗」的确定性顺序（夹具手段，见模块文档）。
#[when(expr = "持仓批次按导入先后锚定顺序")]
fn anchor_lot_order(world: &mut LedgerWorld) {
    let ids = world.imported_buy_txn_ids.clone();
    let conn = world_conn!(world);
    for (i, txn_id) in ids.iter().enumerate() {
        // 分/秒进位避免分钟数溢出（批次数不受 60 限制）
        let created_at = format!("2026-01-01T00:{:02}:{:02}Z", i / 60, i % 60);
        conn.execute(
            "UPDATE security_lots SET created_at=?1 WHERE buy_transaction_id=?2",
            params![created_at, txn_id],
        )
        .unwrap_or_else(|e| panic!("锚定批次顺序失败（txn {txn_id}）: {e}"));
    }
}

// ---------------------------------------------------------------------------
// Then：导入结果与持仓/盈亏读回
// ---------------------------------------------------------------------------

#[then(expr = "导入的投资交易应有 {int} 行全部成功")]
fn assert_imported_trades_all_success(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(world.last_batch_results.len(), expected, "导入结果行数不符");
    for r in &world.last_batch_results {
        assert!(r.success, "导入行应成功: {r:?}");
        assert!(!r.duplicate, "首次导入不应命中重复: {r:?}");
        assert!(r.id.is_some(), "成功行应返回交易 id: {r:?}");
    }
}

/// 持仓读回：剩余数量实时聚合 + 剩余批次的加权平均每份成本。
#[then(expr = "标的 {string} 持仓应为 {float} 每份成本 {int}")]
fn assert_holding(world: &mut LedgerWorld, symbol: String, quantity: f64, cost_per_unit: i64) {
    let (qty, avg_cost): (f64, i64) = {
        let conn = world_conn!(world);
        conn.query_row(
            "SELECT COALESCE(SUM(l.remaining_quantity), 0.0), \
                    COALESCE(SUM(l.remaining_quantity * l.cost_per_unit_cents), 0.0) / SUM(l.remaining_quantity) \
             FROM security_lots l \
             JOIN instruments i ON i.id = l.instrument_id \
             JOIN accounts a ON a.id = l.account_id \
             WHERE i.symbol=?1 AND a.is_deleted=0 AND l.remaining_quantity > 0",
            params![symbol],
            |r| Ok((r.get(0)?, r.get::<_, f64>(1)?.round() as i64)),
        )
        .unwrap()
    };
    assert!(
        (qty - quantity).abs() < 1e-9,
        "标的 {symbol} 持仓数量不符：期望 {quantity}，实际 {qty}"
    );
    assert_eq!(
        avg_cost, cost_per_unit,
        "标的 {symbol} 加权平均每份成本不符"
    );
}

/// 已实现盈亏读回：该标的所有卖出匹配记录的盈亏合计（FIFO 消耗 + 手续费分摊的净结果）。
#[then(expr = "标的 {string} 已实现盈亏应为 {int}")]
fn assert_realized_pnl(world: &mut LedgerWorld, symbol: String, expected: i64) {
    let total: i64 = {
        let conn = world_conn!(world);
        conn.query_row(
            "SELECT COALESCE(SUM(sls.realized_pnl_cents), 0) \
             FROM security_lot_sales sls \
             JOIN security_lots l ON l.id = sls.lot_id \
             JOIN instruments i ON i.id = l.instrument_id \
             WHERE i.symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(
        total, expected,
        "标的 {symbol} 已实现盈亏不符（FIFO 消耗与手续费分摊的净结果）"
    );
}
