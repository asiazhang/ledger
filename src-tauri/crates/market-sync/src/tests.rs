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

mod bulk_fetch;
mod fund_nav;
mod fund_search;
mod http_client;
mod instrument_info_sync;
mod stock_quote;

// ---------------------------------------------------------------------------
// 共享测试脚手架（一份）
// ---------------------------------------------------------------------------

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
