//! generate 子命令：核心交易域性能画像的确定性生成（issue #459 / ADR-0062）。
//!
//! 建库经应用自身的迁移应用路径（[`open_connection`] + [`init_db`]，从 lib 复用，
//! 不复制 DDL）；数据行由本模块批量直插（一次性事务 + 关闭 fsync 的连接级 PRAGMA，
//! 供几十秒内产出 50 万笔）。全部画像参数以常量集中在本模块头部，测试与
//! 用法注释（bin 头）共用同一套数字；确定性由种子化 PRNG 与「无墙钟、无
//! HashMap 遍历」纪律保证——同参数两次生成的库逐行同构（tests 内摘要断言）。

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

use chrono::{Datelike, Duration, Months, NaiveDate};
use rusqlite::Connection;

use tauri_app_lib::categories;
use tauri_app_lib::db::{init_db, open_connection};

use super::GenerateCli;
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
/// kind 构成：支出 80% / 收入 10% / 转账 8% / 退款 2%（其余 kind 属投资域，见 issue #460）。
const EXPENSE_SHARE: f64 = 0.80;
const INCOME_SHARE: f64 = 0.10;
const TRANSFER_SHARE: f64 = 0.08;
// 退款 = 其余 2%（refund 链：refund_of_transaction_id 指向更早的支出）。
/// 交易软删除比例（约 1%）。
const SOFT_DELETE_RATE: f64 = 0.01;
/// 备注挂载率（保证 TransactionSearch 有内容可搜）。
const NOTE_RATE: f64 = 0.40;
const TRANSFER_NOTE_RATE: f64 = 0.20;
/// 外币折算基准（exchange_rates 落库值与 amount_native_cents 折算共用，恒一致）。
const USD_CNY: f64 = 7.20;
const EUR_CNY: f64 = 7.85;
/// 写入侧设备标识（非真实设备）。
const DEVICE_ID: &str = "ledger-perf";
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
        "生成完成：{} accounts / {} categories（迁移种子另计）/ {} merchants / {} transactions（软删 {}、转账 {}、退款 {}）/ {} fx_rate_history（{} 周采样 × 2 币对）",
        counts.accounts,
        counts.categories,
        counts.merchants,
        counts.transactions,
        counts.deleted_transactions,
        counts.transfer_transactions,
        counts.refund_transactions,
        counts.fx_rate_history,
        counts.fx_rate_history / 2,
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
    let stamp = format!("{start_date}T08:00:00Z");
    let mut rng = Rng::new(p.seed);
    let mut counts = GenCounts::default();

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    // 1) 账户（现金/储蓄/信用卡/钱包混合 + 少量 USD/EUR）
    let account_rows = insert_accounts(&tx, &mut rng, &stamp, &start_date, &mut counts)?;

    // 2) 分类（40 个，支出 30 / 收入 10，两级）+ 读取全量分类池（含迁移种子）
    insert_categories(&tx, &stamp, &mut counts)?;
    let (expense_pool, income_pool) = category_pools(&tx)?;

    // 3) 商户（800 个，长尾结构）
    let (top_merchants, tail_merchants) = insert_merchants(&tx, &stamp, &mut counts)?;

    // 4) 交易（核心画像）
    let txn_counts = insert_transactions(
        &tx,
        &mut rng,
        p,
        &account_rows,
        &expense_pool,
        &income_pool,
        &top_merchants,
        &tail_merchants,
        start_date,
        total_days,
    )?;
    counts.transactions = txn_counts.total;
    counts.deleted_transactions = txn_counts.deleted;
    counts.transfer_transactions = txn_counts.transfers;
    counts.refund_transactions = txn_counts.refunds;

    // 5) 当前汇率 + 全历史周采样汇率（全历史填充供历史折算与走势查询）
    counts.exchange_rates = insert_exchange_rates(&tx, p)?;
    counts.fx_rate_history = insert_fx_rate_history(&tx, &mut rng, p, start_date)?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(counts)
}

// ---------------------------------------------------------------------------
// 账户
// ---------------------------------------------------------------------------

/// 50 个账户：现金 12 / 储蓄 22（19 CNY + 2 USD + 1 EUR）/ 信用卡 10（9 CNY + 1 USD）/
/// 钱包 6，全 CNY 除注明的 3 个外币户。外币户占 3/50 ≈ 6%，即「USD/EUR 少量」。
fn insert_accounts(
    conn: &Connection,
    rng: &mut Rng,
    stamp: &str,
    start_date: &NaiveDate,
    counts: &mut GenCounts,
) -> Result<Vec<AccountRow>, String> {
    let mut specs: Vec<(&'static str, &'static str)> = Vec::new();
    for _ in 0..12 {
        specs.push(("cash", "CNY"));
    }
    for _ in 0..19 {
        specs.push(("bank", "CNY"));
    }
    specs.push(("bank", "USD"));
    specs.push(("bank", "USD"));
    specs.push(("bank", "EUR"));
    for _ in 0..9 {
        specs.push(("credit", "CNY"));
    }
    specs.push(("credit", "USD"));
    for _ in 0..6 {
        specs.push(("ewallet", "CNY"));
    }
    debug_assert_eq!(specs.len(), ACCOUNT_TOTAL);

    let type_label = |t: &str| match t {
        "cash" => "现金",
        "bank" => "储蓄卡",
        "credit" => "信用卡",
        _ => "电子钱包",
    };
    let initial = |t: &str, rng: &mut Rng| -> i64 {
        match t {
            "cash" => rng.range_i64(5_000, 200_000),
            "bank" => rng.range_i64(1_000_000, 30_000_000),
            "credit" => 0,
            _ => rng.range_i64(20_000, 500_000),
        }
    };

    let mut rows: Vec<AccountRow> = Vec::with_capacity(specs.len());
    let mut seq_per_type: BTreeMap<&'static str, u32> = BTreeMap::new();
    let millis = date_millis(start_date);
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
        rows.push(AccountRow { id, ccy: ccy_code });
    }
    counts.accounts = rows.len();
    Ok(rows)
}

// ---------------------------------------------------------------------------
// 分类与商户
// ---------------------------------------------------------------------------

/// 40 个生成分类：支出 10 顶级 + 20 二级，收入 4 顶级 + 6 二级；名字带「基准」前缀
/// 与迁移种子默认分类区分。
fn insert_categories(conn: &Connection, stamp: &str, counts: &mut GenCounts) -> Result<(), String> {
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
    let millis_stamp = stamp_date(stamp);

    let insert_one = |conn: &Connection,
                      name: &str,
                      kind: &str,
                      parent: Option<&str>,
                      seq: u64|
     -> Result<String, String> {
        let id = time_ordered_id("categories", seq, millis_stamp);
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

/// 从 stamp（ISO 时间戳）取日期部分转毫秒（仅用于确定性 ID 时间位）。
fn stamp_date(stamp: &str) -> i64 {
    stamp
        .split('T')
        .next()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .map(|d| date_millis(&d))
        .unwrap_or(0)
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
    let millis = stamp_date(stamp);
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

/// 生成的账户行：id 连同本位币（交易币种与转账同币约束都消费它）。
struct AccountRow {
    id: String,
    ccy: &'static str,
}

/// 退款链的支出源引用（同账户环形缓冲，供 refund 继承账户/币种/分类/商户）。
#[derive(Clone)]
struct ExpenseRef {
    id: String,
    date: NaiveDate,
    amount_cents: i64,
    category: Option<String>,
    merchant: Option<String>,
}

struct TxnCounts {
    total: usize,
    deleted: usize,
    transfers: usize,
    refunds: usize,
}

#[allow(clippy::too_many_arguments)]
fn insert_transactions(
    conn: &Connection,
    rng: &mut Rng,
    p: &GenerateParams,
    account_rows: &[AccountRow],
    expense_pool: &[String],
    income_pool: &[String],
    top_merchants: &[String],
    tail_merchants: &[String],
    start_date: NaiveDate,
    total_days: i64,
) -> Result<TxnCounts, String> {
    const SQL: &str = "INSERT INTO transactions (id,kind,amount_cents,currency_code,\
         amount_native_cents,account_id,to_account_id,category_id,merchant_id,\
         refund_of_transaction_id,note,dedup_hash,idempotency_key,date,created_at,updated_at,\
         version,device_id,is_deleted,policy_id)\
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,NULL,NULL,?12,?13,?13,1,?14,?15,NULL)";
    let mut stmt = conn.prepare(SQL).map_err(|e| e.to_string())?;

    // 生成账户的 id 与本位币视图（决定交易币种与转账同币约束）；
    // 外币折算率与 exchange_rates 落库值共用常量，恒一致。
    let account_ids: Vec<&str> = account_rows.iter().map(|a| a.id.as_str()).collect();
    let account_ccy = |idx: usize| account_rows[idx].ccy;
    let fx = |ccy: &str| match ccy {
        "USD" => USD_CNY,
        "EUR" => EUR_CNY,
        _ => 1.0,
    };

    // 同币种账户索引表：转账必须在同币种两账户间进行（跨币种转账非产品语义）。
    let mut same_ccy: Vec<(&str, Vec<usize>)> = Vec::new();
    for (idx, ccy_code) in account_rows.iter().enumerate() {
        match same_ccy.iter_mut().find(|(c, _)| *c == ccy_code.ccy) {
            Some((_, slot)) => slot.push(idx),
            None => same_ccy.push((ccy_code.ccy, vec![idx])),
        }
    }

    // 各账户的近期支出环形缓冲（refund 链来源）。
    let mut buffers: Vec<VecDeque<ExpenseRef>> = vec![VecDeque::new(); account_ids.len()];
    let mut counts = TxnCounts {
        total: 0,
        deleted: 0,
        transfers: 0,
        refunds: 0,
    };

    for seq in 0..p.transactions {
        let account_idx = rng.below(account_ids.len() as u64) as usize;
        let ccy = account_ccy(account_idx);
        let roll = rng.next_f64();
        let mut new_expense: Option<ExpenseRef> = None;
        let (kind, to_account, category, merchant, refund_of, note, amount, date) = if roll
            < EXPENSE_SHARE
        {
            let amount = expense_amount(rng);
            let category = Some(rng.pick(expense_pool).clone());
            let merchant =
                maybe_merchant(rng, top_merchants, tail_merchants, EXPENSE_MERCHANT_RATE);
            let note = maybe_note(rng, NOTE_RATE);
            let date = rand_date(rng, start_date, total_days, p.end_date);
            new_expense = Some(ExpenseRef {
                id: String::new(),
                date,
                amount_cents: amount,
                category: category.clone(),
                merchant: merchant.clone(),
            });
            (
                "expense", None, category, merchant, None, note, amount, date,
            )
        } else if roll < EXPENSE_SHARE + INCOME_SHARE {
            let amount = rng.range_i64(50_000, 2_000_000);
            let category = Some(rng.pick(income_pool).clone());
            let merchant = maybe_merchant(rng, top_merchants, tail_merchants, INCOME_MERCHANT_RATE);
            let note = maybe_note(rng, NOTE_RATE);
            let date = rand_date(rng, start_date, total_days, p.end_date);
            ("income", None, category, merchant, None, note, amount, date)
        } else if roll < EXPENSE_SHARE + INCOME_SHARE + TRANSFER_SHARE {
            // 同币种两账户间转账（跨币种转账非产品语义）；本币种无第二个账户时
            // 降级为支出（如唯一的外币户），避免转账两端同户。
            let candidates: Option<&Vec<usize>> = same_ccy
                .iter()
                .find(|(c, _)| *c == ccy)
                .map(|(_, idxs)| idxs)
                .filter(|idxs| idxs.len() >= 2);
            if let Some(idxs) = candidates {
                let mut to = rng.below(idxs.len() as u64) as usize;
                while idxs[to] == account_idx {
                    to = rng.below(idxs.len() as u64) as usize;
                }
                let amount = rng.range_i64(5_000, 2_000_000);
                let note = maybe_note(rng, TRANSFER_NOTE_RATE);
                let date = rand_date(rng, start_date, total_days, p.end_date);
                counts.transfers += 1;
                (
                    "transfer",
                    Some(account_ids[idxs[to]]),
                    None,
                    None,
                    None,
                    note,
                    amount,
                    date,
                )
            } else {
                let amount = expense_amount(rng);
                let category = Some(rng.pick(expense_pool).clone());
                let merchant =
                    maybe_merchant(rng, top_merchants, tail_merchants, EXPENSE_MERCHANT_RATE);
                let note = maybe_note(rng, NOTE_RATE);
                let date = rand_date(rng, start_date, total_days, p.end_date);
                new_expense = Some(ExpenseRef {
                    id: String::new(),
                    date,
                    amount_cents: amount,
                    category: category.clone(),
                    merchant: merchant.clone(),
                });
                (
                    "expense", None, category, merchant, None, note, amount, date,
                )
            }
        } else {
            // 退款链：从同账户近期支出取源；缓冲为空（极小规模）时退化为支出。
            let buffer_len = buffers[account_idx].len();
            if buffer_len == 0 {
                let amount = expense_amount(rng);
                let category = Some(rng.pick(expense_pool).clone());
                let merchant =
                    maybe_merchant(rng, top_merchants, tail_merchants, EXPENSE_MERCHANT_RATE);
                let note = maybe_note(rng, NOTE_RATE);
                let date = rand_date(rng, start_date, total_days, p.end_date);
                new_expense = Some(ExpenseRef {
                    id: String::new(),
                    date,
                    amount_cents: amount,
                    category: category.clone(),
                    merchant: merchant.clone(),
                });
                (
                    "expense", None, category, merchant, None, note, amount, date,
                )
            } else {
                let pick_idx = rng.below(buffer_len as u64) as usize;
                let src = &buffers[account_idx][pick_idx];
                let amount = if rng.chance(0.5) {
                    src.amount_cents
                } else {
                    (src.amount_cents / 2).max(1)
                };
                let date = (src.date + Duration::days(rng.below(31) as i64)).min(p.end_date);
                counts.refunds += 1;
                (
                    "refund",
                    None,
                    src.category.clone(),
                    src.merchant.clone(),
                    Some(src.id.clone()),
                    Some("退款".to_string()),
                    amount,
                    date,
                )
            }
        };

        // 行序号推导确定性 id；新支出带 id 入缓冲（成为后续退款的源）。
        let millis = date_millis(&date);
        let id = time_ordered_id("transactions", seq, millis);
        if let Some(exp) = new_expense.as_mut() {
            exp.id = id.clone();
            buffers[account_idx].push_back(exp.clone());
        }
        trim_buffer(&mut buffers[account_idx]);

        let created = format!(
            "{date}T{:02}:{:02}:{:02}Z",
            rng.range_i64(6, 23),
            rng.range_i64(0, 59),
            rng.range_i64(0, 59)
        );
        let native = (amount as f64 * fx(ccy)).round() as i64;
        let deleted = rng.chance(SOFT_DELETE_RATE);
        if deleted {
            counts.deleted += 1;
        }

        stmt.execute(rusqlite::params![
            id,
            kind,
            amount,
            ccy,
            native,
            account_ids[account_idx],
            to_account,
            category,
            merchant,
            refund_of,
            note,
            date.to_string(),
            created,
            DEVICE_ID,
            deleted as i64,
        ])
        .map_err(|e| e.to_string())?;
        counts.total += 1;
    }
    Ok(counts)
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

/// 窗口内均匀随机日期（钳到锚定结束日期内）。
fn rand_date(rng: &mut Rng, start: NaiveDate, total_days: i64, end: NaiveDate) -> NaiveDate {
    (start + Duration::days(rng.below(total_days as u64) as i64)).min(end)
}

/// 保留每账户最近 REFUND_BUFFER_CAP 条支出作为退款源。
fn trim_buffer(buf: &mut VecDeque<ExpenseRef>) {
    while buf.len() > REFUND_BUFFER_CAP {
        buf.pop_front();
    }
}

fn date_millis(d: &NaiveDate) -> i64 {
    d.and_hms_opt(12, 0, 0)
        .map(|dt| dt.and_utc().timestamp_millis())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// 汇率
// ---------------------------------------------------------------------------

/// 当前汇率两行（USD/EUR → CNY；折算基准与交易 native 列共用常量，恒一致）。
fn insert_exchange_rates(conn: &Connection, p: &GenerateParams) -> Result<usize, String> {
    let millis = date_millis(&p.end_date);
    let priced_at = format!("{}T16:00:00Z", p.end_date);
    for (seq, (base, rate)) in [(0u64, ("USD", USD_CNY)), (1, ("EUR", EUR_CNY))] {
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
    Ok(2)
}

/// fx_rate_history 全历史填充：窗口内每周一采样一条（trade_date 取当周周五、
/// 不足则钳到锚定结束日），USD/EUR 两币对，汇率围绕基准 ±2% 种子化漂移。
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
        for (base, base_rate) in [("USD", USD_CNY), ("EUR", EUR_CNY)] {
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
