//! 标的字典步骤（issue #199）：搜索语义的 BDD 接缝。实现为
//! `commands::investment::list_instruments_internal`（与 IPC 命令同一实现）。
//! 另载按代码即拉添加基金的编排接缝（issue #301）：东财详情以注入桩离线驱动，
//! 实现为 `commands::investment::add_fund_by_code_with`（与 IPC 命令同一套
//! 校验/拉取编排/落库实现，网络层经注入替换）。

use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::commands::investment::{add_fund_by_code_with, list_instruments_internal};
use tauri_app_lib::db::{device_id, new_uuid, now_iso};
use tauri_app_lib::error::Result;
use tauri_app_lib::models::{FundDetail, FundNav, InstrumentListFilter};

use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

/// 直接插入金融工具字典行（投资域字典，可指定中文名称供拼音语义场景使用）。
#[given(expr = "存在标的 {string} 名称 {string} 币种 {string}")]
fn create_instrument_named(
    world: &mut LedgerWorld,
    symbol: String,
    name: String,
    currency: String,
) {
    let now = now_iso();
    world_conn!(world)
        .execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,'stock',?3,?4,'unknown',?5,?5,1,?6)",
            params![new_uuid(), symbol, name, currency, now, device_id()],
        )
        .unwrap();
}

/// 直接插入指定类型的金融工具字典行（同码异类型消歧场景用，issue #294）。
#[given(expr = "存在类型 {string} 的标的 {string} 名称 {string} 币种 {string}")]
fn create_instrument_of_type(
    world: &mut LedgerWorld,
    kind: String,
    symbol: String,
    name: String,
    currency: String,
) {
    let now = now_iso();
    world_conn!(world)
        .execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,?3,?4,?5,'unknown',?6,?6,1,?7)",
            params![new_uuid(), symbol, kind, name, currency, now, device_id()],
        )
        .unwrap();
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

#[when(expr = "搜索标的 {string}")]
fn search_instruments(world: &mut LedgerWorld, query: String) {
    let filter = InstrumentListFilter {
        search: Some(query),
        ..Default::default()
    };
    world.last_instrument_search =
        Some(list_instruments_internal(&world_conn!(world), &filter).expect("标的搜索失败"));
}

/// 按类型过滤搜索（同码异类型消歧语义，issue #294；与 HTTP 端点的 type 参数同一接缝）。
#[when(expr = "搜索类型 {string} 的标的 {string}")]
fn search_instruments_of_kind(world: &mut LedgerWorld, kind: String, query: String) {
    let filter = InstrumentListFilter {
        search: Some(query),
        kind: Some(kind.parse().expect("未知金融工具类型")),
        ..Default::default()
    };
    world.last_instrument_search =
        Some(list_instruments_internal(&world_conn!(world), &filter).expect("标的搜索失败"));
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "标的搜索命中 {int} 条 总数 {int}")]
fn assert_instrument_search(world: &mut LedgerWorld, items: usize, total: i64) {
    let result = world
        .last_instrument_search
        .as_ref()
        .expect("未执行标的搜索");
    assert_eq!(result.items.len(), items, "命中条数不符：{result:?}");
    assert_eq!(result.total, total, "命中总数不符：{result:?}");
}

#[then(expr = "标的搜索首个结果代码为 {string}")]
fn assert_instrument_first_symbol(world: &mut LedgerWorld, symbol: String) {
    let result = world
        .last_instrument_search
        .as_ref()
        .expect("未执行标的搜索");
    assert_eq!(
        result.items.first().map(|i| i.symbol.as_str()),
        Some(symbol.as_str()),
        "首个结果代码不符：{result:?}"
    );
}

// ---------------------------------------------------------------------------
// 按代码即拉添加基金（issue #301 / ADR-0038）：When 注入桩驱动编排接缝
// ---------------------------------------------------------------------------

/// 驱动添加基金编排接缝并把结果/错误记入 world（供 Then 断言）。
/// 获取函数收到请求代码时须全等（桩只对目标代码返回详情）。
fn run_add_fund<F>(world: &mut LedgerWorld, code: String, fetch: F)
where
    F: FnMut(&str) -> Result<FundDetail>,
{
    let mut fetch = fetch;
    let outcome = add_fund_by_code_with(&world_conn!(world), &code, &mut fetch);
    match outcome {
        Ok(result) => {
            world.last_add_fund = Some(result);
            world.last_error = None;
        }
        Err(e) => {
            world.last_add_fund = None;
            world.last_error = Some(e.to_string());
        }
    }
}

#[when(
    expr = "按代码添加基金 {string} 东财返回名称 {string} 分类 {string} 净值 {float} 净值日期 {string}"
)]
fn add_fund_with_stub_detail(
    world: &mut LedgerWorld,
    code: String,
    name: String,
    fund_class: String,
    nav: f64,
    nav_date: String,
) {
    let detail = FundDetail {
        code: code.clone(),
        name,
        fund_class,
        nav: Some(FundNav { nav, nav_date }),
    };
    run_add_fund(world, code, move |requested: &str| {
        assert_eq!(requested, detail.code, "获取函数应收到请求代码");
        Ok(detail.clone())
    });
}

#[when(expr = "按代码添加基金 {string} 东财返回名称 {string} 分类 {string} 未取到净值")]
fn add_fund_with_stub_no_nav(
    world: &mut LedgerWorld,
    code: String,
    name: String,
    fund_class: String,
) {
    let detail = FundDetail {
        code: code.clone(),
        name,
        fund_class,
        nav: None,
    };
    run_add_fund(world, code, move |requested: &str| {
        assert_eq!(requested, detail.code, "获取函数应收到请求代码");
        Ok(detail.clone())
    });
}

#[when(expr = "按代码添加基金 {string} 东财查无此码")]
fn add_fund_with_stub_not_found(world: &mut LedgerWorld, code: String) {
    let mut fetch = |requested: &str| -> Result<FundDetail> {
        Err(tauri_app_lib::error::AppError::Invalid(format!(
            "查无基金代码 {requested}，请核对后重试",
        )))
    };
    let outcome = add_fund_by_code_with(&world_conn!(world), &code, &mut fetch);
    match outcome {
        Ok(_) => panic!("查无此码应报错而非成功"),
        Err(e) => {
            world.last_add_fund = None;
            world.last_error = Some(e.to_string());
        }
    }
}

// ---------------------------------------------------------------------------
// Then：标的字典行 / 现价缓存 / 错误
// ---------------------------------------------------------------------------

#[then(expr = "标的字典存在类型 {string} 代码 {string} 名称 {string} 来源 {string} 市场 {string}")]
fn assert_instrument_row(
    world: &mut LedgerWorld,
    kind: String,
    symbol: String,
    name: String,
    source: String,
    market: String,
) {
    let row: Option<(String, String, String)> = world_conn!(world)
        .query_row(
            "SELECT name, source, market FROM instruments \
             WHERE symbol=?1 AND instrument_type=?2",
            params![symbol, kind],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    let (actual_name, actual_source, actual_market) =
        row.unwrap_or_else(|| panic!("标的 {symbol}（{kind}）应存在"));
    assert_eq!(actual_name, name, "名称不符");
    assert_eq!(actual_source, source, "来源不符");
    assert_eq!(actual_market, market, "市场不符");
}

#[then(expr = "标的字典中 {string} 类型标的共 {int} 条")]
fn assert_instrument_kind_count(world: &mut LedgerWorld, kind: String, count: i64) {
    let actual: i64 = world_conn!(world)
        .query_row(
            "SELECT COUNT(*) FROM instruments WHERE instrument_type=?1",
            params![kind],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(actual, count, "{kind} 类型标的条数不符");
}

#[then(expr = "标的 {string} 现价为 {int} 币种 {string} 净值日期 {string}")]
fn assert_fund_market_price(
    world: &mut LedgerWorld,
    symbol: String,
    price_cents: i64,
    currency: String,
    nav_date: String,
) {
    let row: (i64, String, String, Option<String>) = world_conn!(world)
        .query_row(
            "SELECT p.price_cents, p.currency_code, p.priced_at, p.nav_date \
             FROM market_prices p JOIN instruments i ON i.id = p.instrument_id \
             WHERE i.symbol=?1 AND i.instrument_type='fund'",
            params![symbol],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap_or_else(|e| panic!("基金 {symbol} 应有现价（{e}）"));
    assert_eq!(row.0, price_cents, "现价（万分之一元）不符");
    assert_eq!(row.1, currency, "币种不符");
    // 现价的行情日期 = 净值日期（单位净值即价格，ADR-0038 决策 3），两处同源断言。
    assert_eq!(row.2, nav_date, "priced_at 应为净值日期");
    assert_eq!(row.3.as_deref(), Some(nav_date.as_str()), "nav_date 不符");
}

#[then(expr = "标的 {string} 无现价")]
fn assert_fund_no_market_price(world: &mut LedgerWorld, symbol: String) {
    let count: i64 = world_conn!(world)
        .query_row(
            "SELECT COUNT(*) FROM market_prices p JOIN instruments i ON i.id = p.instrument_id \
             WHERE i.symbol=?1 AND i.instrument_type='fund'",
            params![symbol],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "基金 {symbol} 不应有现价缓存");
}

#[then(expr = "添加基金应返回错误 {string}")]
fn assert_add_fund_error(world: &mut LedgerWorld, fragment: String) {
    let error = world
        .last_error
        .as_ref()
        .unwrap_or_else(|| panic!("添加基金应失败但未记录错误"));
    assert!(
        error.contains(&fragment),
        "错误「{error}」应包含「{fragment}」"
    );
}
