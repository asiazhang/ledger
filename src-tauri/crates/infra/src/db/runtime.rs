//! 连接层运行时接缝（自 `db/mod.rs` 按职责拆出，issue #1127，纯移动）：
//! 统一写入口与提交点后置钩子注册（ADR-0032 / spec #1086）、统一 DB 调用
//! helper（ADR-0069 形状乙，spec #498）与应用状态 [`DbState`]。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use rusqlite::Connection;

use super::connection::open_in_memory;
use super::migrate::init_db;
use crate::error::{AppError, Result};

/// 持锁时长探针阈值（issue #1276 守门③）：单次取锁的持有时长超过即记 warn。
/// 定值远大于任何合法单事务（毫秒级），远小于分钟级网络同步——越界即值得人工
/// 看一眼的异常信号，不是失败判定。
pub const LOCK_HOLD_PROBE_THRESHOLD: Duration = Duration::from_secs(1);

/// 运行时持锁时长探针（issue #1276 守门③，ADR-0069 决策 4 修订的守门半边）：
/// 连接锁单次持有的时长超过 [`LOCK_HOLD_PROBE_THRESHOLD`] 即记 warn 日志
/// ——「网络往返不得进锁」的纪律从注释与评审升级为运行时可观察的越界信号。
/// 探针只记日志、不改变行为：阈值取值远大于任何合法单事务（毫秒级）、远小于
/// 分钟级网络同步；越界即「慢闭包进锁」的嫌疑现场，由人工按坐标追认。
/// 取锁点全部接哨：门面写线程（作业占用 DB 线程时长，ADR-0125 决策 2）、壳层
/// 读入口（`read_entry`）与分段写入口的 `SegmentLock`（壳层
/// `shell_support::write_entry`，#1108 迁出根包）。
///
/// **门面形态的语义承接**（ADR-0125 决策 2，issue #1408）：异步 DB 门面把取锁
/// 收进 DB 线程后，同一探针的量纲由「持有互斥锁时长」变为「**作业占用 DB 线程
/// 时长**」——门面线程为整个作业持有槽锁，两者在门面形态下同区间；阈值与告警
/// 口径（越界即 warn、不静默）不变，故共用本函数，不另立第二套阈值。
///
/// 产品路径经 [`probe_lock_hold`] 取 [`LOCK_HOLD_PROBE_THRESHOLD`]；本函数把阈值
/// 开放为参数，供调用点自持阈值——多端同步调度侧的轮次连接源据此注入测试短阈值
/// （「超阈值即记 warn」是瞬时可判定语义，实等 1.1s 只为越过产品阈值，不含信息量，
/// spec #1086 / issue #1514）。
pub fn probe_lock_hold_within(hold: Duration, threshold: Duration) {
    if hold >= threshold {
        tracing::warn!(
            hold_ms = hold.as_millis() as u64,
            threshold_ms = threshold.as_millis() as u64,
            "连接锁持有时长超过阈值（疑似慢闭包进锁，ADR-0069 决策 4：分钟级网络往返不得进锁）"
        );
    }
}

/// 产品阈值形态的持锁时长探针：阈值取 [`LOCK_HOLD_PROBE_THRESHOLD`]。
pub fn probe_lock_hold(hold: Duration) {
    probe_lock_hold_within(hold, LOCK_HOLD_PROBE_THRESHOLD);
}

// ---------------------------------------------------------------------------
// 连接层统一写入口（ADR-0032）
// ---------------------------------------------------------------------------

/// 连接层统一写入口 · **已持锁形态**（ADR-0125 决策 2，issue #1408）：调用方
/// 已持有连接槽锁（异步 DB 门面的 DB 线程），本函数只做写入口的语义部分——
/// 执行写闭包 → 「闭包成功且已回到提交点（`is_autocommit()`）」单点执行置脏 +
/// 写时到期检查。取锁、锁失败映射与持锁时长探针归取锁方。
///
/// 置脏触发的单点，业务写路径对备份域零感知：
/// - 执行闭包；
/// - 闭包成功且事务已提交（`is_autocommit()`，含闭包内部自行 `BEGIN`/`COMMIT`
///   后回到提交点）→ 单点执行置脏 + 写时顺带到期检查；
/// - 闭包失败（事务回滚）不置脏；未提交就返回（显式事务仍打开）同样不置脏——
///   「事务内推迟、提交后补上」由本结构保证，不再依赖调用方注释口口相传。
///
/// 豁免清单集中在「不经过本入口的写方」：设置与调度状态写入（`app_settings`
/// 全表，经 [`crate::settings`] 单点收口）与恢复（Restore）路径。
///
/// 薄 wrapper 边界：只做「写后置动作/检查」，不接管事务管理——闭包内保留
/// 裸 `BEGIN`/`COMMIT`/`ROLLBACK` 写法（原生事务语句的合法住址见
/// [`crate::db::tx_scope`]）。耗时日志等其它连接级横切机制收口时并入本入口
/// （单独开票）。
///
/// **唯一形态**（issue #1438）：取锁形态（`runtime::write` / `DbState::write`）
/// 已退役——置脏语义只有本函数一处实现。生产面写路径一律经门面作业在本函数上
/// 执行（门面线程持锁，闭包内再取同一槽即自死锁，由 ADR-0125 决策 1 的取用独占
/// 与结构守门挡住）；取锁与持锁时长探针归取锁方（门面线程，或测试面自取槽锁后
/// 直呼本函数）。
pub fn write_locked<T>(conn: &Connection, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let result = f(conn);
    if result.is_ok() && conn.is_autocommit() {
        after_commit(conn);
    }
    result
}

/// 提交点后置动作注册点（spec #1086 / issue #1088 基础设施 crate 归位）。
///
/// 连接层写入口在「闭包成功且已提交」时触碰的副作用按「下层定义注册点、上层
/// 注册实现、壳层启动时接线」形态挂在基础设施侧：本 crate 只承诺调用时机，
/// 不知道副作用语义——数据库与备份域之间不再有 crate 依赖边（备份域是业务域，
/// 基础设施不得反向引用）。实现由备份域提供、壳层在启动时注册；其余两类写路径
/// 挂载点（受影响账户余额重算、计划来源解析）已由 #1090「写路径副作用接缝反转」
/// 收口为同一形态（核心交易域 `transaction::seams::balance` / `transaction::seams::source`
/// 注册点，账户域 / 定时计划域实现），期次落账置脏同票收口（定时计划域
/// `auto_run` 注册点、备份域实现）。
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
/// 仅由 [`write_locked`] 在「闭包成功且 `is_autocommit()`」时调用；闭包 Err
/// （回滚）或未提交就返回（显式事务仍打开）不会到达本函数——「事务内推迟、
/// 提交后补上」由 [`write_locked`] 的 `is_autocommit()` 复核结构保证。实现缺失
/// 即接线缺失，记错误日志使失败可见（不静默丢副作用）。
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
/// - 闭包自带连接获取方式：读路径与免置脏直写（设置 KV、检查点快照一类）在
///   闭包内持锁执行；置脏语义的写路径不经本 helper——一律走异步 DB 门面的写
///   作业（连接层统一写入口 [`write_locked`] 在门面线程内执行，ADR-0125）；
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
        with_caller_context(&dispatch, &caller_span, command, f)
    })
    .await
    .map_err(|e| AppError::Io(format!("数据库任务执行失败: {e}")))?
}

/// 跨线程携带调用点上下文的执行单点（ADR-0069 决策 3）：在调用点 dispatcher 下、
/// 以调用点 span 为当前 span 执行 `f`；调用点无 span（IPC 异步命令）时重建
/// `command` span 兜底——SQL 耗时归因口径的唯一实现处，阻塞线程池 helper
/// [`run_db`] 与异步 DB 门面（`db::facade`）共用，归因不因形态而漂移。
///
/// 调用方负责在调用点（异步上下文那一侧）捕获 [`tracing::dispatcher::get_default`]
/// 与 [`tracing::Span::current`] 并随闭包带入线程——线程局部上下文不会自动跟随。
pub(crate) fn with_caller_context<R>(
    dispatch: &tracing::Dispatch,
    caller_span: &tracing::Span,
    command: &'static str,
    f: impl FnOnce() -> R,
) -> R {
    tracing::dispatcher::with_default(dispatch, || {
        let _caller = caller_span.enter();
        let command_span = tracing::info_span!("command", command);
        let _command = caller_span.is_none().then(|| command_span.enter());
        f()
    })
}

// ---------------------------------------------------------------------------
// 连接槽换连代次（ADR-0125 决策 2「换连承接」/ 决策 3「退役或重建」，issue #1409）
// ---------------------------------------------------------------------------

/// 连接槽换连代次表：以槽（`Arc<Mutex<Connection>>`）的分配地址为键，登记一个
/// 单调代次。
///
/// **为什么需要它**：连接槽受 ADR-0125 决策 4 保护（测试世界与调用面零改造，
/// `DbState` 不加字段），成对换连原语（[`DbState::swap_pair`] /
/// [`replace_read_conn_slot`]）因此无法在类型上把「本槽已被换连」告知异步 DB
/// 门面；而门面的「连接不可信」标记（ADR-0125 决策 3）是**连接**级的——换连换
/// 进来的是另一条连接，旧连接的失败态不该被它继承。决策 3 把「是否退役或重建
/// 该连接」交给实施票按现场判定，issue #1409 判定为：换连即重建，标记随之复位。
/// 代次把这个事实旁挂在槽之外，门面线程取到槽锁后比对即识别换连，不动槽类型。
///
/// 键用槽 Arc 的分配地址：Arc 存活期间地址稳定，且一个地址不可能同时对应两条
/// 存活的 Arc；值为 `Weak`，槽全部释放后条目自然失效并在下次登记时清理（测试
/// 世界创建 / 销毁大量 `DbState`，防表无界增长）。
static SLOT_SWAP_EPOCHS: OnceLock<Mutex<HashMap<usize, Weak<AtomicU64>>>> = OnceLock::new();

/// 取槽的换连代次句柄（未登记则新建）；消费方经 [`SlotWatch`] 使用，不直接持有。
fn slot_swap_epoch_handle(slot: &Arc<Mutex<Connection>>) -> Arc<AtomicU64> {
    let key = Arc::as_ptr(slot) as usize;
    let mut table = SLOT_SWAP_EPOCHS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(existing) = table.get(&key).and_then(Weak::upgrade) {
        return existing;
    }
    // 顺带清理失效条目（槽已释放、代次句柄随之消失）；表只随存活槽增长。
    table.retain(|_, weak| weak.strong_count() > 0);
    let epoch = Arc::new(AtomicU64::new(0));
    table.insert(key, Arc::downgrade(&epoch));
    epoch
}

/// 抬高槽的换连代次（成对换连原语在**持有槽锁期间**调用）：门面线程取到槽锁
/// 后读到的代次要么已含本次换连、要么不含，不存在「换连已落地但代次未抬」的
/// 窗口（锁内抬高与锁内读取代次的先后由互斥锁排序）。
fn bump_slot_swap_epoch(slot: &Arc<Mutex<Connection>>) {
    slot_swap_epoch_handle(slot).fetch_add(1, Ordering::SeqCst);
}

/// 连接槽观测句柄（ADR-0125 决策 2 的换连承接，issue #1409）：把「观测哪个槽」与
/// 「上次看到的换连代次」绑成一件。异步 DB 门面每条 DB 线程持有一个——取到槽锁
/// 后经 [`SlotWatch::observe_swap`] 识别换连，据此复位连接不可信标记。
///
/// 句柄持槽的 Arc 克隆，故槽的分配地址在门面存活期间稳定——代次表
/// （[`SLOT_SWAP_EPOCHS`]）的键在门面使用期间不会失效，也不会被另一条槽顶替。
pub(crate) struct SlotWatch {
    /// 被观测的连接槽（门面为整个作业持它的锁）。
    slot: Arc<Mutex<Connection>>,
    /// 本槽的换连代次（与换连原语的抬高同源）。
    epoch: Arc<AtomicU64>,
    /// 上一次观测到的代次（基线）。`Cell` 而非 `&mut`：观测发生在槽锁守卫仍在手的
    /// 时候（守卫借自本结构），单线程使用，故内部可变即可。
    seen: std::cell::Cell<u64>,
}

impl SlotWatch {
    /// 开始观测一个连接槽：取出（或新建）本槽的换连代次，并记下当前代次作基线。
    pub(crate) fn new(slot: &Arc<Mutex<Connection>>) -> Self {
        let epoch = slot_swap_epoch_handle(slot);
        let seen = epoch.load(Ordering::SeqCst);
        SlotWatch {
            slot: Arc::clone(slot),
            epoch,
            seen: std::cell::Cell::new(seen),
        }
    }

    /// 被观测的槽（门面取锁用）。
    pub(crate) fn slot(&self) -> &Mutex<Connection> {
        &self.slot
    }

    /// 自上次观测以来是否发生过换连。**须在持有本槽锁之后调用**：换连原语在槽锁
    /// 内抬高代次，锁内比对才没有竞速窗口（锁外读会读到旧代次却已用上新连接）。
    pub(crate) fn observe_swap(&self) -> bool {
        let current = self.epoch.load(Ordering::SeqCst);
        if current == self.seen.get() {
            return false;
        }
        self.seen.set(current);
        true
    }
}

// ---------------------------------------------------------------------------
// 应用状态
// ---------------------------------------------------------------------------

/// 应用状态：写连接 + 只读读连接（读路径独立只读连接，issue #1280 / ADR-0117）。
///
/// - `conn`（写连接）：维持单写者互斥——统一写入口 [`write_locked`]（经门面写
///   作业执行，issue #1438 起本状态不再提供取锁便捷形态）与壳层统一写入口
///   `shell_support::write_entry`（#1108 迁出根包）等既有接缝原样（ADR-0104：
///   `read_entry` 消费的句柄类型不变，变的只是传入句柄指向读连接）；
/// - `read_conn`（读连接）：只服务壳层统一读入口 `shell_support::read_entry`——只读
///   flags + busy_timeout，读可用性不再受写者闸门约束。
///
/// 两槽同用「共享句柄 + 互斥体内槽替换」形态（ADR-0080：换入后壳层、HTTP 壳、
/// 调度线程已持有的克隆同步可见）；换入/换出收口在既有换连单点（引导序列与
/// 解锁/重置编排），成对换连，禁止任何路径手搓第二条连接绕开收口（ADR-0117
/// 决策 4）。
pub struct DbState {
    pub conn: Arc<Mutex<Connection>>,
    pub read_conn: Arc<Mutex<Connection>>,
}

impl DbState {
    /// 连接对构造单点（issue #1303）：裸连接对 → 共享锁形态的 [`DbState`]。
    /// 生产结构体字面量全部经本构造器（`open_db_in` / `reset_db_in` + 壳层
    /// 首登记），字段直拼不再新增。
    pub fn from_pair(conn: Connection, read_conn: Connection) -> Self {
        DbState {
            conn: Arc::new(Mutex::new(conn)),
            read_conn: Arc::new(Mutex::new(read_conn)),
        }
    }

    /// 打开内存库并完成迁移，包成共享锁形态（单元测试与 BDD 世界用）。
    ///
    /// 内存库按连接隔离（第二条内存连接是另一个空库），读槽与写槽共享同一
    /// 连接句柄——读写共用同锁同库，与单连接时代测试语义一致（文件库场景
    /// 的成对形态见 [`crate::db::open_db_in`]）。
    pub fn open_in_memory() -> Result<DbState> {
        let mut conn = open_in_memory()?;
        init_db(&mut conn)?;
        let conn = Arc::new(Mutex::new(conn));
        Ok(DbState {
            read_conn: conn.clone(),
            conn,
        })
    }

    /// 成对换入原语（issue #1303 / ADR-0117 决策 3）：「锁写槽→替换→还锁→
    /// 换读槽」的机械序列单点——返回后可观察行为是写读两槽指向同一库；机械
    /// 序列内两槽可能短暂分属两库的窗口由调用点的门翻转次序屏蔽（进锁定/失败
    /// 先立门再换出、回到就绪先换入再开门，fail-closed，ADR-0117 决策 3），
    /// 业务读写触达不到。收口既有换连单点（引导序列连接换入步骤与解锁/重置
    /// 编排）消费，禁止任何路径手搓第二个副本绕开本原语（ADR-0117 决策 4
    /// 换连半边，壳层源扫描守门）。
    ///
    /// 换连两侧都在槽锁内抬高本槽的换连代次（issue #1409 / ADR-0125 决策 2 的
    /// 换连承接）：异步 DB 门面的 DB 线程取到槽锁后比对代次即识别换连，据此
    /// 复位连接不可信标记（决策 3 的「退役 / 重建」）。
    pub fn swap_pair(&self, conn: Connection, read_conn: Connection) -> Result<()> {
        let mut guard = self.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        *guard = conn;
        // 换连代次在槽锁内抬高（issue #1409）：异步 DB 门面的 DB 线程取到槽锁后
        // 读到的代次要么已含本次换连、要么不含，识别不依赖时序。
        bump_slot_swap_epoch(&self.conn);
        drop(guard);
        self.replace_read_conn(read_conn)
    }

    /// 原位换入读连接（成对换连的读侧，issue #1280 / ADR-0117 决策 3）。
    ///
    /// `pub(crate)`（issue #1303）：生产面换入一律走成对原语 [`DbState::swap_pair`]，
    /// 读槽单槽换入在本 crate 之外再无合法调用点——绕开成对原语编译即错（守门
    /// 的编译期半边）。
    pub(crate) fn replace_read_conn(&self, conn: Connection) -> Result<()> {
        replace_read_conn_slot(&self.read_conn, conn)
    }

    /// 读连接占位化（恢复 / 整库转换的原位换出，issue #1280 / ADR-0117 决策 3）：
    /// 换入占位内存库（无业务表）——库文件已被替换/重命名后，旧读连接仍指旧
    /// inode（rename 后句柄仍有效），继续读会给出陈旧数据，故换出不得等到重启；
    /// 占位化后读命令报错（占位无表，归一化 AppError::Db），不静默回落写连接。
    pub fn placeholderize_read_conn(&self) -> Result<()> {
        self.replace_read_conn(open_in_memory()?)
    }

    /// 连接槽对（ADR-0125 决策 1/4，issue #1410）：句柄与门面解析的唯一构造输入。
    /// 连接槽与成对构造点保持原形（本访问器不改变状态形状），线程化收在门面一侧。
    pub fn slots(&self) -> super::facade_handles::DbSlotPair {
        super::facade_handles::DbSlotPair::new(Arc::clone(&self.conn), Arc::clone(&self.read_conn))
    }

    /// 写侧门面句柄：命令层与壳层统一写入口的取用形态——调用方不再取连接槽，
    /// 只把作业交给句柄。
    pub fn write_handle(&self) -> super::facade_handles::DbWriteHandle {
        self.slots().write_handle()
    }

    /// 读侧门面句柄：读入口的取用形态。读句柄只投读作业，读路径不进写者闸门
    ///（ADR-0117）。
    pub fn read_handle(&self) -> super::facade_handles::DbReadHandle {
        self.slots().read_handle()
    }
}

/// 读连接槽的原位替换（共享句柄 + 互斥体内槽替换，ADR-0080 / ADR-0117 决策 3）：
/// 已持有的 Arc 克隆同步可见；旧读连接随替换立即关闭（指向旧 inode 的句柄
/// 不再存活）。恢复路径持有 [`DbState`] 的读槽 Arc 克隆而非应用状态句柄
///（无 DbState 时命令仍可用，issue #601 前置修复），经本自由函数消费同一机制。
pub fn replace_read_conn_slot(slot: &Arc<Mutex<Connection>>, conn: Connection) -> Result<()> {
    let mut guard = slot.lock().map_err(|e| AppError::Db(e.to_string()))?;
    // 旧连接先出槽再显式丢弃（关闭旧文件句柄），新连接同刻就位。
    drop(std::mem::replace(&mut *guard, conn));
    // 读槽换连代次（issue #1409）：与成对原语写槽半边同址同形制。
    bump_slot_swap_epoch(slot);
    Ok(())
}
