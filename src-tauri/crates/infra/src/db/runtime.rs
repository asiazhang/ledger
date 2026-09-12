//! 连接层运行时接缝（自 `db/mod.rs` 按职责拆出，issue #1127，纯移动）：
//! 统一写入口与提交点后置钩子注册（ADR-0032 / spec #1086）、统一 DB 调用
//! helper（ADR-0069 形状乙，spec #498）与应用状态 [`DbState`]。

use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::Connection;

use super::connection::open_in_memory;
use super::migrate::init_db;
use crate::error::{AppError, Result};

// ---------------------------------------------------------------------------
// 连接层统一写入口（ADR-0032）
// ---------------------------------------------------------------------------

/// 连接层统一写入口：锁连接执行写闭包（ADR-0032，spec #173）。
///
/// 置脏触发的单点，业务写路径对备份域零感知：
/// - 锁连接 → 执行闭包；
/// - 闭包成功且事务已提交（`is_autocommit()`，含闭包内部自行 `BEGIN`/`COMMIT`
///   后回到提交点）→ 单点执行置脏 + 写时顺带到期检查；
/// - 闭包失败（事务回滚）不置脏；未提交就返回（显式事务仍打开）同样不置脏——
///   「事务内推迟、提交后补上」由本结构保证，不再依赖调用方注释口口相传。
///
/// 豁免清单集中在「不经过本入口的写方」：设置与调度状态写入（`app_settings`
/// 全表，经 [`crate::settings`] 单点收口）与恢复（Restore）路径。
///
/// 薄 wrapper 边界：只做「锁 + 写后置动作/检查」，不接管事务管理——闭包内保留
/// 裸 `BEGIN`/`COMMIT`/`ROLLBACK` 写法。耗时日志等其它连接级横切机制收口时
/// 并入本入口（单独开票）。
pub fn write<T>(conn: &Mutex<Connection>, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let result = f(&conn);
    if result.is_ok() && conn.is_autocommit() {
        after_commit(&conn);
    }
    result
}

/// 提交点后置动作注册点（spec #1086 / issue #1088 基础设施 crate 归位）。
///
/// 连接层写入口在「闭包成功且已提交」时触碰的副作用按「下层定义注册点、上层
/// 注册实现、壳层启动时接线」形态挂在基础设施侧：本 crate 只承诺调用时机，
/// 不知道副作用语义——数据库与备份域之间不再有 crate 依赖边（备份域是业务域，
/// 基础设施不得反向引用）。实现由备份域提供、壳层在启动时注册；其余两类写路径
/// 挂载点（受影响账户余额重算、计划来源解析）由 #1090「写路径副作用接缝反转」
/// 收口为同一形态。
pub type AfterCommitHook = fn(&Connection);

static AFTER_COMMIT_HOOK: OnceLock<AfterCommitHook> = OnceLock::new();

/// 注册提交点后置动作实现（幂等：进程级一次，重复注册保留首次实现）。
///
/// 调用点在壳层启动接线与测试建库单点（`test_support::open`、BDD world），
/// 与生产同形；未注册时写入口报错误日志（`after_commit` 内），不静默。
pub fn register_after_commit_hook(hook: AfterCommitHook) {
    let _ = AFTER_COMMIT_HOOK.set(hook);
}

/// 提交点单点后置动作：委派给注册的实现（连接层内部实现细节，ADR-0032）。
///
/// 仅由 [`write`] 在「闭包成功且 `is_autocommit()`」时调用；闭包 Err（回滚）或
/// 未提交就返回（显式事务仍打开）不会到达本函数——「事务内推迟、提交后补上」
/// 由 [`write`] 的 `is_autocommit()` 复核结构保证。实现缺失即接线缺失，记错误
/// 日志使失败可见（不静默丢副作用）。
fn after_commit(conn: &Connection) {
    match AFTER_COMMIT_HOOK.get() {
        Some(hook) => hook(conn),
        None => tracing::error!(
            "db 提交点后置动作未注册：写已提交但置脏/到期检查被跳过（壳层启动接线缺失）"
        ),
    }
}

// ---------------------------------------------------------------------------
// 连接层统一 DB 调用 helper（形状乙，spec #498 / issue #501）
// ---------------------------------------------------------------------------

/// 连接层统一 DB 调用 helper：把 DB 闭包放到 tauri 全局运行时的阻塞线程池执行
/// （显式句柄 [`tauri::async_runtime::spawn_blocking`]，从 HTTP 壳自建运行时调用
/// 亦安全——返回的 JoinHandle 是跨运行时 future，生产先例
/// `fetch_fund_quote_for_api`），事件循环线程与 tokio worker 不再被 DB 调用占用。
///
/// - 闭包自带连接获取方式：读路径锁内执行（`conn.lock()`），写路径经连接层
///   统一写入口 [`write()`]（ADR-0032 置脏语义零改动）；
/// - `command` 用于在闭包内重建命令 span：异步命令与 wrapper 不同线程，SQL 耗时
///   归因靠这里兜底（lib.rs 异步命令归因约定，先例 `sync_instrument_info`）；
///   调用点已有活动 span 时（HTTP handlers 在 tower_http 请求 span 内运行）改为
///   携带调用方 span 与 dispatcher 跨线程执行——线程局部上下文不会自动跟随
///   spawn_blocking，显式带入后 HTTP 侧 SQL 归因沿请求 span 不漂移（API 集成
///   测试 `test_http_sql_duration_attributed_to_request_span` 钉死的行为）；
/// - 闭包的 `Result` 原样传播（业务错误不二次包装）；闭包 panic（JoinError）
///   归一化为 [`AppError::Io`]，与既有 `spawn_blocking` 先例同形。
///
/// 纯单测见 `db::tests::run_db`（闭包执行、Result 传播、上下文传播与重建）。
pub async fn run_db<T, F>(command: &'static str, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    // 捕获调用点当前 dispatcher 与 span（HTTP：tower_http 请求 span；IPC 异步
    // 命令：无）——两者都是线程局部，需显式带入阻塞线程。
    let dispatch = tracing::dispatcher::get_default(tracing::Dispatch::clone);
    let caller_span = tracing::Span::current();
    tauri::async_runtime::spawn_blocking(move || {
        tracing::dispatcher::with_default(&dispatch, || {
            let _caller = caller_span.enter();
            // 调用点无 span（IPC 异步命令）→ 重建命令 span 兜底维持归因。
            let command_span = tracing::info_span!("command", command);
            let _command = caller_span.is_none().then(|| command_span.enter());
            f()
        })
    })
    .await
    .map_err(|e| AppError::Io(format!("数据库任务执行失败: {e}")))?
}

// ---------------------------------------------------------------------------
// 应用状态
// ---------------------------------------------------------------------------

pub struct DbState {
    pub conn: Arc<Mutex<Connection>>,
}

impl DbState {
    /// 打开内存库并完成迁移，包成共享锁形态（单元测试与 BDD 世界用）。
    pub fn open_in_memory() -> Result<DbState> {
        let mut conn = open_in_memory()?;
        init_db(&mut conn)?;
        Ok(DbState {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// 写入口的命令层便捷形态（语义见 [`write()`]）：`state.write(|conn| ...)`。
    pub fn write<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        write(&self.conn, f)
    }
}
