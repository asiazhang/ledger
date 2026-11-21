use std::sync::{Arc, Mutex};

use crate::http::Pacer;
use ledger_infra::error::Result;
use rusqlite::{Connection, params};
use tauri_app_lib::test_support::scan::{mask_non_code, matching_brace_end};
use tauri_app_lib::test_support::{seed_account, seed_instrument};

mod bulk_fetch;
mod csrc;
mod fund_nav;
mod fund_quote;
mod fx_persist;
mod fx_sync;
mod history_backfill;
mod http_client;
mod instrument_info_sync;
mod sina_fund;
mod source_tail;
mod stock_quote;
mod tencent;
mod weekly;

pub(super) fn ready<T: Send + 'static>(value: Result<T>) -> crate::channels::FetchFuture<T> {
    Box::pin(std::future::ready(value))
}

pub(super) fn insert_holding(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
    symbol: &str,
    kind: &str,
    currency: &str,
    market: &str,
) {
    seed_account(
        conn,
        account_id,
        &format!("账户-{account_id}"),
        "investment",
        currency,
        0,
    );
    seed_instrument(
        conn,
        instrument_id,
        symbol,
        &format!("名称-{symbol}"),
        currency,
        market,
    );
    if kind != "stock" {
        conn.execute(
            "UPDATE instruments SET instrument_type=?1 WHERE id=?2",
            params![kind, instrument_id],
        )
        .unwrap();
    }
    insert_lot(conn, account_id, instrument_id, currency);
}

pub(super) fn insert_lot(conn: &Connection, account_id: &str, instrument_id: &str, currency: &str) {
    let txn_id = format!("txn-{account_id}-{instrument_id}");
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id) \
         VALUES (?1,'buy',1000,?2,1000,?3,'2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params![txn_id, currency, account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',10,100,0)",
        params![txn_id, instrument_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,10,10,100,?5,'2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params![
            format!("lot-{account_id}-{instrument_id}"),
            account_id,
            instrument_id,
            txn_id,
            currency
        ],
    )
    .unwrap();
}

pub(super) fn spawn_capture_server(
    responder: impl Fn(usize) -> (u16, Vec<u8>) + Send + 'static,
) -> (String, Arc<Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let heads = Arc::new(Mutex::new(Vec::new()));
    let heads_clone = heads.clone();
    std::thread::spawn(move || {
        let mut seq = 0usize;
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let Ok(reader_stream) = stream.try_clone() else {
                break;
            };
            let mut reader = BufReader::new(reader_stream);
            let mut head = String::new();
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let ends_head = line == "\r\n" || line == "\n";
                        head.push_str(&line);
                        if ends_head {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            heads_clone.lock().unwrap().push(head);
            seq += 1;
            let (status, body) = responder(seq);
            let reason = if status == 200 { "OK" } else { "Error" };
            let mut stream = stream;
            let resp_head = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(resp_head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    (url, heads)
}

pub(super) fn spawn_header_capture_server(
    body: impl Into<Vec<u8>>,
) -> (String, Arc<Mutex<Vec<String>>>) {
    let body = body.into();
    spawn_capture_server(move |_| (200, body.clone()))
}

pub(super) fn zero_pacer() -> Arc<tokio::sync::Mutex<Pacer>> {
    Arc::new(tokio::sync::Mutex::new(Pacer::new(
        std::time::Duration::ZERO,
    )))
}

fn blank_inline_test_modules(source: &str) -> String {
    let mut out = source.to_string();
    let mut i = 0usize;
    while let Some(rel) = out[i..].find("#[cfg(test)]") {
        let anchor = i + rel;
        let head = anchor + "#[cfg(test)]".len();
        let after = &out[head..];
        let trimmed = after.trim_start();
        if !trimmed.starts_with("mod") {
            break;
        }
        let Some(open) = after.find('{').map(|p| head + p) else {
            break;
        };
        let Some(end) = matching_brace_end(&out, open) else {
            break;
        };
        let blanked: String = out[anchor..end]
            .chars()
            .map(|c| if c == '\n' { '\n' } else { ' ' })
            .collect();
        out.replace_range(anchor..end, &blanked);
        i = anchor + blanked.len();
    }
    out
}

fn production_source_files() -> Vec<(&'static str, String)> {
    let mut files: Vec<(&'static str, String)> = vec![
        ("bulk.rs", include_str!("bulk.rs").to_string()),
        ("channels.rs", include_str!("channels.rs").to_string()),
        ("csrc.rs", include_str!("csrc.rs").to_string()),
        (
            "daily_refresh.rs",
            include_str!("daily_refresh.rs").to_string(),
        ),
        ("ecb.rs", include_str!("ecb.rs").to_string()),
        ("fund.rs", include_str!("fund.rs").to_string()),
        (
            "fund_backfill.rs",
            include_str!("fund_backfill.rs").to_string(),
        ),
        ("fund_nav.rs", include_str!("fund_nav.rs").to_string()),
        (
            "fund_price_refresh.rs",
            include_str!("fund_price_refresh.rs").to_string(),
        ),
        ("fx.rs", include_str!("fx.rs").to_string()),
        ("fx_daily.rs", include_str!("fx_daily.rs").to_string()),
        ("history.rs", include_str!("history.rs").to_string()),
        ("http.rs", include_str!("http.rs").to_string()),
        ("incremental.rs", include_str!("incremental.rs").to_string()),
        ("lane.rs", include_str!("lane.rs").to_string()),
        ("lib.rs", include_str!("lib.rs").to_string()),
        ("model.rs", include_str!("model.rs").to_string()),
        ("persist.rs", include_str!("persist.rs").to_string()),
        ("progress.rs", include_str!("progress.rs").to_string()),
        ("session.rs", include_str!("session.rs").to_string()),
        ("sina_fund.rs", include_str!("sina_fund.rs").to_string()),
        ("source_tail.rs", include_str!("source_tail.rs").to_string()),
        ("stock.rs", include_str!("stock.rs").to_string()),
        ("tencent.rs", include_str!("tencent.rs").to_string()),
        (
            "tencent_kline.rs",
            include_str!("tencent_kline.rs").to_string(),
        ),
        ("weekly.rs", include_str!("weekly.rs").to_string()),
    ];
    files.sort_by_key(|(name, _)| *name);
    files
}

#[test]
fn guard_source_list_matches_directory_exactly() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let src_dir = std::path::Path::new(manifest).join("src");
    let mut on_disk: Vec<String> = std::fs::read_dir(&src_dir)
        .expect("src 目录应可枚举")
        .map(|entry| entry.expect("目录项应可读").path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "rs"))
        .map(|path| {
            path.file_name()
                .expect("文件名应存在")
                .to_string_lossy()
                .to_string()
        })
        .filter(|name| name != "tests.rs")
        .collect();
    on_disk.sort();
    let mut in_list: Vec<&str> = production_source_files()
        .iter()
        .map(|(name, _)| *name)
        .collect();
    in_list.sort();
    assert_eq!(
        in_list, on_disk,
        "守门源文件清单与 src/ 目录漂移——新增/删除生产文件须同步 production_source_files"
    );
}

#[test]
fn background_lanes_are_global_runtime_async_tasks() {
    let sources: Vec<(&'static str, String)> = production_source_files()
        .into_iter()
        .map(|(name, src)| (name, blank_inline_test_modules(&mask_non_code(&src))))
        .collect();

    let thread_hits: Vec<&str> = sources
        .iter()
        .filter(|(_, src)| src.contains("thread::spawn") || src.contains("std::thread::sleep"))
        .map(|(name, _)| *name)
        .collect();
    assert!(
        thread_hits.is_empty(),
        "行情同步域生产面出现自建线程/阻塞定时 {thread_hits:?}——后台车道必须是挂全局运行时的 \
         async 任务（tauri::async_runtime::spawn + tokio::time::sleep，ADR-0125 决策 7）"
    );

    for (name, src) in &sources {
        match *name {
            "lane.rs" => {
                assert!(
                    src.contains("tauri::async_runtime::spawn"),
                    "lane.rs 调度循环应以 tauri::async_runtime::spawn 拉起 async 任务 \
                     （ADR-0125 决策 7 / issue #1413）"
                );
                assert!(
                    src.contains("tokio::time::sleep"),
                    "lane.rs 的启动延迟与自然日窗口应用 tokio::time::sleep 异步定时 \
                     （ADR-0125 决策 7 / issue #1413）"
                );
            }
            "daily_refresh.rs" | "history.rs" | "fx_daily.rs" => {
                assert!(
                    src.contains("start_daily_lane("),
                    "{name} 应调起共享调度循环 start_daily_lane（issue #1622）——\
                     自建平行调度循环即 Shotgun Surgery 回归"
                );
            }
            _ => {}
        }
    }
}

#[test]
fn production_face_has_no_blocking_bridge() {
    let sources: Vec<(&'static str, String)> = production_source_files()
        .into_iter()
        .map(|(name, src)| (name, blank_inline_test_modules(&mask_non_code(&src))))
        .collect();

    let bridge_hits: Vec<&str> = sources
        .iter()
        .filter(|(_, src)| src.contains("block_on"))
        .map(|(name, _)| *name)
        .collect();
    assert!(
        bridge_hits.is_empty(),
        "行情同步域生产面出现阻塞驱动点 block_on {bridge_hits:?}——过渡同步桥已随 \
         issue #1413 拆除，网络等待一律以 await 表达（ADR-0125 决策 5/7）"
    );

    for (name, src) in &sources {
        let is_entry = matches!((*name, ()), ("fund.rs", ()) | ("stock.rs", ()));
        if !is_entry {
            continue;
        }
        let needle = if *name == "fund.rs" {
            "pub async fn fetch_fund_quote_production"
        } else {
            "pub async fn fetch_stock_quote_production"
        };
        assert!(
            src.contains(needle),
            "{name} 的生产拉取入口必须保持 async fn（{needle}）——壳层接缝在异步上下文 \
             直接 await，改回同步形状即重引阻塞驱动（ADR-0125 决策 5/7 / issue #1413）"
        );
    }
}

#[test]
fn production_entries_join_foreground_lane_discipline() {
    let sources: Vec<(&'static str, String)> = production_source_files()
        .into_iter()
        .map(|(name, src)| (name, blank_inline_test_modules(&mask_non_code(&src))))
        .collect();
    for (name, signature) in [
        ("fund.rs", "pub async fn fetch_fund_quote_production("),
        ("stock.rs", "pub async fn fetch_stock_quote_production("),
    ] {
        let src = sources
            .iter()
            .find(|(file, _)| *file == name)
            .map(|(_, src)| src.as_str())
            .unwrap_or_else(|| panic!("{name} 应在守门源文件清单内"));
        let anchor = src
            .find(signature)
            .unwrap_or_else(|| panic!("{name} 的生产拉取入口应在位"));
        let open = anchor + src[anchor..].find('{').expect("生产拉取入口应有函数体");
        let end = matching_brace_end(src, open).expect("生产拉取入口体花括号应配对");
        let body = &src[open..end];
        for (needle, what) in [
            ("ForegroundGuard::enter()", "前台在途守卫"),
            ("shared_pacer()", "进程级共享限速器"),
        ] {
            assert!(
                body.contains(needle),
                "{name} 的生产拉取入口函数体应接线 {what}（{needle}）——手动查询须被\
                 后台车道看见并入全局额度（issue #1674），删除该接线即红"
            );
        }
    }
}

#[test]
fn background_lanes_share_single_round_skeleton() {
    let sources: Vec<(&'static str, String)> = production_source_files()
        .into_iter()
        .map(|(name, src)| (name, mask_non_code(&src)))
        .collect();

    const SKELETON_MECHANICS: [&str; 7] = [
        "production_backfill()",
        "FacadeWriteSession::new",
        "any_written()",
        "write.run(",
        "emit_for(",
        "emit_backfill_progress",
        "emit_progress",
    ];
    for name in ["daily_refresh.rs", "history.rs"] {
        let src = sources
            .iter()
            .find(|(file, _)| *file == name)
            .map(|(_, src)| src.as_str())
            .unwrap_or_else(|| panic!("{name} 应在守门源文件清单内"));
        for needle in SKELETON_MECHANICS {
            assert!(
                !src.contains(needle),
                "{name} 出现骨架机制 {needle}——换装 / 会话 / 见证 / 裁决 / 发射归 \
                 lane.rs 单点，回流到车道模块即平行骨架复活（issue #1426）"
            );
        }
    }
}
