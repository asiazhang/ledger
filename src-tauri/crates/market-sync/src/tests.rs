//! 行情同步测试（issue #89 外迁）：HTTP 重试/多主机/解析、价格换算与增量同步行为。
//! HTTP 层通过本地 HTTP 服务独立测试，不依赖真实网络。
//!
//! #256 按行为主题拆为子模块（纯移动）：
//! - `bulk_fetch`：行情批量取数面报文解析、请求形态与跨同步记忆（ADR-0121，issue #1374）；
//! - `http_client`：HTTP 重试与多主机切换；
//! - `instrument_info_sync`：标的信息同步与 ulist / 日 K 报文解析；
//! - `fund_search`：东财基金搜索报文解析与命中挑选（issue #301，fixture 驱动）；
//! - `fund_nav`：历史净值报文解析、水位窗口与 Referer 传播（issue #303，fixture 驱动）；
//! - `stock_quote`：股票单点行情报文解析、类型特征探测与命中挑选（issue #693，fixture 驱动）。
//!
//! 全量同步（clist 报文解析、分页编排、取消与重入守卫）已随 ADR-0081 决策 3
//! 退役删除（issue #698）。

use std::sync::{Arc, Mutex};

use ledger_infra::error::Result;
use rusqlite::{Connection, params};
use tauri_app_lib::test_support::{seed_account, seed_instrument};

mod bulk_fetch;
mod fund_nav;
mod fund_search;
mod history_backfill;
mod http_client;
mod instrument_info_sync;
mod stock_quote;

// ---------------------------------------------------------------------------
// 共享测试脚手架（一份）
// ---------------------------------------------------------------------------

/// 桩适配器（issue #1412）：同步应答值 → 通道 future（抓取闭包 async 形态后的
/// 最小包装——既有桩闭包的应答逻辑保持同步表达式，只在出口装箱为立即就绪的
/// future，调用点零改动）。
pub(super) fn ready<T: Send + 'static>(value: Result<T>) -> crate::channels::FetchFuture<T> {
    Box::pin(std::future::ready(value))
}

/// 直插一条持仓（账户 + 标的 + 交易 + 批次），绕过交易行为层以聚焦增量同步自身逻辑。
/// 账户/标的经工厂种子（spec #728 / ADR-0084 决策 4）；标的类型工厂固定 stock，
/// bond/other/fund 等域变体经类型修正表达——类型是本域 secid 构造/跳过规则的
/// 行为输入，不入工厂种子。
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

/// 直插一笔买入交易 + 持仓批次（绕过交易行为层，聚焦同步自身逻辑）。
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

/// 起一个捕获请求头的本地 HTTP 服务（响应体固定、按顺序收集请求头），返回
/// (基础地址, 请求头收集器)——文本 / JSON 两类通道的请求形态断言共用一份脚手架。
pub(super) fn spawn_header_capture_server(body: String) -> (String, Arc<Mutex<Vec<String>>>) {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let heads = Arc::new(Mutex::new(Vec::new()));
    let heads_clone = heads.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            heads_clone
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf).to_string());
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    (url, heads)
}
