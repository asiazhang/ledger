//! 仪表盘域（issue #142；#405 域目录化 ADR-0056）：首页净资产跨币种合计，
//! 只读聚合。
//!
//! 口径（ADR-0020，真实财富视角；第三腿 ADR-0064 决策 6）：
//! 净资产 = Σ 非投资账户折本位币余额 + Σ 折本位币持仓市值
//!   + Σ 在持实物资产估值折本位币。
//! - 账户侧余额沿用 `db::balance::list_account_balances_with_visibility`
//!   （内部即 `account_flow` 口径的 `compute_all_balances_with_visibility`，
//!   与账户列表/余额页一致，排除隐藏与黑洞账户），并剔除投资账户（其价值
//!   经持仓市值计入，避免同一笔资产重复计算）；
//! - 持仓侧读 `v_holdings` 视图市值（账户本位币），再折算到全局默认币种；
//!   `market_value_cents` 为 NULL（从未录价或缺折算汇率）时按空值语义跳过；
//! - 币种折算一律复用 [`amount::convert_to_native`]，缺汇率错误上抛
//!   （中文错误信息），不静默混币种；
//! - 实物资产腿复用 [`crate::physical_asset`] 域 API 的在持合计读口径
//!   （`list_physical_assets` 的 `holding_total_native_cents`，最新估值行经
//!   Amount 接缝折算、缺汇率错误上抛、已处置 / 软删不计入），与实物资产
//!   列表「家底合计」同源不漂移；
//! - 投资域不新增任何写函数（ADR-0013）；可投资资产（财务自由度分子，
//!   ADR-0048）不受实物资产影响。
//!
//! 核心函数吃 `&Connection` 可直接单测/供 e2e 复用；IPC 参数解包与连接锁
//! 管理在壳层 `commands::dashboard`（#405 压平为单文件纯壳）。依赖方向恒为
//! 「壳层 → dashboard → 基础设施」，本模块不反向依赖壳层。净资产总览
//! 读模型集中本域 [`model`]（#421 随域归位），消费方经域路径逐类型显式 import。

mod model;

pub use model::DashboardOverview;

use rusqlite::Connection;

use crate::accounts::AccountType;
use crate::db::balance::list_account_balances_with_visibility;
use crate::db::net_worth::{self, CachedNetWorth};
use crate::db::query::{FromRow, query_all};
use crate::error::Result;
use crate::physical_asset;
use crate::transaction::amount;

impl From<&CachedNetWorth> for DashboardOverview {
    fn from(cached: &CachedNetWorth) -> Self {
        DashboardOverview {
            native_currency: cached.native_currency.clone(),
            net_worth_cents: cached.net_worth_cents,
            accounts_balance_cents: cached.accounts_balance_cents,
            holdings_market_value_cents: cached.holdings_market_value_cents,
            physical_assets_value_cents: cached.physical_assets_value_cents,
        }
    }
}

/// 持仓市值行：`v_holdings` 市值（账户本位币，可为 NULL）+ 账户币种。
/// 与投资域 `financial_freedom::HoldingValue` 同形片段刻意不收拢（跨域共享
/// 待共享持仓市值合计立项时统一提取，见对向注释）。
struct HoldingValue {
    market_value_cents: Option<i64>,
    currency_code: String,
}

impl FromRow for HoldingValue {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(HoldingValue {
            market_value_cents: row.get(0)?,
            currency_code: row.get(1)?,
        })
    }
}

/// conn 级聚合：净资产总览读探针（issue #491 / ADR-0067，只读 + 终值回填）。
///
/// 先算当前输入指纹（各贡献表 MAX(updated_at) 组合）：与缓存终值一致则直接
/// 返回；不一致（或无缓存行）则调下方既有实时聚合重算并回填缓存——指纹与
/// 缓存收口在 [`net_worth`]（只包存储），聚合公式仍在本域，无定时任务。
pub fn query_dashboard_overview(conn: &Connection) -> Result<DashboardOverview> {
    let fingerprint = net_worth::current_fingerprint(conn)?;
    if let Some(cached) = net_worth::read_valid(conn, &fingerprint)? {
        // 基准币种与当前一致才可信（缓存跨币种设置变更不成立时重算）。
        if cached.native_currency == amount::default_currency_code() {
            return Ok((&cached).into());
        }
    }
    let overview = compute_dashboard_overview(conn)?;
    net_worth::write(
        conn,
        &fingerprint,
        &CachedNetWorth {
            native_currency: overview.native_currency.clone(),
            net_worth_cents: overview.net_worth_cents,
            accounts_balance_cents: overview.accounts_balance_cents,
            holdings_market_value_cents: overview.holdings_market_value_cents,
            physical_assets_value_cents: overview.physical_assets_value_cents,
        },
    )?;
    Ok(overview)
}

/// 既有实时聚合公式（本位币净资产总览）：三腿合计，口径不变——余额腿自
/// B2 起读余额缓存（五出口切缓存），其余两腿仍实时聚合。
fn compute_dashboard_overview(conn: &Connection) -> Result<DashboardOverview> {
    // 非投资账户余额合计：余额口径与账户列表一致（account_flow，排除隐藏/黑洞）。
    let mut accounts_sum = 0i64;
    for ab in list_account_balances_with_visibility(conn, false)? {
        if ab.account.kind == AccountType::Investment {
            continue;
        }
        accounts_sum +=
            amount::convert_to_native(conn, ab.balance_cents, &ab.account.currency_code)?;
    }

    // 持仓市值合计：v_holdings 市值（账户本位币）→ 全局默认币种；NULL 市值跳过。
    let holdings: Vec<HoldingValue> = query_all(
        conn,
        "SELECT h.market_value_cents, a.currency_code \
         FROM v_holdings h JOIN accounts a ON a.id = h.account_id",
        [],
    )?;
    let mut holdings_sum = 0i64;
    for h in holdings {
        if let Some(market_value_cents) = h.market_value_cents {
            holdings_sum += amount::convert_to_native(conn, market_value_cents, &h.currency_code)?;
        }
    }

    // 在持实物资产估值合计（第三腿，ADR-0064 决策 6）：经实物资产域单一读
    // 口径取数（缺汇率错误上抛，不静默漏算；已处置 / 软删不计入），与实物
    // 资产列表「家底合计」同源。
    let physical_assets_value_cents =
        physical_asset::list_physical_assets(conn, None)?.holding_total_native_cents;

    Ok(DashboardOverview {
        native_currency: amount::default_currency_code().to_string(),
        net_worth_cents: accounts_sum + holdings_sum + physical_assets_value_cents,
        accounts_balance_cents: accounts_sum,
        holdings_market_value_cents: holdings_sum,
        physical_assets_value_cents,
    })
}
