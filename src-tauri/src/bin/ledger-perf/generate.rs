//! generate 子命令：核心交易域 + 投资域/计划域画像的确定性生成
//! （issue #459/#460 / ADR-0062）。
//!
//! 建库经应用自身的迁移应用路径（[`open_connection`] + [`init_db`]，从 lib 复用，
//! 不复制 DDL）；数据行由本模块批量直插（一次性事务 + 关闭 fsync 的连接级 PRAGMA，
//! 供几十秒内产出 50 万笔）。交易行同步补填 note_pinyin 派生列（V018，与 Writer
//! 接缝同写维护同规则 [`pinyin_initials`]，issue #514：基准库与真实库搜索路径
//! 画像一致，不依赖 bench 预热回填）。全部画像参数以常量集中在本模块头部，测试与
//! 用法注释（bin 头）共用同一套数字；确定性由种子化 PRNG 与「无墙钟、无
//! HashMap 遍历」纪律保证——同参数两次生成，全部生成内容的全表有序摘要一致
//! （迁移种子行的审计时间列不参与比对，tests 内断言）。

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

use chrono::{Datelike, Duration, Months, NaiveDate};
use rusqlite::Connection;

use tauri_app_lib::categories;
use tauri_app_lib::db::{init_db, open_connection};
use tauri_app_lib::transaction::pinyin_initials;

use super::GenerateCli;
use super::investments::{self, MarketData, Portfolio, TradeKind};
use super::plans;
use super::rng::{Rng, time_ordered_id};

// ---------------------------------------------------------------------------
// 画像常量（约数以固定值落地；变更须同步 bin 头注释与 tests）
// ---------------------------------------------------------------------------

/// 数据窗口长度：锚定结束日期前约 5 年。
const WINDOW_MONTHS: u32 = 60;
/// 生成账户数：现金/储蓄/信用卡/钱包混合，含少量外币账户。
const ACCOUNT_TOTAL: usize = 50;
/// 生成分类数（迁移自带的默认种子分类之外另生成）。
const CATEGORY_TOTAL: usize = 40;
/// 商户数与长尾结构：top 20 占挂商户流水的约 60%。
const MERCHANT_TOTAL: usize = 800;
const TOP_MERCHANTS: usize = 20;
const TOP_MERCHANT_FLOW_SHARE: f64 = 0.60;
/// 商户挂载率：支出/退款 85%、收入 30%（工资类收入通常无商户）。
const EXPENSE_MERCHANT_RATE: f64 = 0.85;
const INCOME_MERCHANT_RATE: f64 = 0.30;
/// kind 构成：支出 79.4% / 收入 10% / 转账 8% / 退款 2% / 买入 0.4% / 卖出 0.2%
/// （买入+卖出 = 约 3000 笔标的交易 @50 万笔，见 issue #460；分红/拆股不生成——
/// 产品写入层拒绝且分红以普通 income 承载，词汇表 Investment 词条）。
const EXPENSE_SHARE: f64 = 0.794;
const INCOME_SHARE: f64 = 0.10;
const TRANSFER_SHARE: f64 = 0.08;
// 退款 = 2%（refund 链：refund_of_transaction_id 指向更早的支出）；
// 买入 0.4% + 卖出 0.2% = 约 3000 笔标的交易 @50 万笔（issue #460）。
const REFUND_SHARE: f64 = 0.02;
const BUY_SHARE: f64 = 0.004;
const SELL_SHARE: f64 = 0.002;
// kind 边界（累计份额和恒为 1；assert 钉住画像常量不漂移）。
const INCOME_BOUND: f64 = EXPENSE_SHARE + INCOME_SHARE;
const TRANSFER_BOUND: f64 = INCOME_BOUND + TRANSFER_SHARE;
const REFUND_BOUND: f64 = TRANSFER_BOUND + REFUND_SHARE;
const BUY_BOUND: f64 = REFUND_BOUND + BUY_SHARE;
const SELL_BOUND: f64 = BUY_BOUND + SELL_SHARE;
/// 交易软删除比例（约 1%）。标的交易不软删：产品删除 buy/sell 会回滚批次副作用，
/// 「软删交易行 + 存活批次」不是产品能产出的形态。
const SOFT_DELETE_RATE: f64 = 0.01;
/// 备注挂载率（保证 TransactionSearch 有内容可搜）。
const NOTE_RATE: f64 = 0.40;
const TRANSFER_NOTE_RATE: f64 = 0.20;
/// 标的交易备注挂载率（低于支出：真实用户少给买卖写备注）。
pub(crate) const TRADE_NOTE_RATE: f64 = 0.10;
/// 外币折算基准（exchange_rates 落库值与 amount_native_cents 折算共用，恒一致）。
const USD_CNY: f64 = 7.20;
const EUR_CNY: f64 = 7.85;
/// 港元折算基准（港股户交易的 native 折算与 exchange_rates 落库值共用，恒一致）。
const HKD_CNY: f64 = 0.92;
/// 写入侧设备标识（非真实设备）。
pub(crate) const DEVICE_ID: &str = "ledger-perf";
/// 每账户保留的近期支出退款源上限（环形缓冲）。
const REFUND_BUFFER_CAP: usize = 64;

/// 备注素材池：中英混合 + 纯中文品牌词，供备注随机拼装（TransactionSearch 语料）。
const NOTE_SUBJECTS: [&str; 24] = [
    "和同事午餐",
    "超市购物",
    "打车回家",
    "地铁通勤",
    "买咖啡",
    "网购日用品",
    "健身房年费",
    "电影票",
    "话费充值",
    "水电煤",
    "房租",
    "给爸妈买礼物",
    "早餐煎饼",
    "外卖麻辣烫",
    "grocery run",
    "team dinner",
    "airport taxi",
    "online course",
    "coffee beans",
    "星巴克",
    "瑞幸",
    "盒马",
    "山姆",
    "京东自营",
];
const NOTE_SUFFIXES: [&str; 8] = [
    "",
    "（报销）",
    "（家庭）",
    "-第二批",
    "备用",
    "拼单",
    "加班餐",
    "临时",
];

/// generate 参数（解析后、日期已合法的形态）。
pub(crate) struct GenerateParams {
    pub seed: u64,
    pub transactions: u64,
    pub end_date: NaiveDate,
}

/// 生成结果计数（控制台摘要 + 测试参考）。
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct GenCounts {
    pub accounts: usize,
    pub categories: usize,
    pub merchants: usize,
    pub transactions: usize,
    pub deleted_transactions: usize,
    pub transfer_transactions: usize,
    pub refund_transactions: usize,
    pub exchange_rates: usize,
    pub fx_rate_history: usize,
    /// 投资域（issue #460）。
    pub instruments: usize,
    pub market_prices: usize,
    pub price_history: usize,
    pub buy_trades: usize,
    pub sell_trades: usize,
    /// 预算与定时计划（issue #460）。
    pub budgets: usize,
    pub scheduled_plans: usize,
    pub scheduled_occurrences: usize,
    /// 已完成期次生成的真实交易（从 --transactions 预算中预留）。
    pub scheduled_occurrence_transactions: usize,
}

/// 入口：解析日期、准备输出文件、经迁移建库、生成、打印摘要。
pub(crate) fn run(cli: GenerateCli) -> Result<(), String> {
    let end_date = NaiveDate::parse_from_str(&cli.end_date, "%Y-%m-%d")
        .map_err(|e| format!("--end-date 需要 YYYY-MM-DD 格式：{e}"))?;
    let params = GenerateParams {
        seed: cli.seed,
        transactions: cli.transactions,
        end_date,
    };

    prepare_out_path(&cli.out)?;
    let mut conn = open_connection(&cli.out).map_err(|e| e.to_string())?;
    init_db(&mut conn).map_err(|e| e.to_string())?;

    let started = std::time::Instant::now();
    let counts = generate_into(&mut conn, &params)?;
    println!(
        "生成完成：核心域 {} accounts / {} categories（迁移种子另计）/ {} merchants / {} transactions\
         （软删 {}、转账 {}、退款 {}）",
        counts.accounts,
        counts.categories,
        counts.merchants,
        counts.transactions,
        counts.deleted_transactions,
        counts.transfer_transactions,
        counts.refund_transactions,
    );
    println!(
        "投资与计划：{} instruments / {} market_prices / {} price_history / {} 标的交易\
         （买 {} 卖 {}）/ {} budgets / {} scheduled_plans（{} 期次、{} 期次交易）",
        counts.instruments,
        counts.market_prices,
        counts.price_history,
        counts.buy_trades + counts.sell_trades,
        counts.buy_trades,
        counts.sell_trades,
        counts.budgets,
        counts.scheduled_plans,
        counts.scheduled_occurrences,
        counts.scheduled_occurrence_transactions,
    );
    println!(
        "汇率：{} exchange_rates / {} fx_rate_history（{} 周采样 × 3 币对）",
        counts.exchange_rates,
        counts.fx_rate_history,
        counts.fx_rate_history / 3,
    );
    println!(
        "输出：{}（耗时 {:.1?}）",
        cli.out.display(),
        started.elapsed()
    );
    Ok(())
}

/// 输出路径准备：建父目录；已存在的库文件（含遗留 -wal/-shm）先删除，
/// 保证「从空库迁移 + 幂等重建」。
fn prepare_out_path(out: &Path) -> Result<(), String> {
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建输出目录失败：{e}"))?;
    }
    for suffix in ["", "-wal", "-shm"] {
        let mut name = out.as_os_str().to_os_string();
        name.push(suffix);
        let p = std::path::PathBuf::from(name);
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("删除已存在文件 {} 失败：{e}", p.display())),
        }
    }
    Ok(())
}

/// 在已建库（迁移完成）的连接上生成全部画像数据。
pub(crate) fn generate_into(
    conn: &mut Connection,
    p: &GenerateParams,
) -> Result<GenCounts, String> {
    // 生成期连接级提速 PRAGMA：synchronous/journal_mode 均不持久化进库文件，
    // 生成结束后库文件保持默认形态；生成的库是性能基准耗材，崩溃重跑即可。
    conn.execute_batch(
        "PRAGMA synchronous = OFF;\n\
         PRAGMA journal_mode = MEMORY;\n\
         PRAGMA cache_size = -65536;",
    )
    .map_err(|e| e.to_string())?;

    let start_date = p.end_date - Months::new(WINDOW_MONTHS) + Duration::days(1);
    let total_days = (p.end_date - start_date).num_days() + 1;
    let window = Window {
        start: start_date,
        total_days,
        end: p.end_date,
    };
    let millis = date_millis(&start_date);
    let stamp = format!("{start_date}T08:00:00Z");
    debug_assert!((SELL_BOUND - 1.0).abs() < 1e-9, "kind 份额之和必须为 1");
    let mut rng = Rng::new(p.seed);
    let mut counts = GenCounts::default();

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    // 1) 账户（现金/储蓄/信用卡/钱包/投资混合 + 少量外币户）
    let account_rows = insert_accounts(&tx, &mut rng, &stamp, millis, &mut counts)?;

    // 2) 分类（40 个，支出 30 / 收入 10，两级）+ 读取全量分类池（含迁移种子）
    insert_categories(&tx, &stamp, millis, &mut counts)?;
    let (expense_pool, income_pool) = category_pools(&tx)?;

    // 3) 商户（800 个，长尾结构）
    let (top_merchants, tail_merchants) = insert_merchants(&tx, &stamp, millis, &mut counts)?;

    // 4) 投资域字典与价格线（20 标的 + 周采样历史 + 现价缓存，issue #460）
    let instrument_rows = investments::insert_instruments(&tx, &stamp, millis, &mut counts)?;
    let md = investments::insert_market_data(&tx, &mut rng, &instrument_rows, window, &mut counts)?;

    // 5) 预算与定时计划（少量固定块；期次交易从预算预留，issue #460）
    plans::insert_budgets(&tx, &mut rng, &expense_pool, start_date, &mut counts)?;
    let reserved = plans::insert_scheduled(
        &tx,
        &expense_pool,
        &top_merchants,
        &account_rows,
        p.end_date,
        &mut counts,
    )?;

    // 6) 交易（核心画像 + 标的交易臂；预算扣留期次交易）
    let ctx = TxContext::new(
        &account_rows,
        &expense_pool,
        &income_pool,
        &top_merchants,
        &tail_merchants,
        window,
        md,
    );
    let mut pf = Portfolio::new(ctx.inv.slots.len());
    let txn_counts =
        insert_transactions(&tx, &mut rng, p, &ctx, &instrument_rows, &mut pf, reserved)?;
    counts.transactions = txn_counts.total + reserved as usize;
    counts.deleted_transactions = txn_counts.deleted;
    counts.transfer_transactions = txn_counts.transfers;
    counts.refund_transactions = txn_counts.refunds;
    let (buys, sells) = pf.trade_counts();
    counts.buy_trades = buys;
    counts.sell_trades = sells;

    // 7) 当前汇率 + 全历史周采样汇率（全历史填充供历史折算与走势查询）
    counts.exchange_rates = insert_exchange_rates(&tx, p)?;
    counts.fx_rate_history = insert_fx_rate_history(&tx, &mut rng, p, start_date)?;

    tx.commit().map_err(|e| e.to_string())?;

    // 余额缓存回填（issue #491 / ADR-0067）：生成器绕过 Writer 接缝裸插数据，
    // 对应「存量用户升级后被 V017 回填」形态——读基准走缓存口径，缺缓存行会
    // 报码化错误而非实时聚合。
    tauri_app_lib::accounts::balance::refresh_all_account_balances(conn)
        .map_err(|e| e.to_string())?;

    // 数据落定后全量 ANALYZE（issue #490）：基准库对应「存量用户升级后」形态
    // ——迁移尾部 ANALYZE 在建库时空表运行，统计须随数据重算；时点持仓等
    // join 顺序依赖统计假设，缺统计会让基准失真于真实用户库。
    conn.execute_batch("ANALYZE;").map_err(|e| e.to_string())?;

    Ok(counts)
}

// ---------------------------------------------------------------------------
// 账户
// ---------------------------------------------------------------------------

/// 50 个账户：现金 12 / 储蓄 19（16 CNY + 2 USD + 1 EUR）/ 信用卡 9（8 CNY + 1 USD）/
/// 钱包 4 / 投资 6（5 CNY + 1 HKD），全 CNY 除注明的 3 个外币户。外币户占 3/50 ≈ 6%，
/// 即「USD/EUR 少量」；投资户承接标的交易（同币种纪律见 investments 模块头）。
fn insert_accounts(
    conn: &Connection,
    rng: &mut Rng,
    stamp: &str,
    millis: i64,
    counts: &mut GenCounts,
) -> Result<Vec<AccountRow>, String> {
    let mut specs: Vec<(&'static str, &'static str)> = Vec::new();
    for _ in 0..12 {
        specs.push(("cash", "CNY"));
    }
    for _ in 0..16 {
        specs.push(("bank", "CNY"));
    }
    specs.push(("bank", "USD"));
    specs.push(("bank", "USD"));
    specs.push(("bank", "EUR"));
    for _ in 0..8 {
        specs.push(("credit", "CNY"));
    }
    specs.push(("credit", "USD"));
    for _ in 0..4 {
        specs.push(("ewallet", "CNY"));
    }
    for _ in 0..5 {
        specs.push(("investment", "CNY"));
    }
    specs.push(("investment", "HKD"));
    debug_assert_eq!(specs.len(), ACCOUNT_TOTAL);

    let type_label = |t: &str| match t {
        "cash" => "现金",
        "bank" => "储蓄卡",
        "credit" => "信用卡",
        "investment" => "证券户",
        _ => "电子钱包",
    };
    let initial = |t: &str, rng: &mut Rng| -> i64 {
        match t {
            "cash" => rng.range_i64(5_000, 200_000),
            "bank" => rng.range_i64(1_000_000, 30_000_000),
            "credit" => 0,
            "investment" => rng.range_i64(500_000, 20_000_000),
            _ => rng.range_i64(20_000, 500_000),
        }
    };

    let mut rows: Vec<AccountRow> = Vec::with_capacity(specs.len());
    let mut seq_per_type: BTreeMap<&'static str, u32> = BTreeMap::new();
    for (idx, (atype, ccy)) in specs.iter().enumerate() {
        let seq = seq_per_type.entry(atype).or_insert(0);
        *seq += 1;
        let ccy_code: &'static str = ccy;
        let ccy_suffix = if ccy_code == "CNY" {
            String::new()
        } else {
            format!("-{ccy_code}")
        };
        let name = format!("{}{:02}{ccy_suffix}", type_label(atype), seq);
        let id = time_ordered_id("accounts", idx as u64, millis);
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,\
             created_at,updated_at,version,device_id,is_deleted,is_hidden)\
             VALUES (?1,?2,?3,?4,?5,?6,?6,1,?7,0,0)",
            rusqlite::params![
                id,
                name,
                atype,
                ccy_code,
                initial(atype, rng),
                stamp,
                DEVICE_ID
            ],
        )
        .map_err(|e| e.to_string())?;
        rows.push(AccountRow {
            id,
            ccy: ccy_code,
            atype,
        });
    }
    counts.accounts = rows.len();
    Ok(rows)
}

// ---------------------------------------------------------------------------
// 分类与商户
// ---------------------------------------------------------------------------

/// 40 个生成分类：支出 10 顶级 + 20 二级，收入 4 顶级 + 6 二级；名字带「基准」前缀
/// 与迁移种子默认分类区分。
fn insert_categories(
    conn: &Connection,
    stamp: &str,
    millis: i64,
    counts: &mut GenCounts,
) -> Result<(), String> {
    let expense_tops = [
        "基准餐饮",
        "基准交通",
        "基准购物",
        "基准住房",
        "基准娱乐",
        "基准医疗",
        "基准教育",
        "基准人情",
        "基准数码",
        "基准其他",
    ];
    let income_tops = ["基准工资", "基准理财", "基准分红", "基准其他收入"];

    let insert_one = |conn: &Connection,
                      name: &str,
                      kind: &str,
                      parent: Option<&str>,
                      seq: u64|
     -> Result<String, String> {
        let id = time_ordered_id("categories", seq, millis);
        conn.execute(
            "INSERT INTO categories (id,name,kind,parent_id,icon,sort_order,\
             created_at,updated_at,version,device_id,is_deleted)\
             VALUES (?1,?2,?3,?4,NULL,?5,?6,?6,1,?7,0)",
            rusqlite::params![id, name, kind, parent, seq as i64, stamp, DEVICE_ID],
        )
        .map_err(|e| e.to_string())?;
        Ok(id)
    };

    let mut seq = 0u64;
    for top in expense_tops {
        seq += 1;
        let parent_id = insert_one(conn, top, "expense", None, seq)?;
        for sub in 0..2 {
            seq += 1;
            insert_one(
                conn,
                &format!("{top}细项{}", sub + 1),
                "expense",
                Some(&parent_id),
                seq,
            )?;
        }
    }
    for top in income_tops {
        seq += 1;
        let parent_id = insert_one(conn, top, "income", None, seq)?;
        for sub in 0..1 {
            seq += 1;
            insert_one(
                conn,
                &format!("{top}细项{}", sub + 1),
                "income",
                Some(&parent_id),
                seq,
            )?;
        }
    }
    // 补齐到 40：支出 30 + 收入 8 = 38，再加 2 个收入顶级。
    for extra in 0..2 {
        seq += 1;
        insert_one(
            conn,
            &format!("基准补充收入{}", extra + 1),
            "income",
            None,
            seq,
        )?;
    }
    counts.categories = CATEGORY_TOTAL;
    Ok(())
}

/// 分类池：读取全量在用分类并按 kind 分池（含迁移种子 + 生成分类）。
fn category_pools(conn: &Connection) -> Result<(Vec<String>, Vec<String>), String> {
    let cats = categories::list_categories(conn, false).map_err(|e| e.to_string())?;
    let mut expense = Vec::new();
    let mut income = Vec::new();
    for c in cats {
        if c.kind == "expense" {
            expense.push(c.id);
        } else {
            income.push(c.id);
        }
    }
    Ok((expense, income))
}

/// 800 个商户：top 20 真实感名字（长尾头部）+ 780 个长尾行。
fn insert_merchants(
    conn: &Connection,
    stamp: &str,
    millis: i64,
    counts: &mut GenCounts,
) -> Result<(Vec<String>, Vec<String>), String> {
    const TOP_NAMES: [&str; TOP_MERCHANTS] = [
        "美团",
        "京东",
        "淘宝",
        "拼多多",
        "滴滴出行",
        "饿了么",
        "山姆会员店",
        "盒马鲜生",
        "瑞幸咖啡",
        "星巴克",
        "肯德基",
        "麦当劳",
        "叮咚买菜",
        "铁路12306",
        "携程旅行",
        "中国石油",
        "国家电网",
        "中国移动",
        "苹果商店",
        "网易云音乐",
    ];
    let mut top_ids = Vec::with_capacity(TOP_MERCHANTS);
    let mut tail_ids = Vec::with_capacity(MERCHANT_TOTAL - TOP_MERCHANTS);
    for (i, name) in TOP_NAMES.iter().enumerate() {
        let id = time_ordered_id("merchants", i as u64, millis);
        conn.execute(
            "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted)\
             VALUES (?1,?2,?3,?3,1,?4,0)",
            rusqlite::params![id, name, stamp, DEVICE_ID],
        )
        .map_err(|e| e.to_string())?;
        top_ids.push(id);
    }
    for i in 0..(MERCHANT_TOTAL - TOP_MERCHANTS) {
        let id = time_ordered_id("merchants", (TOP_MERCHANTS + i) as u64, millis);
        let name = format!("长尾商户{:04}", i + 1);
        conn.execute(
            "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted)\
             VALUES (?1,?2,?3,?3,1,?4,0)",
            rusqlite::params![id, name, stamp, DEVICE_ID],
        )
        .map_err(|e| e.to_string())?;
        tail_ids.push(id);
    }
    counts.merchants = top_ids.len() + tail_ids.len();
    Ok((top_ids, tail_ids))
}

// ---------------------------------------------------------------------------
// 交易
// ---------------------------------------------------------------------------

/// 数据窗口：交易与汇率历史共用的日期上下文（起点 / 天数 / 锚定结束日）。
#[derive(Clone, Copy)]
pub(crate) struct Window {
    pub start: NaiveDate,
    pub total_days: i64,
    pub end: NaiveDate,
}

impl Window {
    /// 窗口内均匀随机日期（锥到锚定结束日期内）。
    pub fn rand_date(&self, rng: &mut Rng) -> NaiveDate {
        (self.start + Duration::days(rng.below(self.total_days as u64) as i64)).min(self.end)
    }
}

/// 生成行的账户：id 连同本位币与类型（交易币种/转账同币约束/计划账户选取/
/// 投资槽位都消费它）。
pub(crate) struct AccountRow {
    pub id: String,
    pub ccy: &'static str,
    pub atype: &'static str,
}

/// 退款链的支出源引用（同账户环形缓冲，供 refund 继承账户/分类/商户；币种随账户）。
#[derive(Clone)]
struct ExpenseRef {
    id: String,
    date: NaiveDate,
    amount_cents: i64,
    category: Option<String>,
    merchant: Option<String>,
}

/// 交易生成的共享上下文：账户视图、分类/商户池、日期窗口、投资价格线与
/// 投资账户槽位（行间只读复用）。
struct TxContext<'a> {
    account_ids: Vec<&'a str>,
    ccys: Vec<&'a str>,
    same_ccy: Vec<(&'a str, Vec<usize>)>,
    expense_pool: &'a [String],
    income_pool: &'a [String],
    top_merchants: &'a [String],
    tail_merchants: &'a [String],
    window: Window,
    /// 投资价格线（标的交易取当日周价）。
    md: MarketData,
    /// 投资账户选取：同币种槽位表 + 槽位 → 账户下标。
    inv: InvPick,
}

/// 投资账户选取上下文：槽位（生成账户清单中的下标）按币种分桶，
/// 标的交易按「标的币种 = 账户币种」同币纪律选槽。
struct InvPick {
    ccy_slots: Vec<(&'static str, Vec<usize>)>,
    slots: Vec<usize>,
}

impl InvPick {
    fn new(account_rows: &[AccountRow]) -> Self {
        let mut slots = Vec::new();
        let mut ccy_slots: Vec<(&'static str, Vec<usize>)> = Vec::new();
        for (idx, row) in account_rows.iter().enumerate() {
            if row.atype != "investment" {
                continue;
            }
            let slot = slots.len();
            slots.push(idx);
            match ccy_slots.iter_mut().find(|(c, _)| *c == row.ccy) {
                Some((_, list)) => list.push(slot),
                None => ccy_slots.push((row.ccy, vec![slot])),
            }
        }
        InvPick { ccy_slots, slots }
    }
}

impl<'a> TxContext<'a> {
    /// 由生成的账户行与各池构建；同币种账户索引表供转账挑对手
    /// （转账必须在同币种两账户间进行，跨币种转账非产品语义）；
    /// 投资槽位与价格线供标的交易臂消费（issue #460）。
    fn new(
        account_rows: &'a [AccountRow],
        expense_pool: &'a [String],
        income_pool: &'a [String],
        top_merchants: &'a [String],
        tail_merchants: &'a [String],
        window: Window,
        md: MarketData,
    ) -> Self {
        let mut same_ccy: Vec<(&str, Vec<usize>)> = Vec::new();
        for (idx, row) in account_rows.iter().enumerate() {
            match same_ccy.iter_mut().find(|(c, _)| *c == row.ccy) {
                Some((_, slot)) => slot.push(idx),
                None => same_ccy.push((row.ccy, vec![idx])),
            }
        }
        TxContext {
            account_ids: account_rows.iter().map(|a| a.id.as_str()).collect(),
            ccys: account_rows.iter().map(|a| a.ccy).collect(),
            same_ccy,
            expense_pool,
            income_pool,
            top_merchants,
            tail_merchants,
            window,
            md,
            inv: InvPick::new(account_rows),
        }
    }

    fn ccy_of(&self, idx: usize) -> &'a str {
        self.ccys[idx]
    }

    /// 支出行：对数均匀金额 + 分类/商户/备注挂载；同时成为本账户的退款源。
    fn expense_row(&self, rng: &mut Rng) -> PendingRow {
        let amount = expense_amount(rng);
        let category = Some(rng.pick(self.expense_pool).clone());
        let merchant = maybe_merchant(
            rng,
            self.top_merchants,
            self.tail_merchants,
            EXPENSE_MERCHANT_RATE,
        );
        let note = maybe_note(rng, NOTE_RATE);
        let date = self.window.rand_date(rng);
        PendingRow {
            kind: "expense",
            to_account: None,
            category: category.clone(),
            merchant: merchant.clone(),
            refund_of: None,
            note,
            amount,
            date,
            new_expense: Some(ExpenseRef {
                id: String::new(),
                date,
                amount_cents: amount,
                category,
                merchant,
            }),
        }
    }

    /// 收入行：金额高一个量级、低商户挂载率（工资类收入通常无商户）。
    fn income_row(&self, rng: &mut Rng) -> PendingRow {
        PendingRow {
            kind: "income",
            to_account: None,
            category: Some(rng.pick(self.income_pool).clone()),
            merchant: maybe_merchant(
                rng,
                self.top_merchants,
                self.tail_merchants,
                INCOME_MERCHANT_RATE,
            ),
            refund_of: None,
            note: maybe_note(rng, NOTE_RATE),
            amount: rng.range_i64(50_000, 2_000_000),
            date: self.window.rand_date(rng),
            new_expense: None,
        }
    }

    /// 转账行：同币种两账户间转账；本币种无第二个账户时降级为支出（如唯一的外币户）。
    fn transfer_row(&self, rng: &mut Rng, account_idx: usize) -> PendingRow {
        let ccy = self.ccy_of(account_idx);
        let candidates = self
            .same_ccy
            .iter()
            .find(|(c, _)| *c == ccy)
            .map(|(_, idxs)| idxs)
            .filter(|idxs| idxs.len() >= 2);
        if let Some(idxs) = candidates {
            let mut to = rng.below(idxs.len() as u64) as usize;
            while idxs[to] == account_idx {
                to = rng.below(idxs.len() as u64) as usize;
            }
            PendingRow {
                kind: "transfer",
                to_account: Some(self.account_ids[idxs[to]].to_string()),
                category: None,
                merchant: None,
                refund_of: None,
                note: maybe_note(rng, TRANSFER_NOTE_RATE),
                amount: rng.range_i64(5_000, 2_000_000),
                date: self.window.rand_date(rng),
                new_expense: None,
            }
        } else {
            self.expense_row(rng)
        }
    }
}

/// 一行待插入交易的内存形态（id/审计字段由主循环统一推导）。
struct PendingRow {
    kind: &'static str,
    to_account: Option<String>,
    category: Option<String>,
    merchant: Option<String>,
    refund_of: Option<String>,
    note: Option<String>,
    amount: i64,
    date: NaiveDate,
    /// 支出行携带：带 id 后入本账户退款源缓冲。
    new_expense: Option<ExpenseRef>,
}

struct TxnCounts {
    total: usize,
    deleted: usize,
    transfers: usize,
    refunds: usize,
}

fn insert_transactions(
    conn: &Connection,
    rng: &mut Rng,
    p: &GenerateParams,
    ctx: &TxContext,
    instruments: &[investments::InstrumentRow],
    pf: &mut Portfolio,
    reserved: u64,
) -> Result<TxnCounts, String> {
    const SQL: &str = "INSERT INTO transactions (id,kind,amount_cents,currency_code,\
         amount_native_cents,account_id,to_account_id,category_id,merchant_id,\
         refund_of_transaction_id,note,note_pinyin,dedup_hash,idempotency_key,date,created_at,updated_at,\
         version,device_id,is_deleted,policy_id)\
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,NULL,NULL,?13,?14,?14,1,?15,?16,NULL)";
    let mut stmt = conn.prepare(SQL).map_err(|e| e.to_string())?;

    // 各账户的近期支出环形缓冲（refund 链来源）。
    let mut buffers: Vec<VecDeque<ExpenseRef>> = vec![VecDeque::new(); ctx.account_ids.len()];
    let mut counts = TxnCounts {
        total: 0,
        deleted: 0,
        transfers: 0,
        refunds: 0,
    };

    // 预算扣留计划期次交易（issue #460）：序号接在其后，保证 transactions 表
    // 确定性 id 全局不撞（同 tag 同 seq 同毫秒才同 id）。
    let regular = p.transactions.saturating_sub(reserved);
    let mut seq = reserved;
    for _ in 0..regular {
        seq += 1;
        let account_idx = rng.below(ctx.account_ids.len() as u64) as usize;
        let roll = rng.next_f64();

        // 常规臂产 PendingRow；标的臂（buy/sell）产 PendingRow + 交易计划——
        // 交易行金额/日期由计划给出，批次副作用在行落库后应用（issue #460）。
        let (row, trade): (PendingRow, Option<TradeKind>) = if roll < EXPENSE_SHARE {
            (ctx.expense_row(rng), None)
        } else if roll < INCOME_BOUND {
            (ctx.income_row(rng), None)
        } else if roll < TRANSFER_BOUND {
            (ctx.transfer_row(rng, account_idx), None)
        } else if roll < REFUND_BOUND {
            // 退款链：从同账户近期支出取源（账户/分类/商户继承，币种随账户，
            // 退款日期不早于原支出）；缓冲为空（极小规模）时退化为支出。
            let buffer = &mut buffers[account_idx];
            if buffer.is_empty() {
                (ctx.expense_row(rng), None)
            } else {
                let pick_idx = rng.below(buffer.len() as u64) as usize;
                let src = &buffer[pick_idx];
                let amount = if rng.chance(0.5) {
                    src.amount_cents
                } else {
                    (src.amount_cents / 2).max(1)
                };
                let date = (src.date + Duration::days(rng.below(31) as i64)).min(ctx.window.end);
                (
                    PendingRow {
                        kind: "refund",
                        to_account: None,
                        category: src.category.clone(),
                        merchant: src.merchant.clone(),
                        refund_of: Some(src.id.clone()),
                        note: Some("退款".to_string()),
                        amount,
                        date,
                        new_expense: None,
                    },
                    None,
                )
            }
        } else if roll < BUY_BOUND {
            // 买入：随机标的 → 同币种投资账户 → 当日周价（issue #460）。
            match investments::plan_buy(rng, &ctx.inv.ccy_slots, &ctx.md, ctx.window) {
                Some(t) => {
                    let t = TradeKind::Buy(t);
                    let row = PendingRow {
                        kind: "buy",
                        to_account: None,
                        category: None,
                        merchant: None,
                        refund_of: None,
                        note: maybe_note(rng, TRADE_NOTE_RATE),
                        amount: t.row_amount(),
                        date: t.row_date(),
                        new_expense: None,
                    };
                    (row, Some(t))
                }
                // 结构退化兜底（无同币种投资账户，正常参数下不可达）：降级为支出。
                None => (ctx.expense_row(rng), None),
            }
        } else if roll < SELL_BOUND {
            // 卖出：有剩余量的（账户, 标的）组合中随机取一个，FIFO 匹配；
            // 尚无可卖量（窗口初期的卖出 roll）时降级为买入。
            let plan = match investments::plan_sell(rng, pf, &ctx.md, ctx.window) {
                Some(t) => Some(TradeKind::Sell(Box::new(t))),
                None => investments::plan_buy(rng, &ctx.inv.ccy_slots, &ctx.md, ctx.window)
                    .map(TradeKind::Buy),
            };
            match plan {
                Some(t) => {
                    let kind = if matches!(t, TradeKind::Sell(_)) {
                        "sell"
                    } else {
                        "buy"
                    };
                    let row = PendingRow {
                        kind,
                        to_account: None,
                        category: None,
                        merchant: None,
                        refund_of: None,
                        note: maybe_note(rng, TRADE_NOTE_RATE),
                        amount: t.row_amount(),
                        date: t.row_date(),
                        new_expense: None,
                    };
                    (row, Some(t))
                }
                None => (ctx.expense_row(rng), None),
            }
        } else {
            // 浮点边界之外的兜底臂（份额和恒 1，正常不可达）。
            (ctx.expense_row(rng), None)
        };
        if row.kind == "transfer" {
            counts.transfers += 1;
        } else if row.kind == "refund" {
            counts.refunds += 1;
        }

        // 标的交易的账户/币种由计划给出（投资账户 + 同币种纪律），常规行随随机账户。
        let (ccy, account) = match &trade {
            Some(t) => (t.ccy(), ctx.inv.slots[t.account_slot()]),
            None => (ctx.ccy_of(account_idx), account_idx),
        };

        // 行序号推导确定性 id；新支出带 id 入缓冲（成为后续退款的源）。
        let id = time_ordered_id("transactions", seq, date_millis(&row.date));
        if let Some(exp) = &row.new_expense {
            let mut src = exp.clone();
            src.id = id.clone();
            buffers[account_idx].push_back(src);
        }
        trim_buffer(&mut buffers[account_idx]);

        let created = format!(
            "{}T{:02}:{:02}:{:02}Z",
            row.date,
            rng.range_i64(6, 23),
            rng.range_i64(0, 59),
            rng.range_i64(0, 59)
        );
        let native = (row.amount as f64 * fx(ccy)).round() as i64;
        // 派生列与 Writer 同规则同写（issue #514）：无备注为 NULL，有备注为
        // pinyin_initials(note)（含空串情形，与 Writer 口径一致）。
        let note_pinyin = row.note.as_deref().map(pinyin_initials);
        // 标的交易不软删：产品删除 buy/sell 会回滚批次副作用，
        // 「软删交易行 + 存活批次」不是产品能产出的形态。
        let deleted = trade.is_none() && rng.chance(SOFT_DELETE_RATE);
        if deleted {
            counts.deleted += 1;
        }

        stmt.execute(rusqlite::params![
            id,
            row.kind,
            row.amount,
            ccy,
            native,
            ctx.account_ids[account],
            row.to_account,
            row.category,
            row.merchant,
            row.refund_of,
            row.note,
            note_pinyin,
            row.date.to_string(),
            created,
            DEVICE_ID,
            deleted as i64,
        ])
        .map_err(|e| e.to_string())?;
        if let Some(t) = trade {
            let fx = investments::SideEffects {
                instrument_id: &instruments[t.instrument_idx()].id,
                account_id: ctx.account_ids[account],
                created: &created,
                millis: date_millis(&row.date),
            };
            investments::apply_trade(conn, &id, t, &fx, pf)?;
        }
        counts.total += 1;
    }
    Ok(counts)
}

/// 外币折算基准（与 exchange_rates 落库值共用常量，恒一致）；CNY 为 DefaultCurrency。
fn fx(ccy: &str) -> f64 {
    match ccy {
        "USD" => USD_CNY,
        "EUR" => EUR_CNY,
        "HKD" => HKD_CNY,
        _ => 1.0,
    }
}

/// 支出金额：1.5–5 个数量级对数均匀（约 ¥0.3–¥316），形似日常长尾。
fn expense_amount(rng: &mut Rng) -> i64 {
    (10f64.powf(1.5 + rng.next_f64() * 3.5)).round() as i64
}

/// 挂商户：rate 概率挂载；挂载时 top 20 池占 TOP_MERCHANT_FLOW_SHARE（约 60%），
/// 其余走长尾池——两段式构成「头部集中、尾部广阔」的长尾画像。
fn maybe_merchant(rng: &mut Rng, top: &[String], tail: &[String], rate: f64) -> Option<String> {
    if !rng.chance(rate) {
        return None;
    }
    if rng.chance(TOP_MERCHANT_FLOW_SHARE) {
        Some(rng.pick(top).clone())
    } else {
        Some(rng.pick(tail).clone())
    }
}

/// 挂备注：主体 + 后缀拼装（搜索语料）。
fn maybe_note(rng: &mut Rng, rate: f64) -> Option<String> {
    if !rng.chance(rate) {
        return None;
    }
    Some(format!(
        "{}{}",
        rng.pick(&NOTE_SUBJECTS),
        rng.pick(&NOTE_SUFFIXES)
    ))
}

/// 保留每账户最近 REFUND_BUFFER_CAP 条支出作为退款源。
fn trim_buffer(buf: &mut VecDeque<ExpenseRef>) {
    while buf.len() > REFUND_BUFFER_CAP {
        buf.pop_front();
    }
}

pub(crate) fn date_millis(d: &NaiveDate) -> i64 {
    d.and_hms_opt(12, 0, 0)
        .map(|dt| dt.and_utc().timestamp_millis())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// 汇率
// ---------------------------------------------------------------------------

/// 当前汇率三行（USD/EUR/HKD → CNY；折算基准与交易 native 列共用常量，恒一致；
/// HKD 行供港股市值在净资产/可投资资产聚合层的折算，issue #460）。
fn insert_exchange_rates(conn: &Connection, p: &GenerateParams) -> Result<usize, String> {
    let millis = date_millis(&p.end_date);
    let priced_at = format!("{}T16:00:00Z", p.end_date);
    for (seq, (base, rate)) in [
        (0u64, ("USD", USD_CNY)),
        (1, ("EUR", EUR_CNY)),
        (2, ("HKD", HKD_CNY)),
    ] {
        conn.execute(
            "INSERT OR REPLACE INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,\
             updated_at,version,device_id) VALUES (?1,?2,?3,?4,?5,'perf',?5,1,?6)",
            rusqlite::params![
                time_ordered_id("exchange_rates", seq, millis),
                base,
                "CNY",
                rate,
                priced_at,
                DEVICE_ID
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(3)
}

/// fx_rate_history 全历史填充：窗口内每周一采样一条（trade_date 取当周周五、
/// 不足则钳到锚定结束日），USD/EUR/HKD 三币对，汇率围绕基准 ±2% 种子化漂移。
/// 「币种对 × 周唯一」由 UNIQUE(base,quote,week_start) 保证，每周恰一行不冲突。
fn insert_fx_rate_history(
    conn: &Connection,
    rng: &mut Rng,
    p: &GenerateParams,
    start_date: NaiveDate,
) -> Result<usize, String> {
    let sql = "INSERT INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,\
         created_at,updated_at,version,device_id) VALUES (?1,?2,?3,?4,?5,'perf',?6,?6,1,?7)";
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let mut monday =
        start_date - Duration::days(start_date.weekday().num_days_from_monday() as i64);
    let mut seq = 0u64;
    let mut inserted = 0usize;
    while monday <= p.end_date {
        let trade_date = (monday + Duration::days(4)).min(p.end_date);
        let created = format!("{trade_date}T16:00:00Z");
        for (base, base_rate) in [("USD", USD_CNY), ("EUR", EUR_CNY), ("HKD", HKD_CNY)] {
            let drift = 1.0 + (rng.next_f64() - 0.5) * 0.04;
            let rate = (base_rate * drift * 1e6).round() / 1e6;
            stmt.execute(rusqlite::params![
                time_ordered_id("fx_rate_history", seq, date_millis(&trade_date)),
                base,
                "CNY",
                trade_date.to_string(),
                rate,
                created,
                DEVICE_ID
            ])
            .map_err(|e| e.to_string())?;
            seq += 1;
            inserted += 1;
        }
        monday += Duration::days(7);
    }
    Ok(inserted)
}
