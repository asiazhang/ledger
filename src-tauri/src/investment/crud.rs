//! 投资域市场数据 CRUD（汇率 / 行情 / 标的字典，与持仓报告无关的字典写入）。
//!
//! 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）：本模块对备份域
//! 零感知，写入成功后的置脏/到期检查由调用方所在写入口闭包在提交点单点执行。

use rusqlite::Connection;

use super::command::{
    ExchangeRateCommand, InstrumentCommand, InstrumentCommandRow, PriceCommand,
    record_exchange_rate, record_instrument, record_price,
};
use super::model::{
    Holding, Instrument, InstrumentInput, InstrumentListFilter, InstrumentListResult,
    InstrumentType, MarketPrice, MarketPriceInput,
};
use super::predicates::INVESTED_EXISTS;
use super::prices::{MarketPriceWrite, upsert_market_price};
use crate::currencies::{ExchangeRate, ExchangeRateInput};
use crate::db::query::{query_all, query_one};
use crate::db::{new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::sync_engine::device_id;
use crate::transaction::search_text::{split_terms, term_matches_text};

pub fn list_holdings(conn: &Connection) -> Result<Vec<Holding>> {
    query_all(
        conn,
        "SELECT id,account_id,instrument_id,quantity,cost_basis_cents,cost_currency_code, \
         latest_price_cents,latest_price_currency_code,latest_nav_date,market_value_cents,unrealized_pnl_cents,updated_at \
         FROM v_holdings ORDER BY account_id, instrument_id",
        [],
    )
}

pub fn list_exchange_rates(conn: &Connection) -> Result<Vec<ExchangeRate>> {
    query_all(
        conn,
        "SELECT id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id \
         FROM exchange_rates ORDER BY base_code, quote_code",
        [],
    )
}

pub fn create_exchange_rate(conn: &Connection, input: ExchangeRateInput) -> Result<String> {
    if input.rate <= 0.0 {
        return Err(AppError::coded("fx.rate-positive", "汇率必须大于 0"));
    }
    let id = write_exchange_rate(
        conn,
        &new_uuid(),
        &input.base_code,
        &input.quote_code,
        input.rate,
        &input.priced_at,
        input.source.as_deref(),
    )?;
    // op 产出接缝（issue #861 / ADR-0091）：本地写成功 → 动作随行追加进本机
    // OpLog；随同一事务提交/回滚，写失败不残留 op。
    record_exchange_rate(
        conn,
        ExchangeRateCommand::Upsert {
            id: id.clone(),
            base_code: input.base_code,
            quote_code: input.quote_code,
            rate: input.rate,
            priced_at: input.priced_at,
            source: input.source,
        },
    )?;
    Ok(id)
}

/// 汇率写入协议（本地录入与重放共用，无 op 产出）：按货币对 upsert，未命中以
/// `insert_id` 新建（本地传新生成 id、重放携带源端 id，两端收敛同一行），命中
/// 复用既有行 id 只更新语义列。返回落库行实际 id。
pub(crate) fn write_exchange_rate(
    conn: &Connection,
    insert_id: &str,
    base_code: &str,
    quote_code: &str,
    rate: f64,
    priced_at: &str,
    source: Option<&str>,
) -> Result<String> {
    let now = now_iso();
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
            rusqlite::params![base_code, quote_code],
            |r| r.get(0),
        )
        .ok();
    let id = existing_id.unwrap_or_else(|| insert_id.to_string());
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) \
         ON CONFLICT(base_code, quote_code) DO UPDATE SET \
         rate=excluded.rate, priced_at=excluded.priced_at, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1, device_id=excluded.device_id",
        rusqlite::params![
            id,
            base_code,
            quote_code,
            rate,
            priced_at,
            source,
            now,
            1,
            device_id(conn)?
        ],
    )?;
    Ok(id)
}

pub fn list_market_prices(conn: &Connection) -> Result<Vec<MarketPrice>> {
    query_all(
        conn,
        "SELECT id,instrument_id,price_cents,currency_code,priced_at,nav_date,source,created_at,updated_at,version,device_id \
         FROM market_prices ORDER BY instrument_id, priced_at DESC",
        [],
    )
}

pub fn create_market_price(conn: &Connection, input: MarketPriceInput) -> Result<String> {
    if input.price_cents <= 0 {
        return Err(AppError::coded(
            "instrument.price-positive",
            "价格必须大于 0",
        ));
    }
    // 写入委托现价缓存单点 upsert（issue #291 收口：原就地 SQL 与同步通道
    // 两份同形 upsert 合并为一份）；手动落价无净值日期语义，nav_date 覆盖为 NULL
    // （防基金现价被手动更新后旧净值日期残留错配，与同步通道同规则）。
    // source 透传入参（可空，发布 API 形状不变）；手动报价正经 manual_price 模块
    // （record_manual_price），本命令为已发布的独立写价通道（issue #291 前的半成品）。
    let id = upsert_market_price(
        conn,
        &MarketPriceWrite {
            instrument_id: &input.instrument_id,
            price_cents: input.price_cents,
            currency_code: &input.currency_code,
            priced_at: &input.priced_at,
            nav_date: None,
            source: input.source.as_deref(),
        },
    )?;
    // op 产出接缝（issue #861 / ADR-0091）：本地写成功 → 动作随行追加（裁决域
    // = 标的的现价行）；随同一事务提交/回滚。
    record_price(
        conn,
        PriceCommand::MarketPrice {
            instrument_id: input.instrument_id,
            price_cents: input.price_cents,
            currency_code: input.currency_code,
            priced_at: input.priced_at,
            source: input.source,
        },
    )?;
    Ok(id)
}

/// 标的搜索的匹配目标：「代码 · 名称」label 等价文本（与投资表单标的下拉的
/// 选项 label 一致；无名称时退化为裸代码）。收口为具名函数，语义变更只改这里。
fn instrument_match_label(inst: &Instrument) -> String {
    match inst.name.as_deref().filter(|n| !n.is_empty()) {
        Some(name) => format!("{} · {}", inst.symbol, name),
        None => inst.symbol.clone(),
    }
}

pub fn list_instruments(
    conn: &Connection,
    filter: &InstrumentListFilter,
) -> Result<InstrumentListResult> {
    // 关键字过滤走统一模糊搜索语义（ADR-0027，复用全局搜索纯函数）：词条之间
    // AND，命中 = 原文连续子串 ∨ 拼音首字母子序列（大小写不敏感）。判定目标为
    // 「代码 · 名称」label 等价文本（instrument_match_label，与投资表单标的
    // 下拉的 label 一致）。子序列匹配无法下推 SQL，故有搜索词时取候选后
    // Rust 内存过滤再内存分页。
    let search_terms = filter
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(split_terms);

    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(market) = filter.market.as_deref().filter(|m| !m.is_empty()) {
        params.push(Box::new(market.to_string()));
        conditions.push(format!("i.market=?{}", params.len()));
    }
    // 标的类型过滤（issue #294）：同码异类型消歧（如基金 000001 vs 股票 000001）。
    if let Some(kind) = filter.kind {
        params.push(Box::new(kind.to_string()));
        conditions.push(format!("i.instrument_type=?{}", params.len()));
    }
    // 只看持仓：有当前持仓的标的，谓词单点见 predicates 模块（别名契约：i = instruments）。
    if filter.only_invested == Some(true) {
        conditions.push(INVESTED_EXISTS.to_string());
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let select_sql = |limit_clause: &str| {
        format!(
            "SELECT {} \
             FROM instruments i \
             LEFT JOIN market_prices p ON p.instrument_id = i.id \
             {where_clause} ORDER BY i.symbol{limit_clause}",
            instrument_row_projection()
        )
    };

    let page = filter.page.unwrap_or(1).max(1);
    let page_size = filter.page_size.unwrap_or(50).clamp(1, 500);
    let offset = (page - 1) * page_size;

    let (total, items) = if let Some(terms) = &search_terms {
        // 语义匹配分支：全量候选后 Rust 过滤，total = 命中数，内存分页。
        let all: Vec<Instrument> = query_all(conn, &select_sql(""), params_ref.as_slice())?;
        let matched: Vec<Instrument> = all
            .into_iter()
            .filter(|inst| {
                terms
                    .iter()
                    .all(|t| term_matches_text(t, &instrument_match_label(inst)))
            })
            .collect();
        let total = matched.len() as i64;
        let items = matched.into_iter().skip(offset).take(page_size).collect();
        (total, items)
    } else {
        let total: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM instruments i{where_clause}"),
            params_ref.as_slice(),
            |r| r.get(0),
        )?;
        let mut params = params;
        params.push(Box::new(page_size as i64));
        params.push(Box::new(offset as i64));
        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let items = query_all(
            conn,
            &select_sql(&format!(
                " LIMIT ?{} OFFSET ?{}",
                params.len() - 1,
                params.len()
            )),
            params_ref.as_slice(),
        )?;
        (total, items)
    };

    Ok(InstrumentListResult { items, total })
}

/// 标的行 SELECT 投影单点（列表行与按 id 精确取同一形状，issue #709）：基础列
/// 现价缓存（LEFT JOIN）与持仓标志派生列；别名契约 i = instruments、
/// p = market_prices（持仓谓词 `INVESTED_EXISTS` 的别名契约同此），投影变更
/// 只改这里，两个读路径不漂移。
fn instrument_row_projection() -> String {
    format!(
        "i.id,i.symbol,i.instrument_type,i.name,i.currency_code,i.market,i.created_at,i.updated_at,i.version,i.device_id,i.source,p.price_cents, \
         CASE WHEN {INVESTED_EXISTS} THEN 1 ELSE 0 END AS invested"
    )
}

/// 按 id 精确取标的（issue #709）：走势页签 focus 消费的只读解析路径——现有
/// 标的列表过滤仅支持搜索词/市场/类型/持仓，无按 id 路径。返回完整标的对象
/// （行投影与 [`list_instruments`] 列表行一致：含现价缓存与持仓标志），清仓/
/// 无持仓标的照常返回（走势不依赖持仓）；不存在返回码化错误（与删除守卫
/// 同码 `instrument.not-found`）。
pub fn get_instrument(conn: &Connection, id: &str) -> Result<Instrument> {
    query_one(
        conn,
        &format!(
            "SELECT {} \
             FROM instruments i \
             LEFT JOIN market_prices p ON p.instrument_id = i.id \
             WHERE i.id=?1",
            instrument_row_projection()
        ),
        [id],
    )?
    .ok_or_else(|| {
        AppError::codedp_not_found("instrument.not-found", format!("标的 {id} 不存在"), &[id])
    })
}

/// 自建标的物理删除（issue #292 / ADR-0036 决策 5）：守卫前置检查——仅来源为
/// 手动且无任何 buy/sell 流水引用（security_transactions 无行）的自建标的可删；
/// 有引用拒删（交易行与明细归属用户记账事实，不随字典清理）、同步来源标的拒删
/// （字典修正由按代码查询/创建带回权威名称承担，ADR-0081）。不引入软删——标的字典查询面不被污染。现价缓存与
/// 价格历史随外键 CASCADE 一并消失；持仓批次表虽是 RESTRICT，但批次行的
/// buy_transaction_id 为指向 security_transactions 的 NOT NULL 外键——批次存在
/// 必有买入明细行，故守卫的流水 COUNT 已覆盖批次（无流水 ⟺ 无批次），
/// DELETE 不会撞到 RESTRICT 外键错误。
/// 自建标的删除（issue #292 / ADR-0036 决策 5）：守卫与删除语义单一归属
/// [`write_delete_instrument`]，本入口只叠加 op 产出（issue #861）。
pub fn delete_instrument(conn: &Connection, id: &str) -> Result<()> {
    write_delete_instrument(conn, id)?;
    // op 产出接缝（issue #861 / ADR-0091）：删除成功 → delete op（实体 id）
    // 追加；随同一事务提交/回滚。
    record_instrument(conn, InstrumentCommand::Delete { id: id.to_string() })
}

/// 标的删除协议（本地删除与重放共用，无 op 产出）：守卫前置检查——仅来源为
/// 手动且无任何 buy/sell 流水引用（security_transactions 无行）的自建标的可删；
/// 有引用拒删（交易行与明细归属用户记账事实，不随字典清理）、同步来源标的拒删
/// （字典修正由按代码查询/创建带回权威名称承担，ADR-0081）。不引入软删——标的字典查询面不被污染。现价缓存与
/// 价格历史随外键 CASCADE 一并消失；持仓批次表虽是 RESTRICT，但批次行的
/// buy_transaction_id 为指向 security_transactions 的 NOT NULL 外键——批次存在
/// 必有买入明细行，故守卫的流水 COUNT 已覆盖批次（无流水 ⟺ 无批次），
/// DELETE 不会撞到 RESTRICT 外键错误。重放端守卫原样生效：守卫失败（如另一端
/// 已有流水）由引擎挂起待裁决，不自动放行。
pub(crate) fn write_delete_instrument(conn: &Connection, id: &str) -> Result<()> {
    let source: Option<String> = conn
        .query_row(
            "SELECT source FROM instruments WHERE id=?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .ok();
    let source = source.ok_or_else(|| {
        AppError::codedp_not_found("instrument.not-found", format!("标的 {id} 不存在"), &[id])
    })?;
    if source != "manual" {
        return Err(AppError::coded(
            "instrument.sync-delete-forbidden",
            "同步来源标的不支持删除：名称与市场由按代码查询/创建带回权威信息维护",
        ));
    }
    let trade_refs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM security_transactions WHERE instrument_id=?1",
        rusqlite::params![id],
        |r| r.get(0),
    )?;
    if trade_refs > 0 {
        return Err(AppError::coded(
            "instrument.traded-delete-forbidden",
            "该标的已有买卖流水，无法删除：可先删除相关交易后再试",
        ));
    }
    conn.execute("DELETE FROM instruments WHERE id=?1", rusqlite::params![id])?;
    Ok(())
}

/// 核心创建函数（手动 IPC 命令与 AI HTTP 端点共用，ADR-0037）：新建行来源标
/// 'manual'（非同步即手动），（代码，类型）命中既有行则复用并只更新名称/市场，
/// 来源随行终身不变（issue #293 / ADR-0036 决策 2）。写入委托共享协议
/// [`write_instrument`]，实际变化按形态产出 op（issue #861）——新建 →
/// Create op；复用改名/改市场 → Update op；无变化复用不产出（op 是本机数据
/// 变化的记录，零变化零 op）。
pub fn create_instrument(conn: &Connection, input: InstrumentInput) -> Result<String> {
    if input.symbol.trim().is_empty() {
        return Err(AppError::coded(
            "instrument.symbol-required",
            "标的代码不能为空",
        ));
    }
    let row = InstrumentCommandRow {
        symbol: input.symbol,
        kind: input.kind,
        name: input.name,
        currency_code: input.currency_code,
        market: input.market.unwrap_or_else(|| "unknown".to_string()),
    };
    let (id, outcome) = write_instrument(conn, &new_uuid(), &row)?;
    match outcome {
        InstrumentWrite::Created => record_instrument(
            conn,
            InstrumentCommand::Create {
                id: id.clone(),
                row: row.clone(),
            },
        )?,
        InstrumentWrite::Renamed => record_instrument(
            conn,
            InstrumentCommand::Update {
                id: id.clone(),
                symbol: row.symbol.clone(),
                kind: row.kind,
                name: row.name.clone(),
                market: row.market.clone(),
            },
        )?,
        InstrumentWrite::Unchanged => {}
    }
    Ok(id)
}

/// 标的写入形态（op 产出判据）：新建 / 复用有变化 / 复用无变化。
pub(crate) enum InstrumentWrite {
    Created,
    Renamed,
    Unchanged,
}

/// 标的写入协议（本地创建与重放共用，无 op 产出）：按自然键（symbol, 类型）
/// 幂等 upsert——未命中以 `insert_id` 新建（本地传新生成 id、重放携带源端 id，
/// 两端收敛同一行），命中复用既有行 id 并只更新名称/市场（有变化才写）。
/// 返回（生效行 id，写入形态）。
pub(crate) fn write_instrument(
    conn: &Connection,
    insert_id: &str,
    row: &InstrumentCommandRow,
) -> Result<(String, InstrumentWrite)> {
    let existing_id: Option<(String, Option<String>, String)> = conn
        .query_row(
            "SELECT id, name, market FROM instruments WHERE symbol=?1 AND instrument_type=?2",
            rusqlite::params![row.symbol, row.kind],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    if let Some((existing_id, existing_name, existing_market)) = existing_id {
        let name_changed = row.name != existing_name;
        let market_changed = row.market != existing_market;
        if name_changed || market_changed {
            let now = now_iso();
            conn.execute(
                "UPDATE instruments SET name=?1, market=?2, updated_at=?3, version=version+1 WHERE id=?4",
                rusqlite::params![row.name, row.market, now, existing_id],
            )?;
            return Ok((existing_id, InstrumentWrite::Renamed));
        }
        return Ok((existing_id, InstrumentWrite::Unchanged));
    }
    let now = now_iso();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id,source) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'manual')",
        rusqlite::params![
            insert_id,
            row.symbol,
            row.kind,
            row.name,
            row.currency_code,
            row.market,
            now,
            now,
            1,
            device_id(conn)?
        ],
    )?;
    Ok((insert_id.to_string(), InstrumentWrite::Created))
}

/// 复用改名协议（重放）：以自然键（symbol, 类型）定位——建档 id 各端独立生成
/// （并发建档各成一行、业务字段经自然键 upsert 收敛到先到行），自然键是稳定
/// 身份；行不存在（Update 先于 Create 到达）以码化 NotFound 上抛，由引擎挂起，
/// Create 补齐后重投递自愈。
pub(crate) fn write_instrument_update(
    conn: &Connection,
    symbol: &str,
    kind: InstrumentType,
    name: Option<&str>,
    market: &str,
) -> Result<()> {
    let changed = conn.execute(
        "UPDATE instruments SET name=?1, market=?2, updated_at=?3, version=version+1 \
         WHERE symbol=?4 AND instrument_type=?5",
        rusqlite::params![name, market, now_iso(), symbol, kind],
    )?;
    if changed == 0 {
        return Err(AppError::codedp_not_found(
            "instrument.not-found",
            format!("标的 {symbol} 不存在"),
            &[symbol],
        ));
    }
    Ok(())
}

/// 同步随行名称刷新接缝（issue #827）：以数据源权威名称覆盖标的行名称——仅当
/// 名称实际变化时写入（零变化零写入、不虚增 version），返回是否发生写入。
/// 与 [`create_instrument`] 的「复用即更新名称」同一份 UPDATE 语义：只改名称，
/// 不动市场/来源等其余列。空名称（数据源缺名/测试桩）与行已不存在（并发删除）
/// 静默跳过。名称随行刷新是字典修正的同步翼（「随用随修 + 同步随行刷新」，
/// ADR-0036/0081 修订）：同码自建行名称被数据源覆盖为已接受代价（产品裁决留痕）。
pub fn refresh_instrument_name(conn: &Connection, instrument_id: &str, name: &str) -> Result<bool> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(false);
    }
    let current: Option<String> = match conn.query_row(
        "SELECT name FROM instruments WHERE id=?1",
        rusqlite::params![instrument_id],
        |r| r.get(0),
    ) {
        Ok(name) => Some(name),
        // 行已不存在（并发删除）：静默跳过；其余真实 DB 错误照常上抛，不吞。
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(e) => return Err(e.into()),
    };
    match current {
        Some(existing) if existing != name => {
            conn.execute(
                "UPDATE instruments SET name=?1, updated_at=?2, version=version+1 WHERE id=?3",
                rusqlite::params![name, now_iso(), instrument_id],
            )?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// 手动创建入口守卫（ADR-0036 决策 3）：类型白名单收窄为债券/ETF/其他三类——
/// 股票字典归按代码查询/创建带回权威名称（ADR-0081）、基金唯一创建入口归按代码
/// 即拉（issue #301 / ADR-0038），白名单让手动字典与两条自动通道永不相交；
/// 名称必填（自建标的主身份是名称）。
/// 守卫属 UI 入口政策，核心创建函数 [`create_instrument`] 保持通用：AI HTTP
/// 创建端点（ADR-0037）五类全开、名称可选，不经本守卫。同一接缝供 IPC 命令
/// 与 BDD 步骤复用。
pub fn create_instrument_manual(conn: &Connection, input: InstrumentInput) -> Result<String> {
    match input.kind {
        InstrumentType::Bond | InstrumentType::Etf | InstrumentType::Other => {}
        InstrumentType::Stock => {
            return Err(AppError::coded(
                "instrument.stock-manual-forbidden",
                "股票类标的不支持手动创建：请用「添加投资标的」按代码查询，自动回填东财权威名称",
            ));
        }
        InstrumentType::Fund => {
            return Err(AppError::coded(
                "instrument.fund-manual-forbidden",
                "基金类标的不支持手动创建：请用「添加基金」输入 6 位代码自动回填",
            ));
        }
    }
    if input.name.as_deref().is_none_or(|n| n.trim().is_empty()) {
        return Err(AppError::coded(
            "instrument.name-required",
            "标的名称不能为空",
        ));
    }
    create_instrument(conn, input)
}
