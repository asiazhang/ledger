//! 连接槽独占守门（ADR-0125 决策 8，issue #1410）：**生产面连接槽 `lock()` 只允许
//! 出现在异步 DB 门面与壳层统一入口**。
//!
//! 为什么守门：DB 取用独占收进门面后，「调用点自己取槽锁」是纪律退化的唯一入口
//! ——线程归属与锁跨度都靠门面保证，回潮一处就退回迁移前形态（#1403 现场类）。
//! 行为测试看不见「某文件里少了一行 lock」这类形状事实，只有源码扫描即红
//! （既有先例：ADR-0104 决策 6 的锁仪式反向守门、ADR-0073 决策 5 的身份扫描守门）。
//!
//! **判定形态**：壳层标准锁行（[`STANDARD_LOCK_LINE`]，与
//! `signals_cross_check::IPC_READ_ENTRY_EXCEPTIONS` 同款）。文本级扫描（掩码注释
//! 与字符串后匹配），换行拆写或别名改写不可达，靠评审兜底——与既有 bypass 守门
//! 同款取舍。扫描面 = 壳层源码全递归（旁路点就在壳层子目录与命令目录内）+ 基础设施
//! `db` 区之外的源码（`db` 区是门面与连接层正住址，连接槽机制本就住在那里）。
//!
//! **豁免台账**（[`SLOT_LOCK_EXEMPTIONS`]，有技术原因的豁免，不是范围偷懒）：
//! 逐条写动机 + 覆盖的文件；配**死条目断言**——白名单项在扫描面上零命中即红
//! （条目是规格，不留陈目）。多端同步调度侧的轮次连接与备份自动调度/追补的实现
//! 住各自域 crate（不在本守门的扫描面上），故不出现在本表——它们的「取不到就放弃
//! 本轮」语义在域侧由自身守门与评审承接（ADR-0120 决策 4 / ADR-0125 决策 8）。
//!
//! **反向半边**：门面本体（`crates/infra/src/db/facade.rs`）必须仍持有取槽锁的
//! 唯一实现——住址不可达或锁消失即红（守门不许「白名单吞掉全部命中」）。

use crate::signals_cross_check::STANDARD_LOCK_LINE;
use crate::sync_trigger_guard::{production_text, walk_rust_sources};
use crate::test_support::scan::mask_non_code;
use std::path::Path;

/// 门面取用独占住址（唯一合法持锁点，反向断言用）。
const FACADE_SOURCE: &str = "crates/infra/src/db/facade.rs";

/// 门面本体持有的**槽锁**形态（`SlotWatch::slot()` 上的 `lock()`）：必须与门面
/// 内其它互斥体（入队 sender、登记表）区分，否则删掉真正的取槽锁也能假绿。
const FACADE_SLOT_LOCK_TOKEN: &str = "watch.slot().lock()";

/// 已退役取锁写入口的回潮哨兵（`db::write(` / `ledger_infra::db::write(` 都含此
/// token）：取锁形态本体（`runtime::write` / `DbState::write`）已随 issue #1438
/// 删除——写路径一律经门面句柄（`DbWriteHandle::run*`）或测试面自取槽锁后直呼
/// `write_locked`。同名再入编译即错；本文本规则保留为**常设哨兵**，辖本守门
/// 扫描面（壳层 `src/**` + 基础设施非 `db` 区）：扫描面内出现即回潮——取锁
/// 形态绕开门面的取用独占，且不产生连接槽 `lock()` 文本，规则一抓不到；文本级
/// 扫描的别名盲区靠评审兜底（与本守门其余规则同款取舍）。域侧不在扫描面，
/// 同步轮次直锁半边（#1405）按豁免台账正交保留。
const LOCKING_WRITE_ENTRY_TOKEN: &str = "db::write(";

/// 豁免台账（ADR-0125 决策 8，issue #1410）：逐条技术原因 + 覆盖文件。
///
/// 表内每一项必须在扫描面上至少命中一次（死条目断言）；表外零命中是规格。
const SLOT_LOCK_EXEMPTIONS: &[(&str, &str)] = &[
    (
        "src/shell_support/write_entry.rs",
        "壳层统一写入口 · 分段形态（同步轮次半边）：每一段取连接仍是直锁（ADR-0125 决策 8 首句明许——连接槽 lock() \
         允许住门面与壳层统一入口）。标的信息同步已随 #1412 改走 async 分段入口（门面裸作业会话）；\
         同步轮次的 RoundConn 接缝（ADR-0120 决策 4）闭包借用轮次现场，不满足门面作业要求的 \
         Send + 'static，同步域异步化（#1405 另案）前仍走直锁；本形态与 DbWriteHandle::write_slot \
         随之保留。",
    ),
    (
        "src/commands/backup.rs",
        "备份侧豁免（ADR-0125 决策 8 豁免台账同源）：恢复的**分支锁**——主连接可选（启动失败接管现场\
         无连接不持锁，issue #601），锁形态超出门面单作业形状；同文件另有自动备份首次兜底的取槽点\
         （backup::lock_conn_with_timeout，同款「取不到就放弃本轮」语义，随域侧超时放弃口径）。",
    ),
    (
        "src/commands/sync_channel.rs",
        "引导段接缝（MainConnSegments，issue #1285）：整库换入需 `&mut Connection`，门面作业只递 \
         `&Connection`，作业形态表达不了可变借用——按 ADR-0125 决策 8 豁免登记（ADR-0117 决策 3/4 \
         的槽级换入先例同款）。",
    ),
];

/// 扫描面（相对 `src-tauri`）：壳层源码全递归 + 基础设施 `db` 区之外的源码。
fn scan_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// 扫描面上的（相对路径, 生产文本）清单：壳层 `src/` 全递归 + `crates/infra/src`
/// 去掉 `db` 区（门面与连接层正住址不受本守门辖）。
fn production_sources() -> Vec<(String, String)> {
    let mut raw = Vec::new();
    walk_rust_sources(&scan_root().join("src"), &mut raw);
    let mut infra = Vec::new();
    walk_rust_sources(&scan_root().join("crates/infra/src"), &mut infra);
    for (rel, text) in infra {
        if rel.starts_with("crates/infra/src/db/") {
            continue;
        }
        raw.push((rel, text));
    }
    raw.into_iter()
        .map(|(rel, text)| (rel, production_text(&text)))
        .collect()
}

/// 标准锁行在掩码文本上的命中数。
fn lock_hits(masked: &str) -> usize {
    masked.matches(STANDARD_LOCK_LINE).count()
}

#[test]
fn slot_lock_stays_in_facade_and_entries() {
    let sources = production_sources();
    let mut hits: Vec<(String, usize)> = Vec::new();
    for (rel, masked) in &sources {
        let count = lock_hits(masked);
        if count > 0 {
            hits.push((rel.clone(), count));
        }
    }

    // 白名单外零命中：出现手写连接槽锁行即红（回潮）。
    for (rel, count) in &hits {
        assert!(
            SLOT_LOCK_EXEMPTIONS.iter().any(|(path, _)| path == rel),
            "生产面出现连接槽直锁（{rel}，{count} 处）——取用独占应收在异步 DB 门面与壳层统一\
             入口内（ADR-0125 决策 1/8，issue #1410）；如确属豁免，登记 \
             db_slot_guard::SLOT_LOCK_EXEMPTIONS 并附技术原因"
        );
    }

    // 死条目断言：白名单项必须有命中——条目是规格，不留陈目（同款形制见
    // check-structure 白名单与 ADR-0104 决策 6 的例外清单核对）。
    for (path, note) in SLOT_LOCK_EXEMPTIONS {
        assert!(
            hits.iter().any(|(rel, _)| rel == path),
            "豁免台账条目 {path} 在扫描面上零命中（死条目）——豁免已不存在时同步删除条目；\
             动机：{note}"
        );
    }
}

#[test]
fn facade_holds_the_only_slot_lock() {
    let facade = std::fs::read_to_string(scan_root().join(FACADE_SOURCE)).unwrap_or_else(|e| {
        panic!("门面本体不可达 {FACADE_SOURCE}: {e}——取用独占的唯一合法住址不得搬迁不登记")
    });
    assert!(
        mask_non_code(&facade).contains(FACADE_SLOT_LOCK_TOKEN),
        "门面本体不再持有连接槽锁（{FACADE_SOURCE} 内搜不到 {FACADE_SLOT_LOCK_TOKEN}）——\
         取用独占的唯一实现处消失，守门拒绝「豁免吞掉全部命中」"
    );
}

/// 首次登记分支的接线核对（ADR-0125 决策 1/4，issue #1410）：引导序列首次登记
/// `DbState` 时必须安装**进程级**门面——门面状态（连接不可信标记、探针口径）跨
/// 命令常驻，缺此安装则「标记后不静默恢复」退化为每命令一重置。
///
/// 接线型判据的守门形态（ADR-0087）：进程级门面的可观察差异只出现在故障恢复态
/// （healthy 路径下惰性门面行为等价），故行为测试对准「引导后命令面可读写」
/// （`tests/db_facade_wiring.rs`），调用点在位由本源码扫描钉住——删除
/// `install_facade` 调用即红。
#[test]
fn first_registration_installs_process_level_facade() {
    let source = std::fs::read_to_string(scan_root().join("src/commands/boot.rs"))
        .expect("引导序列源码应可读");
    let masked = production_text(&source);
    let registration = masked
        .find("fn swap_or_manage_db_state")
        .expect("首次登记分派单点 swap_or_manage_db_state 应住在引导序列源码内");
    let body = &masked[registration..];
    assert!(
        body.contains("install_facade("),
        "首次登记分支缺 install_facade 调用——门面状态不再跨命令常驻，\
         连接不可信标记会随命令重置（ADR-0125 决策 1/3/4，issue #1410）"
    );
}

/// 壳层生产面零命中已退役取锁写入口（`db::write` 回潮哨兵，issue #1438）：
/// 门面独占的唯一取用面。
#[test]
fn shell_has_no_locking_write_entry_bypass() {
    let mut hits: Vec<String> = Vec::new();
    for (rel, masked) in production_sources() {
        if !rel.starts_with("src/") {
            continue;
        }
        if masked.matches(LOCKING_WRITE_ENTRY_TOKEN).count() > 0 {
            hits.push(rel);
        }
    }
    assert!(
        hits.is_empty(),
        "壳层生产面出现已退役取锁写入口 `db::write`（{hits:?}）——写路径一律经门面句柄\
         （ADR-0125 决策 1/2，issue #1410 / #1438）；确需连接槽取用的路径按豁免台账登记并说明"
    );
}

/// 扫描面自检：壳层与基础设施两侧都必须真的进面（扫描根写错即静默空扫，本守门
/// 与既有 bypass 守门同款「拒绝以空集假绿」口径）。
#[test]
fn scan_surface_covers_shell_and_infra() {
    let sources = production_sources();
    assert!(
        sources.iter().any(|(rel, _)| rel == "src/lib.rs"),
        "扫描面应含壳层源码（src/**）"
    );
    assert!(
        sources
            .iter()
            .any(|(rel, _)| rel == "crates/infra/src/runtime.rs"
                || rel.starts_with("crates/infra/src/")),
        "扫描面应含基础设施 db 区之外的源码"
    );
    assert!(
        !sources
            .iter()
            .any(|(rel, _)| rel.starts_with("crates/infra/src/db/")),
        "扫描面不应含基础设施 db 区（门面与连接层正住址）"
    );
    assert!(
        sources.len() > 50,
        "扫描面过小（{} 个文件）——扫描根或过滤写错即静默漏检",
        sources.len()
    );
}
