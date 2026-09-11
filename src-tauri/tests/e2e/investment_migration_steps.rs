//! 投资迁移链路 e2e 步骤（issue #297 / ADR-0037）。
//!
//! 端到端固化 AI 投资迁移链路：搜索无命中 → 幂等创建标的 → 批量导入 buy/sell →
//! 持仓 / 余额读回核对（旅程终态，ADR-0087 决策 4；中间域细节不展开）。各接缝与对外入口同一实现：
//!
//! - **标的创建**：`investment::create_instrument`——与创建端点同一核心接缝
//!   （find-or-create 幂等；币种推导在 HTTP handler 层，已有集成测试钉住，
//!   本层显式传币种不重复验证）。
//! - **买卖写入**：`batch::TransactionBatch::run`（dedup=true）——与 HTTP 批量
//!   导入端点同一编排（先例：迁移验证步骤「批量导入交易」）；行金额占位 0，
//!   交易行金额由行为层 prepare 按「数量 × 单价 ± 手续费」重算（CONTEXT-investment
//!   TransactionTrade 词条），与 AI 实际提交形状一致。
//!
//! 持仓断言直查投资域扩展表（先例：instruments_steps 直插直查；每份成本等
//! 域细节权威在域单测 trade.rs，此处不展开）；
//! 余额读回复用迁移验证步骤的 `查询全部账户余额` / `账户 … 余额应为 …`。

use cucumber::gherkin::Step;
use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::investment::{InstrumentInput, InstrumentType, create_instrument};
use tauri_app_lib::transaction::TransactionBatch;
use tauri_app_lib::transaction::TransactionInput;
use tauri_app_lib::transaction::amount::TransactionKind;

use crate::common::instrument_id_by_symbol;
use crate::step_inputs::trade_input;
use crate::world::LedgerWorld;

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
    let kind = InstrumentType::parse(&kind).expect("未知金融工具类型");
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
        // L1 买卖工厂：行金额置零（prepare 按「数量 × 单价 ± 手续费」重算，
        // 提交值不被采信）、单价进 wire；币种/手续费为冷字段经结构体更新覆盖。
        inputs.push(TransactionInput {
            currency_code,
            fee_cents: Some(get(row, "手续费").parse().expect("手续费必须是整数")),
            ..trade_input(
                kind,
                &instrument_id,
                get(row, "数量").parse().expect("数量必须是数字"),
                get(row, "单价").parse().expect("单价必须是整数"),
                &account_id,
                &get(row, "日期"),
            )
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

    world.txn.last_batch_results = results;
}

// ---------------------------------------------------------------------------
// Then：导入结果与持仓读回
// ---------------------------------------------------------------------------

#[then(expr = "导入的投资交易应有 {int} 行全部成功")]
fn assert_imported_trades_all_success(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(
        world.txn.last_batch_results.len(),
        expected,
        "导入结果行数不符"
    );
    for r in &world.txn.last_batch_results {
        assert!(r.success, "导入行应成功: {r:?}");
        assert!(!r.duplicate, "首次导入不应命中重复: {r:?}");
        assert!(r.id.is_some(), "成功行应返回交易 id: {r:?}");
    }
}

/// 持仓读回（旅程终态，ADR-0087 决策 4）：剩余数量实时聚合。每份成本、
/// 已实现盈亏等域细节权威在域单测（investment/tests/trade.rs、pnl.rs），不展开。
#[then(expr = "标的 {string} 持仓应为 {float}")]
fn assert_holding(world: &mut LedgerWorld, symbol: String, quantity: f64) {
    let qty: f64 = {
        let conn = world_conn!(world);
        conn.query_row(
            "SELECT COALESCE(SUM(l.remaining_quantity), 0.0) \
             FROM security_lots l \
             JOIN instruments i ON i.id = l.instrument_id \
             JOIN accounts a ON a.id = l.account_id \
             WHERE i.symbol=?1 AND a.is_deleted=0 AND l.remaining_quantity > 0",
            params![symbol],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert!(
        (qty - quantity).abs() < 1e-9,
        "标的 {symbol} 持仓数量不符：期望 {quantity}，实际 {qty}"
    );
}
