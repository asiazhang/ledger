//! 多端同步域测试薄皮（仅限本测试目录使用）：双端建库、交易语义输入构造器、
//! 业务字段行读取与内存假 Transport（#859 真通道前的合成通道）。通用夹具
//! （建库两行序、账户种子）消费统一测试工厂 `crate::test_support`（ADR-0084）；
//! 本文件只留本域特有的输入构造与判据读取。

use rusqlite::Connection;

use crate::sync_engine::{ingest_ops, read_ops};
use crate::test_support::FIXED_NOW;
use crate::transaction::TransactionInput;
use crate::transaction::amount::TransactionKind;

/// 内存假 Transport 的线路形态：读端全部 op 的 wire 序列（JSON 字符串，#859
/// 真通道接线前的合成通道；schema 偏斜场景由此可合成）。
pub(crate) fn wire_out(conn: &Connection) -> Vec<String> {
    read_ops(conn)
        .unwrap()
        .iter()
        .map(|op| serde_json::to_string(op).unwrap())
        .collect()
}

/// 内存假 Transport 投递：对端逐条接入（解析失败按 schema 偏斜挂起，不中断）。
pub(crate) fn wire_in(conn: &Connection, wire: &[String]) -> Vec<crate::sync_engine::ApplyReport> {
    ingest_ops(conn, wire).unwrap()
}

/// 支出输入构造器（闭环测试的「A 端写」侧语义输入）。
pub(crate) fn make_expense(account_id: &str, amount_cents: i64, note: &str) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Expense,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: Some(note.into()),
        date: "2026-01-10".into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    }
}

/// 一笔交易的业务字段快照（判据读取）：账本状态一致 = 业务字段相等。
///
/// 审计列（created_at / updated_at / device_id / version）是各端本地事实，
/// 不参与状态等值判定（ADR-0091：op 只携带 DeviceId / 逻辑时钟 / schema 版本，
/// 不携带墙钟；LWW 裁决依据是 op 全序，#856 承接）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TxnRow {
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub amount_native_cents: i64,
    pub account_id: String,
    pub to_account_id: Option<String>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub date: String,
    pub is_deleted: i64,
}

/// 按 id 读取交易业务字段（不存在返回 None）。
pub(crate) fn read_transaction(conn: &Connection, id: &str) -> Option<TxnRow> {
    conn.query_row(
        "SELECT kind, amount_cents, currency_code, amount_native_cents, account_id, \
         to_account_id, category_id, merchant_id, refund_of_transaction_id, note, date, is_deleted \
         FROM transactions WHERE id = ?1",
        [id],
        |r| {
            Ok(TxnRow {
                kind: r.get(0)?,
                amount_cents: r.get(1)?,
                currency_code: r.get(2)?,
                amount_native_cents: r.get(3)?,
                account_id: r.get(4)?,
                to_account_id: r.get(5)?,
                category_id: r.get(6)?,
                merchant_id: r.get(7)?,
                refund_of_transaction_id: r.get(8)?,
                note: r.get(9)?,
                date: r.get(10)?,
                is_deleted: r.get(11)?,
            })
        },
    )
    .ok()
}

// ---------------------------------------------------------------------------
// 定时计划合成夹具（issue #856 防双扣场景）：计划与期次行按同一计划 id 在两端
// 等量种子（真实世界对应 #860 计划同步后的两端状态）；期次行 id 刻意允许两端
// 不同——期次身份是 (plan_id, 计划日期)，不是本地行 id。
// ---------------------------------------------------------------------------

/// 种入一个 active 订阅计划（期次触发场景的最小计划行，含订阅扩展行）。
pub(crate) fn seed_plan(conn: &Connection, plan_id: &str, account_id: &str, amount_cents: i64) {
    conn.execute(
        "INSERT INTO scheduled_transactions \
         (id,kind,status,account_id,category_id,amount_cents,currency_code,\
         recurrence_type,recurrence_interval,recurrence_day,start_date,note,\
         created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'subscription','active',?2,NULL,?3,'CNY','monthly',1,NULL,'2026-01-01',NULL,?4,?4,1,'test',0)",
        rusqlite::params![plan_id, account_id, amount_cents, FIXED_NOW],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO subscription_plans (scheduled_transaction_id,merchant_id,policy_id) \
         VALUES (?1,NULL,NULL)",
        [plan_id],
    )
    .unwrap();
}

/// 种入一个 pending 期次（期次行 id 本地自定，期次身份由 plan + 日期承载）。
pub(crate) fn seed_occurrence(
    conn: &Connection,
    occ_id: &str,
    plan_id: &str,
    scheduled_date: &str,
    amount_cents: i64,
) {
    conn.execute(
        "INSERT INTO scheduled_transaction_occurrences \
         (id,scheduled_transaction_id,scheduled_date,status,transaction_id,amount_cents,\
         created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,'pending',NULL,?4,?5,?5,1,'test',0)",
        rusqlite::params![occ_id, plan_id, scheduled_date, amount_cents, FIXED_NOW],
    )
    .unwrap();
}

/// 种入本机设备标识（固定 id：需要确定 DeviceId 序的全序/LWW 场景）。
pub(crate) fn seed_device(conn: &Connection, device_id: &str) {
    conn.execute(
        "INSERT INTO sync_device (id, logical_clock, created_at, updated_at) VALUES (?1, 0, ?2, ?2)",
        rusqlite::params![device_id, FIXED_NOW],
    )
    .unwrap();
}

/// 期次行状态快照（判据读取）：(status, transaction_id)。
pub(crate) fn read_occurrence(conn: &Connection, occ_id: &str) -> Option<(String, Option<String>)> {
    conn.query_row(
        "SELECT status, transaction_id FROM scheduled_transaction_occurrences WHERE id = ?1",
        [occ_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .ok()
}

// ---------------------------------------------------------------------------
// #859 通道测试替身：内存假 Transport 与本地 WebDAV 桩
// ---------------------------------------------------------------------------

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::sync_engine::transport::Transport;

/// 内存假 Transport：进程内 `BTreeMap` 字节通道（通道布局/manifest/轮次逻辑
/// 的快速测试替身；HTTP 语义归 WebDAV 桩用例）。
#[derive(Default)]
pub(crate) struct MemoryTransport {
    files: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl MemoryTransport {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 删除文件（测试通道故障注入：段缺失等场景）。
    pub(crate) fn files_remove(&self, path: &str) {
        self.files.lock().unwrap().remove(path);
    }
}

impl Transport for MemoryTransport {
    fn ensure_dir(&self, _path: &str) -> crate::error::Result<()> {
        Ok(())
    }

    fn read_file(&self, path: &str) -> crate::error::Result<Option<Vec<u8>>> {
        Ok(self.files.lock().unwrap().get(path).cloned())
    }

    fn write_file(&self, path: &str, bytes: &[u8]) -> crate::error::Result<()> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_string(), bytes.to_vec());
        Ok(())
    }
}

/// 本地 WebDAV 桩：axum 实现的最小 WebDAV 服务（MKCOL/GET/PUT + 可选 Basic
/// 认证），临时目录承载文件，供 WebDAV 后端与两端文件交换集成测试使用。
pub(crate) struct WebDavStub {
    /// 同步根 URL（`http://127.0.0.1:<port>`）。
    pub(crate) base_url: String,
    root: std::path::PathBuf,
}

impl Drop for WebDavStub {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

/// 起一个本地 WebDAV 桩；`auth` 为 `Some((user, pass))` 时要求 Basic 认证
/// （凭据错误 → 401）。返回桩句柄与独立临时根目录。
pub(crate) fn spawn_webdav_stub(auth: Option<(&str, &str)>) -> WebDavStub {
    use axum::extract::{Request, State};
    use axum::http::{HeaderName, StatusCode};
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as B64;

    struct StubState {
        root: std::path::PathBuf,
        /// 期望的 Authorization 头完整值（`Basic <base64>`）；None = 不鉴权。
        expected_auth: Option<String>,
    }

    /// MKCOL 扩展方法（http crate 无常量，按字面量解析）。
    fn mkcol() -> axum::http::Method {
        axum::http::Method::from_bytes(b"MKCOL").unwrap()
    }

    /// 统一入口：按 HTTP 方法分发 WebDAV 最小语义（响应主体为原始字节）。
    async fn dav(
        State(state): State<std::sync::Arc<StubState>>,
        request: Request,
    ) -> (StatusCode, Vec<u8>) {
        if let Some(expected) = &state.expected_auth {
            let got = request
                .headers()
                .get(HeaderName::from_static("authorization"))
                .and_then(|v| v.to_str().ok());
            if got != Some(expected.as_str()) {
                return (StatusCode::UNAUTHORIZED, Vec::new());
            }
        }
        let path = request.uri().path().trim_start_matches('/').to_string();
        if path
            .split('/')
            .any(|seg| seg.is_empty() || seg == "." || seg == "..")
        {
            return (StatusCode::BAD_REQUEST, Vec::new());
        }
        let target = state.root.join(&path);
        let method = request.method().clone();
        if method == mkcol() {
            if target.is_dir() {
                return (StatusCode::METHOD_NOT_ALLOWED, Vec::new());
            }
            return match std::fs::create_dir_all(&target) {
                Ok(()) => (StatusCode::CREATED, Vec::new()),
                Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Vec::new()),
            };
        }
        if method == axum::http::Method::PUT {
            let body = axum::body::to_bytes(request.into_body(), usize::MAX)
                .await
                .unwrap_or_default();
            let parent = target.parent().unwrap_or(&state.root).to_path_buf();
            return match (
                std::fs::create_dir_all(&parent),
                std::fs::write(&target, &body),
            ) {
                (Ok(()), Ok(())) => (StatusCode::CREATED, Vec::new()),
                _ => (StatusCode::INTERNAL_SERVER_ERROR, Vec::new()),
            };
        }
        if method == axum::http::Method::GET {
            return match std::fs::read(&target) {
                Ok(bytes) => (StatusCode::OK, bytes),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    (StatusCode::NOT_FOUND, Vec::new())
                }
                Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Vec::new()),
            };
        }
        (StatusCode::METHOD_NOT_ALLOWED, Vec::new())
    }

    let root = std::env::temp_dir().join(format!("ledger-webdav-stub-{}", crate::db::new_uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let expected_auth =
        auth.map(|(user, pass)| format!("Basic {}", B64.encode(format!("{user}:{pass}"))));
    let app = axum::Router::new()
        .route(
            "/{*path}",
            axum::routing::any(dav).with_state(std::sync::Arc::new(StubState {
                root: root.clone(),
                expected_auth,
            })),
        )
        .route(
            "/",
            axum::routing::any(|| async { StatusCode::METHOD_NOT_ALLOWED }),
        );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            // tokio 新版要求 fd 先行置为非阻塞再注册（issue #7172）。
            listener.set_nonblocking(true).unwrap();
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(listener, app).await.unwrap();
        });
    });
    WebDavStub {
        base_url: format!("http://{addr}"),
        root,
    }
}
