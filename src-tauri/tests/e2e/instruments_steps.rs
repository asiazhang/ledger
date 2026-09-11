//! 标的字典步骤（issue #199）：搜索语义的 BDD 接缝。实现为
//! `investment::list_instruments`（与 IPC 命令同一实现，#401 域目录化后直调域入口）。
//! 另载按代码即拉添加基金的编排接缝（issue #301 / ADR-0103）：东财报价以注入桩离线驱动，
//! 实现为 `investment::add_fund_by_code_with`（与 IPC 命令同一套
//! 校验/拉取编排/落库实现，网络层经注入替换）。

use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::db::{new_uuid, now_iso};
use tauri_app_lib::error::Result;
use tauri_app_lib::investment::{
    InstrumentInput, InstrumentListFilter, Quote, add_fund_by_code_with,
    add_stock_instrument_with_quote, create_instrument_manual,
    delete_instrument as delete_instrument_domain, fetch_stock_quote_for_add, get_instrument,
    list_instruments, prices::price_value_to_cents,
};

use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

/// 直插金融工具字典行（投资域字典，可指定中文名称供拼音语义场景使用）。
/// 库内状态直置（#764 已登记例外）：场景以此夹具模拟「存量同步行」（来源
/// 'eastmoney'），公开创建入口只产 'manual' 行（来源随行终身不变，ADR-0036），
/// 「同步来源拒删」「upsert 来源不改写」等被测前提依赖直置。
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

/// 直插指定类型的金融工具字典行（同码异类型消歧场景用，issue #294；
/// 同步来源直置例外，动机同上）。
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

/// 直插指定市场的金融工具字典行（美股持仓折算场景用，issue #696：
/// market 为闭集值如 nasdaq/nyse/amex，经 V002 检查约束验证落库；
/// 同步来源直置例外，动机同上）。
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

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

/// 手动创建标的（issue #290 / ADR-0036）：驱动 IPC 命令入口层守卫同一接缝
/// `create_instrument_manual`（类型白名单 + 名称必填在先，核心创建函数
/// 通用）。市场固定未知（不传入参，缺省 unknown）——自定义标的通道建档
/// 同此形状（issue #826：#697 兜底建档的市场透传随一级通道化退役）；
/// 结果/错误记入 world 供 Then 断言。
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
        kind: kind.parse().expect("未知金融工具类型"),
        name: Some(name),
        currency_code: currency,
        market: None,
    };
    match create_instrument_manual(&world_conn!(world), input) {
        Ok(_) => world.last_error = None,
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

/// 删除标的（issue #292 / ADR-0036 决策 5）：驱动 IPC 命令同一接缝
/// `delete_instrument`（守卫前置检查在核心函数内）。入参为标的代码：
/// 场景内代码唯一，按（代码）取 id 驱动；结果/错误记入 world 供 Then 断言。
/// 步骤前提是标的已存在（不存在标的的错误路径由域单测覆盖）。
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

/// 列出全部标的（无过滤）：验证列表返回体来源字段的接缝（issue #290 验收项）。
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

/// 按类型过滤搜索（同码异类型消歧语义，issue #294；与 HTTP 端点的 type 参数同一接缝）。
#[when(expr = "搜索类型 {string} 的标的 {string}")]
fn search_instruments_of_kind(world: &mut LedgerWorld, kind: String, query: String) {
    let filter = InstrumentListFilter {
        search: Some(query),
        kind: Some(kind.parse().expect("未知金融工具类型")),
        ..Default::default()
    };
    world.asset.last_instrument_search =
        Some(list_instruments(&world_conn!(world), &filter).expect("标的搜索失败"));
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

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

/// 标的列表返回体带来源字段（issue #290）：存量同步行回填 'eastmoney'、
/// 手动新建行标 'manual'，列表 UI 的「来源」列由此直出。
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

/// 删除守卫中文错误（issue #292）：同 last_error 记录断言模式。
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

// ---------------------------------------------------------------------------
// 按 id 精确取标的（issue #709）：走势页签 focus 消费的只读解析路径
// ---------------------------------------------------------------------------

/// 按代码定位标的并按 id 精确查询（场景内代码唯一）：域接缝直调，与 IPC 命令
/// 同一实现（先例：列表搜索步骤直调 `investment::list_instruments`）。
#[when(expr = "按 id 精确取标的 {string}")]
fn get_instrument_by_id(world: &mut LedgerWorld, symbol: String) {
    let id: String = world_conn!(world)
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| panic!("按 id 取标的：标的 {symbol} 应已存在"));
    match get_instrument(&world_conn!(world), &id) {
        Ok(inst) => {
            world.asset.last_instrument = Some(inst);
            world.last_error = None;
        }
        Err(e) => {
            world.asset.last_instrument = None;
            world.last_error = Some(e.to_string());
        }
    }
}

#[when(expr = "按 id 精确取不存在的标的")]
fn get_instrument_by_unknown_id(world: &mut LedgerWorld) {
    match get_instrument(&world_conn!(world), "inst-unknown-id") {
        Ok(_) => {
            world.asset.last_instrument = None;
            world.last_error = None;
        }
        Err(e) => {
            world.asset.last_instrument = None;
            world.last_error = Some(e.to_string());
        }
    }
}

/// 完整对象读回：身份字段（代码/名称/类型/币种）与列表行同投影。
#[then(expr = "应返回标的 代码 {string} 名称 {string} 类型 {string} 币种 {string}")]
fn assert_instrument_readback(
    world: &mut LedgerWorld,
    symbol: String,
    name: String,
    kind: String,
    currency: String,
) {
    let inst = world
        .asset
        .last_instrument
        .as_ref()
        .expect("按 id 取标的应已返回");
    assert_eq!(inst.symbol, symbol, "标的代码不匹配");
    assert_eq!(inst.name.as_deref(), Some(name.as_str()), "标的名称不匹配");
    assert_eq!(inst.kind.to_string(), kind, "标的类型不匹配");
    assert_eq!(inst.currency_code, currency, "标的币种不匹配");
}

/// 清仓标的照常返回且派生持仓标志为 false（走势不依赖持仓）。
#[then(expr = "返回标的应无持仓（invested 为 false）")]
fn assert_instrument_not_invested(world: &mut LedgerWorld) {
    let inst = world
        .asset
        .last_instrument
        .as_ref()
        .expect("按 id 取标的应已返回");
    assert!(
        !inst.invested,
        "清仓标的 invested 应为 false，实际 {}",
        inst.invested
    );
}

/// 未知 id 的码化错误（同 last_error 记录断言模式）。
#[then(expr = "取标的应返回错误 {string}")]
fn assert_get_instrument_error(world: &mut LedgerWorld, fragment: String) {
    let error = world
        .last_error
        .as_ref()
        .unwrap_or_else(|| panic!("按 id 取标的应失败但未记录错误"));
    assert!(
        error.contains(&fragment),
        "错误「{error}」应包含「{fragment}」"
    );
}

// ---------------------------------------------------------------------------
// 按代码即拉添加基金（issue #301 / ADR-0038）：When 注入桩驱动编排接缝
// ---------------------------------------------------------------------------

/// 驱动添加基金编排接缝并把错误记入 world（供 Then 断言；成功经重列标的/现价缓存断言）。
/// 获取函数收到请求代码时须全等（桩只对目标代码返回详情）。
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
    // 统一报价载荷（行情接入，ADR-0103）：价格日期与净值日期同为净值日期。
    let quote = Quote {
        code: code.clone(),
        name,
        price_cents: Some(price_value_to_cents(nav)),
        price_date: Some(nav_date.clone()),
        market: None,
        kind_hint: None,
        fund_class: Some(fund_class),
        nav_date: Some(nav_date),
    };
    run_add_fund(world, code, move |requested: &str, _market: &str| {
        assert_eq!(requested, quote.code, "获取函数应收到请求代码");
        Ok(quote.clone())
    });
}

#[when(expr = "按代码添加基金 {string} 东财返回名称 {string} 分类 {string} 未取到净值")]
fn add_fund_with_stub_no_nav(
    world: &mut LedgerWorld,
    code: String,
    name: String,
    fund_class: String,
) {
    let quote = Quote {
        code: code.clone(),
        name,
        price_cents: None,
        price_date: None,
        market: None,
        kind_hint: None,
        fund_class: Some(fund_class),
        nav_date: None,
    };
    run_add_fund(world, code, move |requested: &str, _market: &str| {
        assert_eq!(requested, quote.code, "获取函数应收到请求代码");
        Ok(quote.clone())
    });
}

#[when(expr = "按代码添加基金 {string} 东财查无此码")]
fn add_fund_with_stub_not_found(world: &mut LedgerWorld, code: String) {
    let mut fetch = |requested: &str, _market: &str| -> Result<Quote> {
        Err(tauri_app_lib::error::AppError::Invalid(format!(
            "查无基金代码 {requested}，请核对后重试",
        )))
    };
    let outcome = add_fund_by_code_with(&world_conn!(world), &code, &mut fetch);
    match outcome {
        Ok(_) => panic!("查无此码应报错而非成功"),
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// 添加投资标的·股票通道（issue #697 / spec #690）：市场必选的录入通道 → 按代码
// 查询 → 类型自动识别 → 创建增强回填。编排接缝与生产 IPC 命令同一组合：
// fetch_stock_quote_for_add（锁外查询阶段）→ add_stock_instrument_with_quote
//（识别落库阶段）；东财行情以注入桩离线驱动，桩按请求市场是否等于命中市场
// 决定命中或未命中——美股通道命中前的候选按未命中继续（遍历语义随之被驱动），
// 代码回显请求归一化形态（港股补零 / 美股大写，与访问层回显同构）。
// ---------------------------------------------------------------------------

/// 股票添加的桩驱动组合：命中市场与行情内容随参数给定；`quote_market` 为 None
/// 时桩恒未命中（查无此码），`Err(Io)` 形态以 `temporary_failure` 开关表达。
fn run_add_instrument<F>(world: &mut LedgerWorld, channel: String, code: String, fetch: &mut F)
where
    F: FnMut(&str, &str) -> Result<Quote>,
{
    // 查询阶段（生产在连接锁外）：通道解析 → 候选遍历。
    let quote = match fetch_stock_quote_for_add(&channel, &code, fetch) {
        Ok(quote) => quote,
        Err(e) => {
            world.last_error = Some(e.to_string());
            return;
        }
    };
    // 识别落库阶段（生产在统一写入口内）：类型 = 行情 kind_hint。
    match add_stock_instrument_with_quote(&world_conn!(world), &quote) {
        Ok(_) => world.last_error = None,
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

#[when(
    expr = "按代码添加投资标的 市场 {string} 代码 {string} 行情命中名称 {string} 市场 {string} 现价 {float} 类型提示 {string}"
)]
fn add_instrument_with_stub_quote(
    world: &mut LedgerWorld,
    channel: String,
    code: String,
    name: String,
    quote_market: String,
    price: f64,
    kind_hint: String,
) {
    let kind: tauri_app_lib::investment::InstrumentType =
        kind_hint.parse().expect("未知类型提示（stock/etf）");
    let mut fetch = move |code: &str, market: &str| -> Result<Quote> {
        if market == quote_market {
            Ok(Quote {
                // 代码回显请求归一化形态（与访问层回显同构：命中判定 = 回显全等）。
                code: code.to_string(),
                name: name.clone(),
                price_cents: Some(price_value_to_cents(price)),
                price_date: Some("2026-09-04".to_string()),
                market: Some(market.to_string()),
                kind_hint: Some(kind),
                fund_class: None,
                nav_date: None,
            })
        } else {
            Err(tauri_app_lib::error::AppError::codedp(
                "sync.stock-not-found",
                format!("查无股票代码 {code}，请核对后重试"),
                &[code],
            ))
        }
    };
    run_add_instrument(world, channel, code, &mut fetch);
}

#[when(expr = "按代码添加投资标的 市场 {string} 代码 {string} 行情查无此码")]
fn add_instrument_with_stub_all_miss(world: &mut LedgerWorld, channel: String, code: String) {
    let mut fetch = |code: &str, market: &str| -> Result<Quote> {
        let _ = market;
        Err(tauri_app_lib::error::AppError::codedp(
            "sync.stock-not-found",
            format!("查无股票代码 {code}，请核对后重试"),
            &[code],
        ))
    };
    run_add_instrument(world, channel, code, &mut fetch);
}

#[when(expr = "按代码添加投资标的 市场 {string} 代码 {string} 行情临时不可达")]
fn add_instrument_with_stub_temporary_failure(
    world: &mut LedgerWorld,
    channel: String,
    code: String,
) {
    let mut fetch = |_code: &str, _market: &str| -> Result<Quote> {
        Err(tauri_app_lib::error::AppError::Io("东财临时不可达".into()))
    };
    run_add_instrument(world, channel, code, &mut fetch);
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

/// 股票/ETF 通道落库后的现价断言（issue #697）：与基金现价不同源——
/// priced_at 为写入时刻、净值日期恒空（净值日期是场外基金语义，详断
/// 在域单测 stock_add 钉住，此处不断言）。
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
