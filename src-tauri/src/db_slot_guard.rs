use crate::signals_cross_check::STANDARD_LOCK_LINE;
use crate::sync_trigger_guard::{production_text, walk_rust_sources};
use crate::test_support::scan::mask_non_code;
use std::path::Path;

const FACADE_SOURCE: &str = "crates/infra/src/db/facade.rs";

const FACADE_SLOT_LOCK_TOKEN: &str = "watch.slot().lock()";

const LOCKING_WRITE_ENTRY_TOKEN: &str = "db::write(";

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
        "备份侧豁免（ADR-0125 决策 8 豁免台账同源）：只剩恢复的**分支锁**——主连接可选（启动失败\
         接管现场无连接不持锁，issue #601），锁形态超出门面单作业形状。首次兜底的取槽点已随 \
         issue #1415 改走门面限时等待写槽裸作业（`run_raw_blocking_within`），台账条目随之退役。",
    ),
    (
        "src/commands/sync_channel.rs",
        "引导段接缝（MainConnSegments，issue #1285）：整库换入需 `&mut Connection`，门面作业只递 \
         `&Connection`，作业形态表达不了可变借用——按 ADR-0125 决策 8 豁免登记（ADR-0117 决策 3/4 \
         的槽级换入先例同款）。",
    ),
];

fn scan_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

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

    for (rel, count) in &hits {
        assert!(
            SLOT_LOCK_EXEMPTIONS.iter().any(|(path, _)| path == rel),
            "生产面出现连接槽直锁（{rel}，{count} 处）——取用独占应收在异步 DB 门面与壳层统一\
             入口内（ADR-0125 决策 1/8，issue #1410）；如确属豁免，登记 \
             db_slot_guard::SLOT_LOCK_EXEMPTIONS 并附技术原因"
        );
    }

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

#[test]
fn backup_acquisition_points_go_through_facade_timed_job() {
    let shell = std::fs::read_to_string(scan_root().join("src/commands/backup.rs"))
        .expect("备份命令壳源码应可读");
    assert!(
        shell.contains("run_raw_blocking_within(\"backup.first-fallback\""),
        "首次兜底的连接取用必须经门面的限时等待写槽裸作业（ADR-0125 决策 8 豁免台账 \
         退役，issue #1415）——改回自取槽锁即回潮"
    );
    let scheduler = std::fs::read_to_string(scan_root().join("crates/backup/src/auto.rs"))
        .expect("备份调度源码应可读");
    for point in ["backup.scheduler-round", "backup.exit-fallback"] {
        assert!(
            scheduler.contains(&format!("run_raw_blocking_within(\"{point}\"")),
            "备份调度 / 退出兜底的连接取用必须经门面的限时等待写槽裸作业（{point}，\
             ADR-0125 决策 8 豁免台账退役，issue #1415）"
        );
    }
    let masked = production_text(&scheduler);
    let without_definition = masked.replacen("pub fn lock_conn_with_timeout(", "", 1);
    assert!(
        !without_definition.contains("lock_conn_with_timeout"),
        "备份调度域内不得再回到自取槽锁的等待形态（取用独占收在门面内，issue #1415）：\
         `lock_conn_with_timeout` 的余留消费面只有多端同步调度侧"
    );
}
