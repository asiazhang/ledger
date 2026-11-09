//! NSH 测试器具（#2081 随 api_server 迁居）：API 集成测试（`tests/api_server`）
//! 与壳内单测共用的建库、固定时刻与源码扫描掩码器具——旧壳同款机制的本壳
//! 对应面（根包 test_support 统一工厂链接 tauri/GTK3，与 NSH 的 GTK4 同进程
//! 符号互踩，本 crate 不可消费；#2018 偏离记录先例，ADR-0145 双壳并存期两侧
//! 同构代价条已接受）。
//!
//! 门与可见性（ADR-0111 决策 5 / issue #1132，根包同款）：集成测试链接的是
//! 非 `#[cfg(test)]` 构建的 lib，故本模块以 `cfg(any(test, feature = "test-utils"))`
//! 声明（`lib.rs`），默认不进生产编译；`#[doc(hidden)]` 使其不进入文档。
//!
//! #2077 提示：本模块与根包 `test_support` 的形态统一归壳层机制 NSH 对账票；
//! 后合入者的 merger 需将两侧收拢单点。
//!
//! 测试豁免（ADR-0060）：本模块被集成测试以非 cfg(test) 构建链接，无法经
//! crate 根 cfg(test) 豁免覆盖，故文件级放行六件套；生产路径不得消费本模块。
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable
)]

/// 夹具簿记戳的统一发放值（ADR-0084 决策 5）：与根包固定时刻登记处
/// （`src-tauri/src/test_support/mod.rs` 的 `FIXED_NOW`，#1468）同值同语义
/// ——迁入测试的簿记戳断言随宿主迁居不改值。守门：check-test-support 规则 3
/// 的登记处唯一在根包（本声明是产品路径顶层项，不进其扫描区域）；#2077
/// 统一时退役本副本，禁令值不发生第二事实源。
pub const FIXED_NOW: &str = "2026-01-01T00:00:00Z";

/// 内存测试库（统一建库两行序的 infra 快速形态，spec #1086 / issue #1514）：
/// 接线与生产同形（[`crate::wiring::install_domain_seams`]，幂等先装者优先
/// ——余额刷新等写路径接缝未注册即码化错误，建库前必装），建库走 infra
/// 模板还原入口（旧壳 `test_support::open` 的同一 infra 入口）。与根包工厂
/// 的差异面：不接备份置脏 / 写后同步触发 / 期次钩子（NSH 骨架「宿主机制随
/// 宿主落地」裁决，[`crate::wiring`] 刻意不接清单）——迁入集成测试零置脏
/// 断言依赖（#2081 交付报告留痕）。
pub fn open() -> rusqlite::Connection {
    crate::wiring::install_domain_seams();
    ledger_infra::db::open_in_memory_initialized().expect("打开并初始化内存测试库")
}

/// 测试种子（#2081 随 api_server 迁居）：迁入集成测试消费的种子闭集——与根包
/// `test_support/seed.rs`（ADR-0084 工厂）逐行同构的副本。**为什么是副本**：
/// 根包工厂不可达（#2018 偏离记录），而种子 SQL 是行为断言的夹具形态，随宿主
/// 迁居不改行；簿记戳一律经 [`super::FIXED_NOW`] 内部发放（同根包纪律）。
/// #2077 统一时收拢单点（守门：check-test-support 规则 2 的工厂豁免已同步
/// 把本模块纳入工厂面）。
pub use seed::{
    seed_account, seed_exchange_rate, seed_fx_history_weeks, seed_fx_rate_history, seed_instrument,
    seed_investment_setup,
};

pub mod seed {
    use rusqlite::{Connection, params};

    use super::FIXED_NOW;

    /// 种入一个账户行（吸收 transaction/investment 两域 `insert_account` 同体函数，
    /// 签名按现存最全形状归一，ADR-0084 决策 4）。`kind` 为账户类型闭集字符串
    /// （cash/bank/credit/ewallet/investment/debt/receivable/other），初始余额以分计。
    pub fn seed_account(
        conn: &Connection,
        id: &str,
        name: &str,
        kind: &str,
        currency: &str,
        initial_balance_cents: i64,
    ) -> String {
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,?3,?4,?5,?6,?6,1,'test',0)",
            params![id, name, kind, currency, initial_balance_cents, FIXED_NOW],
        )
        .unwrap();
        id.to_string()
    }

    /// 种入一个股票标的行（吸收 db/investment 两域 `insert_instrument` 同体函数）。
    /// 类型固定 stock（基金/手动形态等单域变体留域薄皮）；市场为显式参数（db 域用
    /// `sh`、投资域用 `unknown`，两域吸收体并集）。
    pub fn seed_instrument(
        conn: &Connection,
        id: &str,
        symbol: &str,
        name: &str,
        currency: &str,
        market: &str,
    ) -> String {
        conn.execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,'stock',?3,?4,?5,?6,?6,1,'test')",
            params![id, symbol, name, currency, market, FIXED_NOW],
        )
        .unwrap();
        id.to_string()
    }

    /// 种入一行「当前汇率」（吸收 investment 域 `insert_rate_1_1` / `insert_rate` 与
    /// transaction、api_server 各处同形状 `exchange_rates` 插入）。行 id 由货币对派生
    /// （表约束 `UNIQUE(base_code, quote_code)`：每货币对仅一行最新汇率）；簿记戳取
    /// [`super::FIXED_NOW`]，`priced_at` 语义为「行情采集时间」，测试不读它。
    pub fn seed_exchange_rate(conn: &Connection, base: &str, quote: &str, rate: f64) -> String {
        seed_exchange_rate_row(conn, base, quote, rate, FIXED_NOW, None)
    }

    /// `exchange_rates` 插入同体（吸收各处同形状插入的单一形态；source 不传即 NULL）。
    fn seed_exchange_rate_row(
        conn: &Connection,
        base: &str,
        quote: &str,
        rate: f64,
        priced_at: &str,
        source: Option<&str>,
    ) -> String {
        let id = format!("er-{base}-{quote}");
        conn.execute(
            "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,1,'test')",
            params![id, base, quote, rate, priced_at, source, FIXED_NOW],
        )
        .unwrap();
        id
    }

    /// 种入一条汇率历史周采样点（吸收 db 域 `insert_fx_rate_history` 同体函数）。
    /// `trade_date` 是域时刻（行为输入），显式传入；来源固定 eastmoney。
    pub fn seed_fx_rate_history(
        conn: &Connection,
        id: &str,
        base: &str,
        quote: &str,
        trade_date: &str,
        rate: f64,
    ) -> String {
        conn.execute(
            "INSERT INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,?3,?4,?5,'eastmoney',?6,?6,1,'test')",
            params![id, base, quote, trade_date, rate, FIXED_NOW],
        )
        .unwrap();
        id.to_string()
    }

    /// 为给定日期各自所属周种汇率历史周点（#1547 写路径按交易日取数的测试夹具）：
    /// 同周去重、`INSERT OR IGNORE` 幂等，不覆盖已有点（读侧语义测试先种的特定
    /// 汇率不受影响）。
    pub fn seed_fx_history_weeks(
        conn: &Connection,
        base: &str,
        quote: &str,
        rate: f64,
        dates: &[&str],
    ) {
        let mut done: Vec<String> = Vec::new();
        for d in dates {
            let week: String = conn
                .query_row("SELECT date(?1,'-6 days','weekday 1')", params![d], |r| {
                    r.get(0)
                })
                .unwrap();
            if done.contains(&week) {
                continue;
            }
            done.push(week.clone());
            conn.execute(
                "INSERT OR IGNORE INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
                 VALUES (?1,?2,?3,?4,?6,'eastmoney',?5,?5,1,'test')",
                params![format!("fxh-{base}-{quote}-{week}"), base, quote, week, FIXED_NOW, rate],
            )
            .unwrap();
        }
    }

    /// 投资铺垫组合种子：账户 + 标的 + 1:1 汇率一行 + 1:1 周点序列（ADR-0084 决策 4，
    /// 吸收 transaction 域 `setup_investment_account` 与 api 集成
    /// `seed_investment_account` 的体）。账户为 USD 投资账户、标的为 USD 股票；
    /// 汇率 USD→CNY 1:1 当期行服务读路径，1:1 周点序列服务写路径（#1547）。
    pub fn seed_investment_setup(
        conn: &Connection,
        account_id: &str,
        instrument_id: &str,
    ) -> (String, String) {
        seed_account(conn, account_id, "美股", "investment", "USD", 0);
        seed_instrument(conn, instrument_id, "SYM", "Symbol", "USD", "unknown");
        seed_exchange_rate(conn, "USD", "CNY", 1.0);
        seed_fx_history_series_1to1(conn, "USD", "CNY");
        (account_id.to_string(), instrument_id.to_string())
    }

    /// 写路径按交易日折算所需的汇率历史周点序列（#1547）：1:1 周点覆盖测试交易
    /// 日期的宽窗口（2025-01-06 起每周一点，共 160 周）；与 1:1 当期行并存——
    /// 当期表服务读路径、历史序列服务写路径。`INSERT OR IGNORE`：同一连接内
    /// 多次调用 setup 不撞周唯一约束。
    fn seed_fx_history_series_1to1(conn: &Connection, base: &str, quote: &str) {
        // 窗口依据：本仓测试交易日期集中在 2026 年（工厂日期 2026-01~02、mwr 到
        // 2027-01），自 2025-01-06 起 160 周覆盖 2025-01 ~ 2028-02，留双倍余量。
        let mut week = "2025-01-06".to_string();
        let mut dates: Vec<String> = Vec::with_capacity(160);
        for _ in 0..160 {
            dates.push(week.clone());
            week = conn
                .query_row("SELECT date(?1,'+7 days')", params![week], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap();
        }
        let refs: Vec<&str> = dates.iter().map(String::as_str).collect();
        seed_fx_history_weeks(conn, base, quote, 1.0, &refs);
    }
}

pub mod scan {
    //! 源码扫描器具（#2081 随 api_server 迁居）：词法掩码与花括号配对原语
    //! ——行情接缝守门（`api_server::state` 测试）与 HTTP 写路径守门
    //! （`api_server::guard`）的共用面，旧壳 `test_support::scan`（issue
    //! #1433 单一维护点）的本壳副本。**双源登记**：与根包 Rust 载体、TS 侧
    //! `gate-primitives.ts::maskNonCode` 同规（三载体并存是双壳并存期的
    //! 同构代价，ADR-0145）；共享语料夹具防漂移——任一载体单独改规则即
    //! 语料测试红。#2077 统一时收拢。

    /// 掩码 Rust 源文本中的注释与字符串/char 字面量（实现与根包
    /// `test_support/scan.rs::mask_non_code` 逐字节同构，规则改动须三载体
    /// 同步 + 语料更新）。
    pub fn mask_non_code(text: &str) -> String {
        // 若 `chars` 中 `i` 起是 Rust 原始字符串前缀，返回其后开引号下标；否则返回
        // `None`。覆盖 `r"…"` / `r#"…"` 与字节变体 `br"…"` / `br#"…"`，# 数任意；
        // 前一字符为标识符成分时是普通名字，不误伤。
        fn raw_string_open_quote(chars: &[char], i: usize) -> Option<usize> {
            let n = chars.len();
            if i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
                return None;
            }
            let mut j = i;
            if j + 1 < n && chars[j] == 'b' && chars[j + 1] == 'r' {
                j += 2;
            } else if j < n && chars[j] == 'r' {
                j += 1;
            } else {
                return None;
            }
            while j < n && chars[j] == '#' {
                j += 1;
            }
            if j < n && chars[j] == '"' {
                Some(j)
            } else {
                None
            }
        }

        let bytes: Vec<char> = text.chars().collect();
        let n = bytes.len();
        let mut out = bytes.clone();
        let blank = |out: &mut Vec<char>, from: usize, to: usize| {
            for k in out.iter_mut().take(to.min(n)).skip(from) {
                if *k != '\n' {
                    *k = ' ';
                }
            }
        };
        let mut i = 0;
        while i < n {
            let c = bytes[i];
            if c == '/' && i + 1 < n && bytes[i + 1] == '/' {
                // 行注释（含 /// 与 //!）到行尾
                let end = bytes[i..]
                    .iter()
                    .position(|&b| b == '\n')
                    .map_or(n, |p| i + p);
                blank(&mut out, i, end);
                i = end;
            } else if c == '/' && i + 1 < n && bytes[i + 1] == '*' {
                // 块注释（Rust 可嵌套）
                let mut depth = 1usize;
                let mut j = i + 2;
                while j < n && depth > 0 {
                    if j + 1 < n && bytes[j] == '/' && bytes[j + 1] == '*' {
                        depth += 1;
                        j += 2;
                    } else if j + 1 < n && bytes[j] == '*' && bytes[j + 1] == '/' {
                        depth -= 1;
                        j += 2;
                    } else {
                        j += 1;
                    }
                }
                blank(&mut out, i, j);
                i = j;
            } else if c == '"' {
                // 普通字符串：跳过转义对
                let mut j = i + 1;
                while j < n {
                    if bytes[j] == '\\' {
                        j += 2;
                    } else if bytes[j] == '"' {
                        j += 1;
                        break;
                    } else {
                        j += 1;
                    }
                }
                blank(&mut out, i, j);
                i = j;
            } else if c == 'r' || c == 'b' {
                // 原始字符串 r"…" / r#"…" / r##"…" 与字节变体
                let Some(open) = raw_string_open_quote(&bytes, i) else {
                    i += 1;
                    continue;
                };
                let prefix_end = if c == 'b' { i + 2 } else { i + 1 };
                let hashes = open - prefix_end;
                let close: Vec<char> = format!("\"{}", "#".repeat(hashes)).chars().collect();
                let mut end = n;
                let mut k = open + 1;
                while k + close.len() <= n {
                    if bytes[k..k + close.len()] == close[..] {
                        end = k + close.len();
                        break;
                    }
                    k += 1;
                }
                blank(&mut out, i, end);
                i = end;
            } else if c == '\'' {
                // char 字面量 vs 生命周期：有闭引号为字面量，否则是生命周期标注
                let mut j = i + 1;
                if j < n && bytes[j] == '\\' {
                    j += 1;
                    if j < n && bytes[j] == '{' {
                        while j < n && bytes[j] != '}' {
                            j += 1;
                        }
                    }
                    j += 1;
                } else {
                    j += 1;
                }
                if j < n && bytes[j] == '\'' {
                    blank(&mut out, i, j + 1);
                    i = j + 1;
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
        out.into_iter().collect()
    }

    /// 掩码文本上从 `open`（必须指向 `{`）起做花括号配对，返回配对闭括号之后的
    /// 字节偏移（实现与根包 `test_support/scan.rs::matching_brace_end` 同构）。
    pub fn matching_brace_end(masked: &str, open: usize) -> Option<usize> {
        if !masked[open..].starts_with('{') {
            return None;
        }
        let mut depth = 0usize;
        for (idx, ch) in masked[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(open + idx + 1);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// 双源防漂移语料（#1433）：本实现与根包 Rust 载体、TS 侧 `maskNonCode`
    /// 消费同一夹具，期望输出以 TS 侧权威实现生成；任一载体单独改规则即
    /// 本测（与根包 scan 语料测试、vitest 侧语料测试三面）红。
    #[test]
    fn mask_non_code_matches_shared_corpus() {
        let corpus = include_str!("../../../../scripts/fixtures/rust-mask-corpus.rs");
        let expected = include_str!("../../../../scripts/fixtures/rust-mask-corpus.expected.txt");
        assert_eq!(
            mask_non_code(corpus),
            expected,
            "掩码输出与共享语料期望漂移——规则改动必须与根包 scan 载体及 TS 侧 \
             maskNonCode（scripts/gate-primitives.ts）同步并重新生成语料期望"
        );
    }

    #[test]
    fn matching_brace_end_matches_nested_and_returns_past_close() {
        let masked = "fn f() { g({ a }); h(); } fn g() {}";
        let open = masked.find('{').unwrap();
        assert_eq!(
            matching_brace_end(masked, open),
            Some(masked.find("} fn").unwrap() + 1)
        );
    }

    #[test]
    fn matching_brace_end_rejects_non_open_and_unbalanced() {
        // open 不指向 {
        assert_eq!(matching_brace_end("fn f() …", 0), None);
        // 不配对（掩码文本不会发生，防御路径）
        let unbalanced = "fn f() { g();";
        let open = unbalanced.find('{').unwrap();
        assert_eq!(matching_brace_end(unbalanced, open), None);
    }
}
