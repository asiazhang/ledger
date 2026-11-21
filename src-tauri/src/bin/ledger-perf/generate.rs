use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};

use chrono::{Datelike, Duration, Months, NaiveDate};
use rusqlite::Connection;

use ledger_categories as categories;
use ledger_infra::db::connection::is_main_db_path;
use ledger_infra::db::{init_db, open_connection, sync_db_path};

use super::bench_common::{FlagSpec, Parsed, parse_flags, parse_nonneg_int};
use super::books;
use super::investments::{self, MarketData, Portfolio, TradeKind};
use super::plans;
use super::rng::{Rng, time_ordered_id};
use super::{DEFAULT_END_DATE, DEFAULT_SEED, DEFAULT_TRANSACTIONS, default_out};

const WINDOW_MONTHS: u32 = 60;
const ACCOUNT_TOTAL: usize = 50;
const CATEGORY_TOTAL: usize = 40;
const MERCHANT_TOTAL: usize = 800;
const TOP_MERCHANTS: usize = 20;
const TOP_MERCHANT_FLOW_SHARE: f64 = 0.60;
const EXPENSE_MERCHANT_RATE: f64 = 0.85;
const INCOME_MERCHANT_RATE: f64 = 0.30;
const EXPENSE_SHARE: f64 = 0.794;
const INCOME_SHARE: f64 = 0.10;
const TRANSFER_SHARE: f64 = 0.08;
const REFUND_SHARE: f64 = 0.02;
const BUY_SHARE: f64 = 0.004;
const SELL_SHARE: f64 = 0.002;
const INCOME_BOUND: f64 = EXPENSE_SHARE + INCOME_SHARE;
const TRANSFER_BOUND: f64 = INCOME_BOUND + TRANSFER_SHARE;
const REFUND_BOUND: f64 = TRANSFER_BOUND + REFUND_SHARE;
const BUY_BOUND: f64 = REFUND_BOUND + BUY_SHARE;
const SELL_BOUND: f64 = BUY_BOUND + SELL_SHARE;
const SOFT_DELETE_RATE: f64 = 0.01;
const NOTE_RATE: f64 = 0.40;
const TRANSFER_NOTE_RATE: f64 = 0.20;
pub(crate) const TRADE_NOTE_RATE: f64 = 0.10;
const USD_CNY: f64 = 7.20;
const EUR_CNY: f64 = 7.85;
const HKD_CNY: f64 = 0.92;
pub(crate) const DEVICE_ID: &str = "ledger-perf";
const REFUND_BUFFER_CAP: usize = 64;

pub(crate) const NOTE_SUBJECTS: [&str; 24] = [
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
pub(crate) const NOTE_SUFFIXES: [&str; 8] = [
    "",
    "（报销）",
    "（家庭）",
    "-第二批",
    "备用",
    "拼单",
    "加班餐",
    "临时",
];
const PURCHASE_ORDER_RATE: f64 = 0.10;
pub(crate) const PURCHASE_ITEM_MAX: u64 = 4;
pub(crate) const PURCHASE_BRANDS: [&str; 26] = [
    "蓝月亮",
    "立白",
    "维达",
    "清风",
    "心相印",
    "洁柔",
    "舒肤佳",
    "佳洁士",
    "高露洁",
    "海飞丝",
    "潘婷",
    "多芬",
    "大宝",
    "百事",
    "可口可乐",
    "乐事",
    "奥利奥",
    "好丽友",
    "徐福记",
    "卫龙",
    "伊利",
    "蒙牛",
    "桃李",
    "盼盼",
    "农夫山泉",
    "怡宝",
];
pub(crate) const PURCHASE_CATEGORIES: [&str; 15] = [
    "洗衣液",
    "纸巾",
    "抽纸",
    "牙膏",
    "洗发水",
    "沐浴露",
    "香皂",
    "湿巾",
    "碳酸饮料",
    "薯片",
    "饼干",
    "牛奶",
    "面包",
    "矿泉水",
    "辣条",
];
pub(crate) const PURCHASE_SPECS: [&str; 3] = ["", "量贩装", "旅行装"];
pub(crate) const PURCHASE_REPEAT_SKUS: [&str; 10] = [
    "渴望猫粮 1.8kg",
    "冻干猫粮 500g",
    "豆腐猫砂 20L",
    "鲜鸡蛋 10枚",
    "加厚垃圾袋 100只",
    "饮用水 550ml×12",
    "卷纸 12卷",
    "酿造酱油 500ml",
    "东北大米 5kg",
    "红烧牛肉面 5连包",
];

pub(crate) fn purchase_name_pool() -> Vec<String> {
    let mut names: Vec<String> = PURCHASE_BRANDS
        .iter()
        .enumerate()
        .flat_map(|(bi, brand)| {
            PURCHASE_CATEGORIES
                .iter()
                .enumerate()
                .map(move |(ci, category)| {
                    format!(
                        "{brand}{category}{}",
                        PURCHASE_SPECS[(bi + ci) % PURCHASE_SPECS.len()]
                    )
                })
        })
        .collect();
    names.extend(PURCHASE_REPEAT_SKUS.iter().map(|s| s.to_string()));
    names
}
pub(crate) struct GenerateParams {
    pub seed: u64,
    pub transactions: u64,
    pub end_date: NaiveDate,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct GenCounts {
    pub accounts: usize,
    pub categories: usize,
    pub merchants: usize,
    pub transactions: usize,
    pub purchases: usize,
    pub deleted_transactions: usize,
    pub transfer_transactions: usize,
    pub refund_transactions: usize,
    pub exchange_rates: usize,
    pub fx_rate_history: usize,
    pub instruments: usize,
    pub market_prices: usize,
    pub price_history: usize,
    pub buy_trades: usize,
    pub sell_trades: usize,
    pub budgets: usize,
    pub scheduled_plans: usize,
    pub scheduled_occurrences: usize,
    pub scheduled_occurrence_transactions: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GenerateCli {
    pub seed: u64,
    pub transactions: u64,
    pub end_date: String,
    pub out: PathBuf,
}

impl Default for GenerateCli {
    fn default() -> Self {
        GenerateCli {
            seed: DEFAULT_SEED,
            transactions: DEFAULT_TRANSACTIONS,
            end_date: DEFAULT_END_DATE.to_string(),
            out: default_out(),
        }
    }
}

pub(crate) const FLAGS: &[FlagSpec<GenerateCli>] = &[
    FlagSpec {
        flag: "--seed <N>",
        help: "随机种子（默认 42，同种子必出同库）",
        apply: |cli, flag, v| {
            cli.seed = parse_nonneg_int(flag, v)?;
            Ok(())
        },
    },
    FlagSpec {
        flag: "--transactions <N>",
        help: "生成笔数（默认 500000）",
        apply: |cli, flag, v| {
            cli.transactions = parse_nonneg_int(flag, v)?;
            Ok(())
        },
    },
    FlagSpec {
        flag: "--end-date <YYYY-MM-DD>",
        help: "数据窗口锚定结束日期（默认 2025-12-31，不锚定「今天」）",
        apply: |cli, _flag, v| {
            cli.end_date = v.to_string();
            Ok(())
        },
    },
    FlagSpec {
        flag: "--out <PATH>",
        help: "输出库文件路径（默认 src-tauri/target/ledger-perf/ledger-perf.db；已存在会先删除再重建）",
        apply: |cli, _flag, v| {
            cli.out = PathBuf::from(v);
            Ok(())
        },
    },
];

pub(crate) fn parse_generate_args(args: &[String]) -> Result<Parsed<GenerateCli>, String> {
    parse_flags(args, FLAGS)
}

pub(crate) fn execute(args: &[String]) -> super::bench_common::Outcome {
    super::bench_common::execute_cli(args, parse_generate_args, run)
}

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
    if !is_main_db_path(&cli.out) {
        let sync_path = sync_db_path(&cli.out);
        conn.execute_batch(&format!(
            "ATTACH DATABASE '{}' AS sync",
            sync_path.display()
        ))
        .map_err(|e| format!("伴生同步件挂载失败（{}）：{e}", sync_path.display()))?;
    }
    init_db(&mut conn).map_err(|e| e.to_string())?;

    let started = std::time::Instant::now();
    let counts = generate_into(&mut conn, &params)?;
    drop(conn);

    let books_dirs = books::generate_attached_books(&cli.out, &params)?;

    println!(
        "生成完成：核心域 {} accounts / {} categories（迁移种子另计）/ {} merchants / {} transactions\
         （软删 {}、转账 {}、退款 {}）/ 购买项 {} 行",
        counts.accounts,
        counts.categories,
        counts.merchants,
        counts.transactions,
        counts.deleted_transactions,
        counts.transfer_transactions,
        counts.refund_transactions,
        counts.purchases,
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
        "附属账本：{} 本 × {} 交易（{}）",
        books_dirs.len(),
        books::ATTACHED_BOOK_TRANSACTIONS,
        books::attached_books_root(&cli.out).display(),
    );
    println!(
        "输出：{}（耗时 {:.1?}）",
        cli.out.display(),
        started.elapsed()
    );
    Ok(())
}

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

pub(crate) fn generate_into(
    conn: &mut Connection,
    p: &GenerateParams,
) -> Result<GenCounts, String> {
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

    let account_rows = insert_accounts(&tx, &mut rng, &stamp, millis, &mut counts)?;

    insert_categories(&tx, &stamp, millis, &mut counts)?;
    let (expense_pool, income_pool) = category_pools(&tx)?;

    let (top_merchants, tail_merchants) = insert_merchants(&tx, &stamp, millis, &mut counts)?;

    let instrument_rows = investments::insert_instruments(&tx, &stamp, millis, &mut counts)?;
    let md = investments::insert_market_data(&tx, &mut rng, &instrument_rows, window, &mut counts)?;

    plans::insert_budgets(&tx, &mut rng, &expense_pool, start_date, &mut counts)?;
    let reserved = plans::insert_scheduled(
        &tx,
        &expense_pool,
        &top_merchants,
        &account_rows,
        p.end_date,
        &mut counts,
    )?;

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

    insert_purchases(&tx, p, &mut counts)?;

    counts.exchange_rates = insert_exchange_rates(&tx, p)?;
    counts.fx_rate_history = insert_fx_rate_history(&tx, &mut rng, p, start_date)?;

    tx.commit().map_err(|e| e.to_string())?;

    ledger_accounts::balance::refresh_all_account_balances(conn).map_err(|e| e.to_string())?;

    conn.execute_batch("ANALYZE;").map_err(|e| e.to_string())?;

    Ok(counts)
}

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

#[derive(Clone, Copy)]
pub(crate) struct Window {
    pub start: NaiveDate,
    pub total_days: i64,
    pub end: NaiveDate,
}

impl Window {
    pub fn rand_date(&self, rng: &mut Rng) -> NaiveDate {
        (self.start + Duration::days(rng.below(self.total_days as u64) as i64)).min(self.end)
    }
}

pub(crate) struct AccountRow {
    pub id: String,
    pub ccy: &'static str,
    pub atype: &'static str,
}

#[derive(Clone)]
struct ExpenseRef {
    id: String,
    date: NaiveDate,
    amount_cents: i64,
    category: Option<String>,
    merchant: Option<String>,
}

struct TxContext<'a> {
    account_ids: Vec<&'a str>,
    ccys: Vec<&'a str>,
    same_ccy: Vec<(&'a str, Vec<usize>)>,
    expense_pool: &'a [String],
    income_pool: &'a [String],
    top_merchants: &'a [String],
    tail_merchants: &'a [String],
    window: Window,
    md: MarketData,
    inv: InvPick,
}

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

struct PendingRow {
    kind: &'static str,
    to_account: Option<String>,
    category: Option<String>,
    merchant: Option<String>,
    refund_of: Option<String>,
    note: Option<String>,
    amount: i64,
    date: NaiveDate,
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
         refund_of_transaction_id,note,dedup_hash,idempotency_key,date,created_at,updated_at,\
         version,device_id,is_deleted,policy_id)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,NULL,NULL,?12,?13,?13,1,?14,?15,NULL)";
    let mut stmt = conn.prepare(SQL).map_err(|e| e.to_string())?;

    let mut buffers: Vec<VecDeque<ExpenseRef>> = vec![VecDeque::new(); ctx.account_ids.len()];
    let mut counts = TxnCounts {
        total: 0,
        deleted: 0,
        transfers: 0,
        refunds: 0,
    };

    let regular = p.transactions.saturating_sub(reserved);
    let mut seq = reserved;
    for _ in 0..regular {
        seq += 1;
        let account_idx = rng.below(ctx.account_ids.len() as u64) as usize;
        let roll = rng.next_f64();

        let (row, trade): (PendingRow, Option<TradeKind>) = if roll < EXPENSE_SHARE {
            (ctx.expense_row(rng), None)
        } else if roll < INCOME_BOUND {
            (ctx.income_row(rng), None)
        } else if roll < TRANSFER_BOUND {
            (ctx.transfer_row(rng, account_idx), None)
        } else if roll < REFUND_BOUND {
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
                None => (ctx.expense_row(rng), None),
            }
        } else if roll < SELL_BOUND {
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
            (ctx.expense_row(rng), None)
        };
        if row.kind == "transfer" {
            counts.transfers += 1;
        } else if row.kind == "refund" {
            counts.refunds += 1;
        }

        let (ccy, account) = match &trade {
            Some(t) => (t.ccy(), ctx.inv.slots[t.account_slot()]),
            None => (ctx.ccy_of(account_idx), account_idx),
        };

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

fn fx(ccy: &str) -> f64 {
    match ccy {
        "USD" => USD_CNY,
        "EUR" => EUR_CNY,
        "HKD" => HKD_CNY,
        _ => 1.0,
    }
}

fn expense_amount(rng: &mut Rng) -> i64 {
    (10f64.powf(1.5 + rng.next_f64() * 3.5)).round() as i64
}

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

fn trim_buffer(buf: &mut VecDeque<ExpenseRef>) {
    while buf.len() > REFUND_BUFFER_CAP {
        buf.pop_front();
    }
}

fn insert_purchases(
    conn: &Connection,
    p: &GenerateParams,
    counts: &mut GenCounts,
) -> Result<(), String> {
    let mut rng = Rng::new(p.seed ^ 0x1885_0000_0000_0001);
    let names = purchase_name_pool();
    let mut stmt = conn
        .prepare(
            "INSERT INTO transaction_purchases \
             (transaction_id,sort,name,quantity,category_id,unit_price_cents) \
             VALUES (?1,?2,?3,?4,NULL,?5)",
        )
        .map_err(|e| e.to_string())?;
    let ids: Vec<String> = {
        let mut q = conn
            .prepare("SELECT id FROM transactions WHERE kind = 'expense' ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = q.query_map([], |r| r.get(0)).map_err(|e| e.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?
    };
    for id in ids {
        if !rng.chance(PURCHASE_ORDER_RATE) {
            continue;
        }
        let items = 1 + rng.below(PURCHASE_ITEM_MAX);
        for sort in 0..items {
            let name = rng.pick(&names);
            let quantity = (1 + rng.below(3)) as i64;
            let price = if rng.chance(0.3) {
                None
            } else {
                Some(rng.range_i64(300, 50_000))
            };
            stmt.execute(rusqlite::params![id, sort as i64, name, quantity, price])
                .map_err(|e| e.to_string())?;
            counts.purchases += 1;
        }
    }
    Ok(())
}

pub(crate) fn date_millis(d: &NaiveDate) -> i64 {
    d.and_hms_opt(12, 0, 0)
        .map(|dt| dt.and_utc().timestamp_millis())
        .unwrap_or(0)
}

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
