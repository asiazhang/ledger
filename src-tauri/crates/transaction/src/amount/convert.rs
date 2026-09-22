//! 本位币折算（共享语义区）：raw 币种金额 → 全局默认币种，三个具名入口。
//!
//! 职责：[`default_currency_code`](本位币基准读取)、[`convert_to_native_current`]
//! （**当期折算**，读路径入口：持仓市值、净资产、财务自由度、实物资产估值、跨账本
//! 汇总、定时花费）、[`convert_to_native_on_trade_date`]（**按交易日折算**，写路径
//! 创建入口，#1547 接入：按交易所属 ISO 周命中汇率历史；返回值随行携带折算留痕
//! [`NativeConversion`]，#1548；可选逐笔显式汇率入参，显式 > 序列 > 报错，#1549）、
//! [`convert_to_native_on_edit`]（**编辑沿用**，写路径修改入口，#1550 接入：未携
//! 显式且币种/金额/日期未变沿用行内留痕，任一变才重查）。
//! 各入口不设隐式默认，调用方必须显式选择（#1540 spec）。
//! 共同不变量：基准为全局默认币种、与账户币种无关（避免跨账户漂移）；与本位币同
//! 币种原样返回；正反向汇率均无即报错，不静默混币种。ADR 指针：ADR-0011 / ADR-0091
//! 决策 3 / ADR-0113 决策 3.1。陷阱：本位币读取经 `super::base_currency` 接缝，
//! 未注册即码化错误。

use rusqlite::Connection;
use rusqlite::OptionalExtension;

use ledger_infra::closed_set;
use ledger_infra::error::{AppError, Result};

/// 周键派生表达式（与 `fx_rate_history.week_start` 生成列同式，V010）：周一为
/// 周键。命中查询与文案推导共用同一片段，保证两侧对同一交易日恒得同周键。
const WEEK_START_EXPR: &str = "date(?,'-6 days','weekday 1')";

// ---------------------------------------------------------------------------
// 折算来源闭集与留痕载体
// ---------------------------------------------------------------------------

closed_set! {
/// 折算来源闭集（issue #1548 / ADR-0011 2026-09-19 修订）：本笔本位币金额的
/// 折算取数来源，随交易行落库留痕（`transactions.fx_rate_source`，V029），使
/// 「这个金额怎么来的」可读回解释、可与重算对齐。
///
/// 与 `transactions.fx_rate_source` 列字面量一一对应（五份表示由 `closed_set!`
/// 同体派生，ADR-0108 同规）；`explicit` 的写入通道由 #1549 接入（调用方逐笔
/// 显式给定），本闭集先收口字面量事实。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FxRateSource {
    /// 命中汇率历史序列（`fx_rate_history` 交易所属周，正反向兜底）。
    Series => "series",
    /// 调用方逐笔显式给定（#1549 接入）：数据源覆盖不到的日期由写入方给出。
    Explicit => "explicit",
}
err_label = "折算来源",
err_code = "fx.source-unknown",
}

// rusqlite：从 `transactions.fx_rate_source` 列直接读为枚举（DB 边界：TEXT 列经
// [`FxRateSource::parse`] 严格映射，未知值即 FromSql 错误——写入通道闭集收口，
// 正常数据不可达，与 [`super::TransactionKind`] 同规）。
impl rusqlite::types::FromSql for FxRateSource {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        FxRateSource::parse(value.as_str()?)
            .map_err(|e| rusqlite::types::FromSqlError::Other(Box::new(e)))
    }
}

// OpenAPI（utoipa）：闭集枚举以小写字符串枚举值入文档，与 wire 格式一致。
impl utoipa::PartialSchema for FxRateSource {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::Schema> {
        utoipa::openapi::RefOr::T(utoipa::openapi::Schema::Object(
            utoipa::openapi::ObjectBuilder::new()
                .schema_type(utoipa::openapi::Type::String)
                .enum_values(Some(FxRateSource::ALL.map(|s| s.as_str().to_string())))
                .description(Some(
                    "折算来源（闭集：series=汇率历史序列；explicit=调用方显式给定）",
                ))
                .build(),
        ))
    }
}

impl utoipa::ToSchema for FxRateSource {}

// serde：与列字面量同形的小写字符串（wire 格式）；反序列化复用 [`FxRateSource::parse`]。
impl serde::Serialize for FxRateSource {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for FxRateSource {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FxRateSource::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// 编辑沿用基线（#1550 / ADR-0011 2026-09-19 修订）：旧行的折算判别三元组
/// （金额/币种/日期）与行内留痕。由修改路径从旧行读出后传入
/// [`convert_to_native_on_edit`]，创建路径传 `None`；重放路径不消费（携带源端
/// 折算结果，ADR-0091 决策 3）。
#[derive(Debug, Clone, PartialEq)]
pub struct FxEditBaseline {
    /// 旧行原始币种金额（分）。
    pub amount_cents: i64,
    /// 旧行币种（退款为继承后的生效币种，与归一化行同口径）。
    pub currency_code: String,
    /// 旧行交易日期。
    pub date: String,
    /// 旧行行内留痕（本位币金额 + 汇率值 + 来源；未折算/存量行为空）。
    pub conversion: NativeConversion,
}

/// 按交易日折算的结果（写路径，#1548）：本位币金额 + 折算来源留痕。
///
/// 留痕两列随归一化行落库（`transactions.fx_rate_used` / `fx_rate_source`，V029）：
/// 未折算（与本位币同币种）时留痕两列为 `None`——空值即「本笔未折算」的诚实语义
/// （同币种原样返回、无汇率可留）。已折算时满足
/// `native_cents ≈ amount_cents × fx_rate_used`（四舍五入到分）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeConversion {
    /// 折算后的本位币金额（整数分，四舍五入）。
    pub native_cents: i64,
    /// 本笔使用的汇率值（反向兜底时存使用值即倒数）；未折算为 `None`。
    pub fx_rate_used: Option<f64>,
    /// 本笔折算来源；未折算为 `None`。
    pub fx_rate_source: Option<FxRateSource>,
}

impl NativeConversion {
    /// **零腿例外**（#1692 / ADR-0106 决策 1 / ADR-0011 2026-09-22 修订 ③）：
    /// 无现金腿写入（份额调整 split 的本地 `prepare_split` 与重放 `replay_split_plan`）
    /// 的显式零腿构造——本位币 0、无汇率留痕、无来源留痕。
    ///
    /// 本构造器是这条知识的唯一住址：**无现金腿不经任何折算入口、不查汇率表**——
    /// 0 在任何币种下的本位币折算恒为 0（0 × 任意汇率 = 0，与汇率在场与否无关），
    /// 故 0 元外币行即使当期表与历史序列都无该币种对也合法落库；留痕两列同为
    /// `None`（#1548 空值语义：「本笔未折算」的诚实表达，不伪造来源）。split 的
    /// 既有守卫（拒绝非零金额 / 携带显式汇率 / 重放拒绝非零 native）不因此松动。
    pub const fn zero_cash_leg() -> Self {
        NativeConversion {
            native_cents: 0,
            fx_rate_used: None,
            fx_rate_source: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 本位币折算
// ---------------------------------------------------------------------------

/// 全局默认（本位）币种基准：`amount_native_cents` 的折算基准，读账本级设置
/// （LedgerLevelSetting 首个成员，issue #858 / ADR-0091 决策 3）：存储落
/// `app_settings`（ADR-0017，读写协议归币种域），缺 key / 缺表回默认 CNY（行为
/// 免费正确），随多端同步分发、全设备强制一致。读取自 #1092 起经交易×币种接缝
/// （[`base_currency`]）：注册点在本共享语义区，实现由币种域启动时装入，
/// 本域对币种域零直接依赖。模块内其余口径不变。
pub fn default_currency_code(conn: &Connection) -> Result<String> {
    super::base_currency::current_base_currency(conn)
}

/// 查询货币对当前汇率（正查失败则反查取倒数）。
///
/// 私有依赖（spec #52）：旧壳层同名查询已随 issue #60 接线删除，
/// 本函数即其收口后的单一实现（Writer 接缝落地时已统一）。
fn lookup_exchange_rate(conn: &Connection, base_code: &str, quote_code: &str) -> Result<f64> {
    if base_code == quote_code {
        return Ok(1.0);
    }
    if let Ok(rate) = conn.query_row(
        "SELECT rate FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
        rusqlite::params![base_code, quote_code],
        |r| r.get::<_, f64>(0),
    ) {
        if rate <= 0.0 {
            return Err(AppError::codedp(
                "fx.rate-non-positive",
                format!("汇率 {base_code}->{quote_code} 非正: {rate}"),
                &[base_code, quote_code, &rate.to_string()],
            ));
        }
        return Ok(rate);
    }
    if let Ok(rev) = conn.query_row(
        "SELECT rate FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
        rusqlite::params![quote_code, base_code],
        |r| r.get::<_, f64>(0),
    ) {
        if rev <= 0.0 {
            return Err(AppError::codedp(
                "fx.reverse-rate-non-positive",
                format!("反向汇率 {quote_code}->{base_code} 非正: {rev}"),
                &[quote_code, base_code, &rev.to_string()],
            ));
        }
        return Ok(1.0 / rev);
    }
    Err(AppError::codedp(
        "fx.rate-missing",
        // 指路文案（issue #1664）：当期表无行多因从未同步，指向设置页「币种」
        // 页签的手动同步入口，面向可自助补救。
        format!(
            "未找到 {base_code} -> {quote_code} 的汇率（正反向均无），可在设置页「币种」页签同步汇率后重试"
        ),
        &[base_code, quote_code],
    ))
}

/// **当期折算**（读路径入口）：将原始币种金额按**当期汇率**折算为全局默认币种
/// 金额（四舍五入到分）。
///
/// - 取数读当期汇率表 `exchange_rates`（每币种对一行当期值）。
/// - 币种与默认币种相同 → 1:1 原样返回。
/// - 基准为 [`default_currency_code`]，**与账户币种无关**：
///   各账户的交易统一折算到同一本位币，避免跨账户漂移。
/// - 正反向汇率均无 → 报错，不静默混币种。
///
/// 消费面（#1541 起）：持仓市值、净资产、财务自由度、实物资产估值、跨账本汇总、
/// 定时花费等读路径（写路径自 #1547 起全部改接
/// [`convert_to_native_on_trade_date`]，当期表不再是写路径取数源）。
///
/// 守门台账（#1692 / ADR-0011 2026-09-22 修订 ①）：本入口的**生产调用方闭集**由
/// 结构守门的符号调用方白名单 `CONVERT_CURRENT_CALLERS`（`scripts/check-structure.ts`）
/// 守门——白名单外调用即红（测试代码按 `/tests/` 路径约定豁免）；新增读路径消费面
/// 先在该台账留痕成因，写路径一律改接按交易日入口。
pub fn convert_to_native_current(
    conn: &Connection,
    amount_cents: i64,
    currency_code: &str,
) -> Result<i64> {
    let target = default_currency_code(conn)?;
    if currency_code == target {
        return Ok(amount_cents);
    }
    let rate = lookup_exchange_rate(conn, currency_code, &target)?;
    Ok((amount_cents as f64 * rate).round() as i64)
}

/// **按交易日折算**（写路径入口，#1547 接入）：raw 币种金额 → 全局默认币种，
/// 取数时点为交易所属日期（#1540 spec：折算时点取交易日期，不用当期值折算历史交易）。
///
/// - 取数读汇率历史序列 `fx_rate_history`，按交易所属 ISO 周精确命中周键
///   （周一为周键，与 `week_start` 生成列同式，SQL 内派生）；正反向兜底；
///   整周无点即报错——不滑到相邻周、不回落当期表（`exchange_rates` 只服务
///   读路径当期折算 [`convert_to_native_current`]）。
/// - 整周无点的文案区分「该周尚未发布（重试即可）」与「该周历史空缺」。
/// - 币种与默认币种相同 → 1:1 原样返回，且不留痕（两个溯源列为 `None`）。
/// - 基准为 [`default_currency_code`]。
/// - 返回值随行携带折算来源留痕（#1548）：金额 + 使用汇率值 + 来源闭集。
/// - 显式汇率优先（#1549）：`explicit_rate` 为 `Some` 且与本位币异币种时**跳过
///   序列查询**，按给定汇率折算、来源标为 `Explicit`——数据源覆盖不到的日期
///   由调用方逐笔给定；取值方向与折算同向（`native = amount × rate`，与留痕
///   列 `fx_rate_used` 同口径）。不带显式汇率的行与 #1547 行为逐位一致。
/// - 显式汇率非法即码化错误、不落库：非正或非有限报
///   `fx.explicit-rate-non-positive`；与本位币同币种的行无折算方向，携带显式
///   汇率报 `fx.explicit-rate-direction-mismatch`。
/// - 修改路径经 [`convert_to_native_on_edit`] 进来（#1550）：未携显式且三元组
///   未变沿用行内留痕，不直接调用本函数。
pub fn convert_to_native_on_trade_date(
    conn: &Connection,
    amount_cents: i64,
    currency_code: &str,
    trade_date: &str,
    explicit_rate: Option<f64>,
) -> Result<NativeConversion> {
    let target = default_currency_code(conn)?;
    if currency_code == target {
        // 同币种无折算方向：显式汇率无处安放，fail fast 不静默吞掉（#1549）。
        if let Some(rate) = explicit_rate {
            return Err(AppError::codedp(
                "fx.explicit-rate-direction-mismatch",
                format!(
                    "显式汇率方向不符：{currency_code} 即本位币，本笔无折算，不应显式给定汇率（{rate}）"
                ),
                &[currency_code],
            ));
        }
        return Ok(NativeConversion {
            native_cents: amount_cents,
            fx_rate_used: None,
            fx_rate_source: None,
        });
    }
    let (rate, source) = match explicit_rate {
        Some(rate) => {
            if !rate.is_finite() || rate <= 0.0 {
                return Err(AppError::codedp(
                    "fx.explicit-rate-non-positive",
                    format!("显式汇率必须大于 0: {rate}"),
                    &[&rate.to_string()],
                ));
            }
            (rate, FxRateSource::Explicit)
        }
        None => (
            lookup_fx_history_rate(conn, currency_code, &target, trade_date)?,
            FxRateSource::Series,
        ),
    };
    Ok(NativeConversion {
        native_cents: (amount_cents as f64 * rate).round() as i64,
        fx_rate_used: Some(rate),
        fx_rate_source: Some(source),
    })
}

/// **按交易日折算的编辑沿用形态**（写路径修改入口，#1550）：未携显式汇率且
/// 币种、金额、日期三元组与旧行完全一致 → 原样返回行内留痕（含空值），不再
/// 查序列——避免导入的历史行因当前数据源查不到当年值而变成改不动的僵尸行；
/// 任一项变了或调用方逐笔显式给定汇率（#1549：显式 > 沿用 > 序列，显式在场时
/// 沿用不生效，来源改标 `Explicit`）→ 走 [`convert_to_native_on_trade_date`]
/// 重查重算，新周查不到即既有码化错误，不静默沿用旧值。基线与显式均缺席
///（创建路径）与按交易日入口完全一致。
pub fn convert_to_native_on_edit(
    conn: &Connection,
    amount_cents: i64,
    currency_code: &str,
    trade_date: &str,
    explicit_rate: Option<f64>,
    baseline: Option<&FxEditBaseline>,
) -> Result<NativeConversion> {
    if explicit_rate.is_none()
        && let Some(old) = baseline
        && old.amount_cents == amount_cents
        && old.currency_code == currency_code
        && old.date == trade_date
    {
        return Ok(old.conversion);
    }
    convert_to_native_on_trade_date(conn, amount_cents, currency_code, trade_date, explicit_rate)
}

/// 按交易日所属 ISO 周在汇率历史序列查汇率（正查失败则反查取倒数）。
///
/// 周键派生在 SQL 内完成（`date(?,'-6 days','weekday 1')`，与 `week_start`
/// 生成列同式），畸形日期派生 NULL → 不命中，落到整周无点报错路径，不静默。
fn lookup_fx_history_rate(
    conn: &Connection,
    base_code: &str,
    quote_code: &str,
    trade_date: &str,
) -> Result<f64> {
    let week_of = |base: &str, quote: &str| {
        conn.query_row(
            &format!(
                "SELECT rate FROM fx_rate_history WHERE base_code=?1 AND quote_code=?2 \
                 AND week_start = {WEEK_START_EXPR}"
            ),
            rusqlite::params![base, quote, trade_date],
            |r| r.get::<_, f64>(0),
        )
        .optional()
    };
    // 只认「该周无行」为缺数据；其余数据库错误原样上抛，不静默并入缺汇率。
    if let Some(rate) = week_of(base_code, quote_code)? {
        if rate <= 0.0 {
            return Err(AppError::codedp(
                "fx.rate-non-positive",
                format!("汇率 {base_code}->{quote_code} 非正: {rate}"),
                &[base_code, quote_code, &rate.to_string()],
            ));
        }
        return Ok(rate);
    }
    if let Some(rev) = week_of(quote_code, base_code)? {
        if rev <= 0.0 {
            return Err(AppError::codedp(
                "fx.reverse-rate-non-positive",
                format!("反向汇率 {quote_code}->{base_code} 非正: {rev}"),
                &[quote_code, base_code, &rev.to_string()],
            ));
        }
        return Ok(1.0 / rev);
    }
    Err(missing_week_error(conn, base_code, quote_code, trade_date))
}

/// 整周无点的 `fx.rate-missing`：文案区分「该周尚未发布（重试即可）」与
/// 「该周历史空缺」。交易周不早于本周（含未来周）→ 尚未发布，同步后重试即可；
/// 更早的历史周缺点 → 历史空缺，重试无济于事。畸形日期派生不出周键时按
/// 历史空缺处理（写入路径对畸形日期本就应先修正，不静默）。沿用既有码与
/// 参数（base/quote，#1540 spec 错误面），周键只进中文 message。
fn missing_week_error(
    conn: &Connection,
    base_code: &str,
    quote_code: &str,
    trade_date: &str,
) -> AppError {
    let trade_week: Option<String> = conn
        .query_row(
            &format!("SELECT {WEEK_START_EXPR}"),
            rusqlite::params![trade_date],
            |r| r.get(0),
        )
        .unwrap_or(None);
    let current_week: Option<String> = conn
        .query_row(
            "SELECT date('now','localtime','-6 days','weekday 1')",
            [],
            |r| r.get(0),
        )
        .ok();
    // 交易周不早于本周（含未来周）→ 尚未发布；本周判定不可得（查询失败）或
    // 交易周更早（含畸形日期派生不出周键）→ 保守按历史空缺，不误导用户重试。
    let not_yet_published = matches!((&trade_week, &current_week), (Some(w), Some(c)) if w >= c);
    let week = trade_week.unwrap_or_else(|| trade_date.to_string());
    if not_yet_published {
        AppError::codedp(
            "fx.rate-missing",
            format!(
                // 指路文案（issue #1664）：同步可解的分支才指路；「该周历史空缺」
                // 分支同步救不了，保持原样不误导。
                "未找到 {base_code} -> {quote_code} 在 {week} 当周的汇率：该周尚未发布，可在设置页「币种」页签同步汇率后重试"
            ),
            &[base_code, quote_code],
        )
    } else {
        AppError::codedp(
            "fx.rate-missing",
            format!("未找到 {base_code} -> {quote_code} 在 {week} 当周的汇率：该周历史空缺"),
            &[base_code, quote_code],
        )
    }
}
