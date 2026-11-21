use cucumber::{given, then, when};
use rusqlite::params;

use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::Result;
use ledger_investment::{
    InstrumentInput, InstrumentListFilter, InstrumentType, Quote, StockRoute,
    add_fund_by_code_with, add_stock_instrument_with_quote, create_instrument_manual,
    delete_instrument as delete_instrument_domain, fetch_stock_quote_for_add, list_instruments,
    prices::price_value_to_cents,
};

use crate::world::LedgerWorld;

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
            params![new_uuid(), symbol, name, currency, now, "e2e-fixture"],
        )
        .unwrap();
}

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
            params![new_uuid(), symbol, kind, name, currency, now, "e2e-fixture"],
        )
        .unwrap();
}

#[given(expr = "存在市场 {string} 的标的 {string} 名称 {string} 币种 {string}")]
fn create_instrument_with_market(
    world: &mut LedgerWorld,
    market: String,
    symbol: String,
    name: String,
    currency: String,
) {
    let now = now_iso();
    world_conn!(world)
        .execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,'stock',?3,?4,?5,?6,?6,1,?7)",
            params![new_uuid(), symbol, name, currency, market, now, "e2e-fixture"],
        )
        .unwrap();
}

#[when(expr = "手动创建标的 {string} 类型 {string} 名称 {string} 币种 {string}")]
fn manual_create_instrument(
    world: &mut LedgerWorld,
    symbol: String,
    kind: String,
    name: String,
    currency: String,
) {
    let input = InstrumentInput {
        symbol,
        kind: InstrumentType::parse(&kind).expect("未知金融工具类型"),
        name: Some(name),
        currency_code: currency,
        market: None,
    };
    match create_instrument_manual(&world_conn!(world), input) {
        Ok(_) => world.last_error = None,
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

#[when(expr = "删除标的 {string}")]
fn delete_instrument(world: &mut LedgerWorld, symbol: String) {
    let id: String = world_conn!(world)
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| panic!("删除标的步骤：标的 {symbol} 应已存在"));
    match delete_instrument_domain(&world_conn!(world), &id) {
        Ok(_) => world.last_error = None,
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

#[when(expr = "列出全部标的")]
fn list_all_instruments(world: &mut LedgerWorld) {
    world.asset.last_instrument_search = Some(
        list_instruments(&world_conn!(world), &InstrumentListFilter::default())
            .expect("标的列表查询失败"),
    );
}

#[when(expr = "搜索标的 {string}")]
fn search_instruments(world: &mut LedgerWorld, query: String) {
    let filter = InstrumentListFilter {
        search: Some(query),
        ..Default::default()
    };
    world.asset.last_instrument_search =
        Some(list_instruments(&world_conn!(world), &filter).expect("标的搜索失败"));
}

#[when(expr = "搜索类型 {string} 的标的 {string}")]
fn search_instruments_of_kind(world: &mut LedgerWorld, kind: String, query: String) {
    let filter = InstrumentListFilter {
        search: Some(query),
        kind: Some(InstrumentType::parse(&kind).expect("未知金融工具类型")),
        ..Default::default()
    };
    world.asset.last_instrument_search =
        Some(list_instruments(&world_conn!(world), &filter).expect("标的搜索失败"));
}

#[then(expr = "标的搜索命中 {int} 条 总数 {int}")]
fn assert_instrument_search(world: &mut LedgerWorld, items: usize, total: i64) {
    let result = world
        .asset
        .last_instrument_search
        .as_ref()
        .expect("未执行标的搜索");
    assert_eq!(result.items.len(), items, "命中条数不符：{result:?}");
    assert_eq!(result.total, total, "命中总数不符：{result:?}");
}

#[then(expr = "标的搜索首个结果代码为 {string}")]
fn assert_instrument_first_symbol(world: &mut LedgerWorld, symbol: String) {
    let result = world
        .asset
        .last_instrument_search
        .as_ref()
        .expect("未执行标的搜索");
    assert_eq!(
        result.items.first().map(|i| i.symbol.as_str()),
        Some(symbol.as_str()),
        "首个结果代码不符：{result:?}"
    );
}

#[then(expr = "标的列表代码 {string} 来源应为 {string}")]
fn assert_instrument_list_source(world: &mut LedgerWorld, symbol: String, source: String) {
    let result = world
        .asset
        .last_instrument_search
        .as_ref()
        .expect("未执行标的列表查询");
    let item = result
        .items
        .iter()
        .find(|i| i.symbol == symbol)
        .unwrap_or_else(|| panic!("标的列表应含代码 {symbol}：{:?}", result.items));
    assert_eq!(item.source, source, "标的 {symbol} 来源不符");
}

#[then(expr = "标的列表共 {int} 条")]
fn assert_instrument_list_total(world: &mut LedgerWorld, total: usize) {
    let result = world
        .asset
        .last_instrument_search
        .as_ref()
        .expect("未执行标的列表查询");
    assert_eq!(
        result.items.len(),
        total,
        "标的列表条数不符：{:?}",
        result.items
    );
    assert_eq!(result.total, total as i64, "标的列表总数不符");
}

#[then(expr = "手动创建标的应返回错误 {string}")]
fn assert_manual_create_error(world: &mut LedgerWorld, fragment: String) {
    let error = world
        .last_error
        .as_ref()
        .unwrap_or_else(|| panic!("手动创建标的应失败但未记录错误"));
    assert!(
        error.contains(&fragment),
        "错误「{error}」应包含「{fragment}」"
    );
}

#[then(expr = "删除标的应返回错误 {string}")]
fn assert_delete_instrument_error(world: &mut LedgerWorld, fragment: String) {
    let error = world
        .last_error
        .as_ref()
        .unwrap_or_else(|| panic!("删除标的应失败但未记录错误"));
    assert!(
        error.contains(&fragment),
        "错误「{error}」应包含「{fragment}」"
    );
}

#[when(expr = "以 {string} 为锚点查询标的列表")]
fn locate_instrument_page(world: &mut LedgerWorld, symbol: String) {
    let id: String = world_conn!(world)
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| panic!("锚点定位：标的 {symbol} 应已存在"));
    query_with_anchor(world, Some(id));
}

#[when(expr = "以未知 id 为锚点查询标的列表")]
fn locate_instrument_page_unknown(world: &mut LedgerWorld) {
    query_with_anchor(world, Some("inst-unknown-id".to_string()));
}

fn query_with_anchor(world: &mut LedgerWorld, anchor_id: Option<String>) {
    let filter = InstrumentListFilter {
        anchor_id,
        ..Default::default()
    };
    match list_instruments(&world_conn!(world), &filter) {
        Ok(result) => {
            world.asset.last_instrument_list = Some(result);
            world.last_error = None;
        }
        Err(e) => {
            world.asset.last_instrument_list = None;
            world.last_error = Some(e.to_string());
        }
    }
}

#[then(expr = "锚点应落在第 {int} 页")]
fn assert_anchor_page(world: &mut LedgerWorld, expected: usize) {
    let result = world
        .asset
        .last_instrument_list
        .as_ref()
        .expect("锚点定位查询应已执行");
    assert_eq!(
        result.anchor_page,
        Some(expected),
        "锚点页码不符：{result:?}"
    );
}

#[then(expr = "锚点应落空（不携带所在页）")]
fn assert_anchor_page_absent(world: &mut LedgerWorld) {
    let result = world
        .asset
        .last_instrument_list
        .as_ref()
        .expect("锚点定位查询应已执行");
    assert_eq!(
        result.anchor_page, None,
        "未知锚点不应携带所在页：{result:?}"
    );
}

#[then(expr = "锚点行 {string} 应在返回页内")]
fn assert_anchor_row_in_page(world: &mut LedgerWorld, symbol: String) {
    let result = world
        .asset
        .last_instrument_list
        .as_ref()
        .expect("锚点定位查询应已执行");
    assert!(
        result.items.iter().any(|inst| inst.symbol == symbol),
        "锚点行 {symbol} 应在返回页内：{:?}",
        result.items.iter().map(|i| &i.symbol).collect::<Vec<_>>()
    );
}

fn run_add_fund<F>(world: &mut LedgerWorld, code: String, fetch: F)
where
    F: FnMut(&str, &str) -> Result<Quote>,
{
    let mut fetch = fetch;
    let outcome = add_fund_by_code_with(&world_conn!(world), &code, &mut fetch);
    match outcome {
        Ok(_) => world.last_error = None,
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

#[when(expr = "按代码添加基金 {string} 行情源返回名称 {string} 净值 {float} 净值日期 {string}")]
fn add_fund_with_stub_detail(
    world: &mut LedgerWorld,
    code: String,
    name: String,
    nav: f64,
    nav_date: String,
) {
    let quote = Quote {
        code: code.clone(),
        name,
        price_cents: Some(price_value_to_cents(nav)),
        price_date: Some(nav_date.clone()),
        market: None,
        kind_hint: None,
        fund_class: None,
        nav_date: Some(nav_date),
        constant_unit_price_cents: None,
        price_source: ledger_investment::prices::SINA_PRICE_SOURCE,
    };
    run_add_fund(world, code, move |requested: &str, _market: &str| {
        assert_eq!(requested, quote.code, "获取函数应收到请求代码");
        Ok(quote.clone())
    });
}

#[when(expr = "按代码添加基金 {string} 行情源返回名称 {string} 未取到净值")]
fn add_fund_with_stub_no_nav(world: &mut LedgerWorld, code: String, name: String) {
    let quote = Quote {
        code: code.clone(),
        name,
        price_cents: None,
        price_date: None,
        market: None,
        kind_hint: None,
        fund_class: None,
        nav_date: None,
        constant_unit_price_cents: None,
        price_source: ledger_investment::prices::SINA_PRICE_SOURCE,
    };
    run_add_fund(world, code, move |requested: &str, _market: &str| {
        assert_eq!(requested, quote.code, "获取函数应收到请求代码");
        Ok(quote.clone())
    });
}

#[when(expr = "按代码添加基金 {string} 行情源查无此码")]
fn add_fund_with_stub_not_found(world: &mut LedgerWorld, code: String) {
    let mut fetch = |requested: &str, _market: &str| -> Result<Quote> {
        Err(ledger_infra::error::AppError::Invalid(format!(
            "查无基金代码 {requested}，请核对后重试",
        )))
    };
    let outcome = add_fund_by_code_with(&world_conn!(world), &code, &mut fetch);
    match outcome {
        Ok(_) => panic!("查无此码应报错而非成功"),
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

async fn run_add_instrument<F, Fut>(
    world: &mut LedgerWorld,
    channel: String,
    code: String,
    fetch: &mut F,
) where
    F: FnMut(&str, StockRoute) -> Fut,
    Fut: std::future::Future<Output = Result<Quote>>,
{
    let quote = match fetch_stock_quote_for_add(&channel, &code, fetch).await {
        Ok(quote) => quote,
        Err(e) => {
            world.last_error = Some(e.to_string());
            return;
        }
    };
    match add_stock_instrument_with_quote(&world_conn!(world), &quote) {
        Ok(_) => world.last_error = None,
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

#[when(
    expr = "按代码添加投资标的 市场 {string} 代码 {string} 行情命中名称 {string} 市场 {string} 现价 {float} 类型提示 {string}"
)]
async fn add_instrument_with_stub_quote(
    world: &mut LedgerWorld,
    channel: String,
    code: String,
    name: String,
    quote_market: String,
    price: f64,
    kind_hint: String,
) {
    let kind =
        ledger_investment::InstrumentType::parse(&kind_hint).expect("未知类型提示（stock/etf）");
    let quote_market =
        ledger_investment::Market::parse(&quote_market).expect("未知命中市场（市场闭集词汇）");
    let mut fetch = move |code: &str, route: StockRoute| {
        let code = code.to_string();
        let name = name.clone();
        async move {
            let hit = quote_market.as_quote_market().map(|qm| qm.as_stock_route()) == Some(route);
            if hit {
                Ok(Quote {
                    code,
                    name,
                    price_cents: Some(price_value_to_cents(price)),
                    price_date: Some("2026-09-04".to_string()),
                    market: Some(quote_market),
                    kind_hint: Some(kind),
                    fund_class: None,
                    nav_date: None,
                    constant_unit_price_cents: None,
                    price_source: ledger_investment::prices::TENCENT_PRICE_SOURCE,
                })
            } else {
                Err(ledger_infra::error::AppError::codedp(
                    "sync.stock-not-found",
                    format!("查无股票代码 {code}，请核对后重试"),
                    &[&code],
                ))
            }
        }
    };
    run_add_instrument(world, channel, code, &mut fetch).await;
}

#[when(expr = "按代码添加投资标的 市场 {string} 代码 {string} 行情查无此码")]
async fn add_instrument_with_stub_all_miss(world: &mut LedgerWorld, channel: String, code: String) {
    let mut fetch = |code: &str, _route: StockRoute| {
        let code = code.to_string();
        async move {
            Err(ledger_infra::error::AppError::codedp(
                "sync.stock-not-found",
                format!("查无股票代码 {code}，请核对后重试"),
                &[&code],
            ))
        }
    };
    run_add_instrument(world, channel, code, &mut fetch).await;
}

#[when(expr = "按代码添加投资标的 市场 {string} 代码 {string} 行情临时不可达")]
async fn add_instrument_with_stub_temporary_failure(
    world: &mut LedgerWorld,
    channel: String,
    code: String,
) {
    let mut fetch = |_code: &str, _route: StockRoute| async move {
        Err(ledger_infra::error::AppError::Io("行情源临时不可达".into()))
    };
    run_add_instrument(world, channel, code, &mut fetch).await;
}

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

#[then(expr = "添加投资标的应返回错误 {string}")]
fn assert_add_instrument_error(world: &mut LedgerWorld, fragment: String) {
    let error = world
        .last_error
        .as_ref()
        .unwrap_or_else(|| panic!("添加投资标的应失败但未记录错误"));
    assert!(
        error.contains(&fragment),
        "错误「{error}」应包含「{fragment}」"
    );
}

#[then(expr = "标的 {string} 现价为 {int} 币种 {string}")]
fn assert_stock_market_price(
    world: &mut LedgerWorld,
    symbol: String,
    price_cents: i64,
    currency: String,
) {
    let row: (i64, String) = world_conn!(world)
        .query_row(
            "SELECT p.price_cents, p.currency_code \
             FROM market_prices p JOIN instruments i ON i.id = p.instrument_id \
             WHERE i.symbol=?1",
            params![symbol],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap_or_else(|e| panic!("标的 {symbol} 应有现价（{e}）"));
    assert_eq!(row.0, price_cents, "现价（万分之一元）不符");
    assert_eq!(row.1, currency, "币种不符");
}
