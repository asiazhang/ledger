//! 门面句柄与按槽解析（ADR-0125 决策 1/4/8，issue #1410）：读 / 写两个**类型化
//! 句柄**、连接槽对，以及「按槽解析 / 进程级安装」的登记表。
//!
//! 与门面本体的分工（ADR-0111 决策 2 的 `db/` 按职责分文件）：[`super::facade`]
//! 持有工作线程、作业模型与 panic 恢复（「作业怎么跑」）；本模块持有「谁在什么
//! 作用下用什么形态投作业」（句柄、槽对、登记与回收）。门面本体是二者共用的
//! 执行核心，句柄只做方向限定与按槽解析。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::Connection;

use super::facade::DbFacade;
use crate::error::Result;

/// 连接槽对（写槽 + 读槽）：句柄与门面解析的唯一构造输入（issue #1410）。
///
/// 两槽同用「共享句柄 + 互斥体内槽替换」形态（ADR-0080 / ADR-0117 决策 3）——
/// 换连换的是槽内连接，槽对本身在换连前后不变，故句柄与门面都不必重建。
/// 内存测试库两槽同指一连接（`DbState::open_in_memory` 与测试工厂形态），与
/// 单连接时代语义一致。
#[derive(Clone)]
pub struct DbSlotPair {
    write: Arc<Mutex<Connection>>,
    read: Arc<Mutex<Connection>>,
}

impl DbSlotPair {
    /// 由两个共享槽构造槽对（`DbState` / HTTP 壳状态的具名访问器消费）。
    pub fn new(write: Arc<Mutex<Connection>>, read: Arc<Mutex<Connection>>) -> Self {
        DbSlotPair { write, read }
    }

    /// 写侧门面句柄（连接层与壳层统一写入口的消费形态）。
    pub fn write_handle(&self) -> DbWriteHandle {
        DbWriteHandle(self.clone())
    }

    /// 读侧门面句柄（壳层统一读入口的消费形态）。
    pub fn read_handle(&self) -> DbReadHandle {
        DbReadHandle(self.clone())
    }

    /// 写槽本体（**过渡形态的直锁半边**，ADR-0125 决策 8 首句：连接槽 `lock()`
    /// 允许住门面与壳层统一入口）。
    ///
    /// 消费方只剩壳层分段写入口的 `SegmentLock`（同步轮次路径）：标的信息同步
    /// 已随 #1412 改走 async 分段入口（门面裸作业会话）；同步轮次的 `RoundConn`
    /// 接缝（ADR-0120 决策 4）闭包借用轮次现场，不满足门面作业要求的
    /// `Send + 'static`，同步域异步化前仍走直锁（ADR-0125 决策 8 豁免台账）。
    /// 锁仍由本槽的同一把互斥体裁定，与门面写线程、换连原语互斥（#1276 的分段
    /// 语义不变）。
    pub fn write_slot(&self) -> &Arc<Mutex<Connection>> {
        &self.write
    }

    /// 本槽对的门面（按写槽解析，未登记则惰性拉起）。
    fn facade(&self) -> Result<Arc<DbFacade>> {
        facade_for(self)
    }

    /// 登记键：写槽的分配地址（`Arc` 地址在槽存活期间稳定——与运行时槽级的
    /// 换连代次表同款键选择）。
    fn registry_key(&self) -> usize {
        Arc::as_ptr(&self.write) as usize
    }
}

/// 写侧门面句柄（ADR-0125 决策 1 的「写类型化句柄」，issue #1410）：连接层与
/// 壳层统一写入口的消费形态——调用方不再取连接槽，只把作业交给句柄。
///
/// **类型化**：写句柄只能由 [`DbSlotPair::write_handle`]（经 `DbState` /
/// HTTP 壳状态的具名访问器）产出，读入口拿不到它——「读命令误取写槽连接」在
/// 类型上不可表达（ADR-0117 的读写分离由本形态承接）。
///
/// **句柄握槽不握线程**：句柄持槽对（写槽是解析键，读槽供门面启动），门面按
/// 写槽解析——门面线程的生命周期不随单个句柄起落，生产面由引导期安装的进程级
/// 门面（[`install_facade`]）常驻。
#[derive(Clone)]
pub struct DbWriteHandle(DbSlotPair);

/// 读侧门面句柄（ADR-0125 决策 1 的「读类型化句柄」）：语义与 [`DbWriteHandle`]
/// 对仗，只能投读作业——读路径不进写者闸门（ADR-0117）。
#[derive(Clone)]
pub struct DbReadHandle(DbSlotPair);

impl DbWriteHandle {
    /// 写作业（连接层统一写入口的「已持锁」形态在门面内落地）：提交点后置动作
    /// （置脏 + 写时到期检查）与 autocommit 复核语义与取锁形态同源。
    pub async fn run<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        self.0.facade()?.run_write(command, f).await
    }

    /// 写槽裸作业（不经连接层统一写入口的提交点后置动作）：读形态但闭包内含
    /// 惰性写的命令（缓存自愈 / 拼音惰性回填 / 漂移修复，ADR-0117 甄别结论）
    /// 走本形态——置脏与信号不由本作业触发，与迁移前「读入口消费写槽」逐字一致。
    /// 同步编排的分段取连接（作用域会话接缝，issue #1412）也走本形态：逐段
    /// autocommit，置脏与信号归分段入口的收尾裁决点。
    pub async fn run_raw<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        self.0.facade()?.run_write_raw(command, f).await
    }

    /// 写作业的**阻塞等待**形态：供阻塞线程上的编排体使用（分段写入口的收尾
    /// 裁决点，ADR-0073 / #1277——一次置脏、一次信号）。
    pub fn run_blocking<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        self.0.facade()?.run_write_blocking(command, f)
    }

    /// 写槽裸作业的阻塞等待形态：供阻塞线程上的编排体使用（设置 KV 写入、
    /// 检查点快照产出一类「短取一次连接」的步骤）。
    pub fn run_raw_blocking<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        self.0.facade()?.run_write_raw_blocking(command, f)
    }

    /// 写槽本体（分段写入口的过渡直锁半边，见 [`DbSlotPair::write_slot`]）。
    pub fn write_slot(&self) -> &Arc<Mutex<Connection>> {
        self.0.write_slot()
    }
}

impl DbReadHandle {
    /// 读作业：在读 DB 线程上执行。读侧与写侧各持一条线程——长写事务不把读排
    /// 在后面（ADR-0117 的实现形态，读路径不进写者闸门）。
    pub async fn run<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        self.0.facade()?.run_read(command, f).await
    }
}

/// 门面登记表条目（issue #1410）：按写槽分配地址登记的门面 + 是否进程级固定。
struct FacadeEntry {
    facade: Arc<DbFacade>,
    /// 进程级安装（[`install_facade`]，生产引导接线）置真：不受句柄引用计数回收——
    /// 连接不可信标记（ADR-0125 决策 3）等门面级状态必须活到换连（槽级）为止，
    /// 否则「标记后不静默恢复」会退化为「每个句柄各自一笔账」。
    pinned: bool,
}

/// 按写槽分配地址登记的门面表（进程内单点）。
static FACADES: OnceLock<Mutex<HashMap<usize, FacadeEntry>>> = OnceLock::new();

/// 门面登记表（惰性初始化：测试世界与生产共用同一张表）。
fn facades() -> &'static Mutex<HashMap<usize, FacadeEntry>> {
    FACADES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 按写槽解析门面，未登记则拉起（连接槽与成对构造点保持原形，ADR-0125 决策 4：
/// 测试世界的内存库工厂与 BDD world 构造零改动——线程化收在门面这一侧）。
///
/// **回收**：未固定（非进程级）且表内只剩一个强引用（句柄已全部释放）的条目
/// 在此停机回收——测试世界创建 / 销毁大量 `DbState`，线程不随条目无界增长；
/// 回收在表锁内判定，同刻解析到的句柄必然先把强计数抬上去，不存在「在用却被回收」。
fn facade_for(pair: &DbSlotPair) -> Result<Arc<DbFacade>> {
    let key = pair.registry_key();
    let mut table = facades()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    table.retain(|_, entry| entry.pinned || Arc::strong_count(&entry.facade) > 1);
    if let Some(entry) = table.get(&key) {
        return Ok(Arc::clone(&entry.facade));
    }
    let facade = Arc::new(DbFacade::start_slots(&pair.write, &pair.read)?);
    table.insert(
        key,
        FacadeEntry {
            facade: Arc::clone(&facade),
            pinned: false,
        },
    );
    Ok(facade)
}

/// 进程级安装门面（issue #1410）：生产引导在首次登记 `DbState` 后调用一次，
/// 此后命令层 / HTTP 壳 / 壳层统一入口按槽解析到的都是这一份——门面状态
/// （连接不可信标记、探针口径）跨命令常驻，换连换的是槽内连接（门面线程经
/// 换连代次识别，issue #1409），句柄与门面都不必重建。
///
/// 至多一份进程级门面：再次安装（重启引导、测试重复接线）时旧固定条目退役，
/// 其线程随之收尾——不留脱管线程、不把旧库连接拖在门面里。
pub fn install_facade(pair: &DbSlotPair) -> Result<()> {
    let key = pair.registry_key();
    let facade = Arc::new(DbFacade::start_slots(&pair.write, &pair.read)?);
    let mut retired = Vec::new();
    {
        let mut table = facades()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let stale: Vec<usize> = table
            .iter()
            .filter(|(existing, entry)| entry.pinned && **existing != key)
            .map(|(existing, _)| *existing)
            .collect();
        for existing in stale {
            if let Some(entry) = table.remove(&existing) {
                retired.push(entry);
            }
        }
        if let Some(previous) = table.insert(
            key,
            FacadeEntry {
                facade,
                pinned: true,
            },
        ) {
            retired.push(previous);
        }
    }
    // 退役在表锁外收尾：门面 Drop 会 join 线程，不在登记表临界区内做。
    drop(retired);
    Ok(())
}

/// 定点查询：该写槽是否已安装进程级门面（**仅测试构建可见**：接线证明的观察点，
/// 不进生产 API 面——生产接线由源码扫描守门核对）。
#[cfg(test)]
pub(crate) fn facade_installed(write: &Arc<Mutex<Connection>>) -> bool {
    let key = Arc::as_ptr(write) as usize;
    facades()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&key)
        .is_some_and(|entry| entry.pinned)
}
