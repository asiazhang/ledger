use crate::test_support::scan::{mask_non_code, matching_brace_end};
use std::path::Path;

fn all_rust_sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    walk_rust_sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    out
}

pub(crate) fn walk_rust_sources(dir: &Path, out: &mut Vec<(String, String)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("扫描目录不可读 {dir:?}: {e}"))
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            walk_rust_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let rel = path
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .expect("源码在 manifest 内")
                .to_string_lossy()
                .into_owned();
            out.push((rel, std::fs::read_to_string(&path).expect("源码应可读")));
        }
    }
}

pub(crate) fn production_text(src: &str) -> String {
    let mut masked = mask_non_code(src);
    while let Some(anchor) = masked.find("#[cfg(test)]") {
        let rest = &masked[anchor..];
        let declaration_end = rest.find(';').map(|p| anchor + p);
        let brace = rest.find('{').map(|p| anchor + p);
        let end = match (declaration_end, brace) {
            (Some(semi), None) => semi + 1,
            (_, Some(brace)) => matching_brace_end(&masked, brace).unwrap_or(masked.len()),
            (None, None) => rest.find('\n').map_or(masked.len(), |p| anchor + p),
        };
        let blank = " ".repeat(end - anchor);
        masked.replace_range(anchor..end, &blank);
    }
    masked
}

#[test]
fn sync_triggers_start_from_every_business_surface_via_single_entry() {
    let sources = all_rust_sources();

    for (path, src) in &sources {
        let text = production_text(src);
        for entry in ["start_sync_scheduler(", "sync_on_start("] {
            assert!(
                !text.contains(entry),
                "{path} 直呼触发内部入口 `{entry}`——分平台门与触发时机编排只收在 \
                 start_triggers 单点（ADR-0098 决策 4），业务可用起点应经 \
                 start_background_services 编排点拉起（issue #961）"
            );
        }
        for entry in ["start_triggers(", "start_background_services("] {
            let mut search_from = 0usize;
            while let Some(rel) = text[search_from..].find(entry) {
                let call_pos = search_from + rel;
                let window = &text[call_pos.saturating_sub(60)..call_pos];
                for gate in [
                    "cfg(desktop)",
                    "cfg(mobile)",
                    "cfg!(desktop)",
                    "cfg!(mobile)",
                ] {
                    assert!(
                        !window.contains(gate),
                        "{path} 的 {entry} 调用贴身出现平台门 `{gate}`——调用点不得 \
                         分平台包装（门只收在 start_triggers 单点，移动端只保留打开即同步）"
                    );
                }
                search_from = call_pos + entry.len();
            }
        }
    }

    let mut callers: Vec<&str> = sources
        .iter()
        .filter(|(_, src)| production_text(src).contains("start_triggers("))
        .map(|(path, _)| path.as_str())
        .collect();
    callers.sort_unstable();
    assert_eq!(
        callers,
        ["src/lib.rs"],
        "start_triggers 调用方漂移——生产调用只能住在壳层唯一编排点 \
         start_background_services（issue #961：与自动备份成对拉起；TS 侧同规则见 \
         check-background-services.ts，两侧互为冗余）"
    );

    let mut orchestrator_callers: Vec<&str> = sources
        .iter()
        .filter(|(_, src)| production_text(src).contains("start_background_services("))
        .map(|(path, _)| path.as_str())
        .collect();
    orchestrator_callers.sort_unstable();
    let orchestrator_expected = [
        "src/commands/boot.rs",
        "src/commands/encryption.rs",
        "src/lib.rs",
    ];
    assert_eq!(
        orchestrator_callers, orchestrator_expected,
        "start_background_services 调用方漂移——三业务可用起点（setup 就绪 / \
         resume_business_surface / restart_app 落 Ready）缺一不可，新增起点须同步登记本守门\
         （ADR-0098 决策 4：起点不在本清单即「打开即同步」本会话不生效；#961 后起点 \
         经唯一编排点接线，成对性由 check-background-services.ts 钉住）"
    );
}

#[test]
fn session_envelope_writes_stay_in_closed_set_with_resume_write_first() {
    let sources = all_rust_sources();

    let mut writers: Vec<&str> = sources
        .iter()
        .filter(|(_, src)| {
            let text = production_text(src);
            text.contains("SessionEnvelope::remember(") || text.contains("SessionEnvelope::forget(")
        })
        .map(|(path, _)| path.as_str())
        .collect();
    writers.sort_unstable();
    assert_eq!(
        writers,
        [
            "src/commands/boot.rs",
            "src/commands/encryption.rs",
            "src/commands/sync_channel.rs",
        ],
        "SessionEnvelope 记忆生产调用点漂移——记入/清空只许住在 resume 体内与三处
         保留原位（encryption.rs / boot.rs restart_app / sync_channel.rs sync_now，
         issue #1395 / ADR-0098 决策 3 修订注记）；业务可用起点应经 resume 签名
         声明信封形态，不新增散落调用点"
    );

    let src = &sources
        .iter()
        .find(|(path, _)| path == "src/commands/encryption.rs")
        .expect("encryption.rs 应在壳层源码内")
        .1;
    let text = production_text(src);
    let resume_pos = text
        .find("fn resume_business_surface")
        .expect("resume_business_surface 应在 encryption.rs 内");
    let remember_rel = text[resume_pos..]
        .find("SessionEnvelope::remember(")
        .unwrap_or_else(|| {
            panic!(
                "resume 体内缺信封写入——解锁/忘记口令重置/启动失败重置三起点的会话形态
             应随 resume 签名声明、写入为函数体首行（issue #1395 / ADR-0098 决策 3）"
            )
        });
    let start_rel = text[resume_pos..]
        .find("start_background_services(")
        .expect("resume 应经唯一编排点拉起后台服务（issue #961）");
    assert!(
        remember_rel < start_rel,
        "resume 体内信封写入必须先于 start_background_services——打开即同步与写后
         触发即时消费会话形态（issue #1395 / ADR-0098 决策 3）；信封写入被移到拉起
         之后或整体删除即在此变红"
    );
}

#[test]
fn sync_desktop_gate_is_a_single_point_in_start_triggers_body() {
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/sync-engine/src/trigger/scheduler.rs"),
    )
    .expect("scheduler.rs 应可读");
    let text = production_text(&src);
    assert_eq!(
        text.matches("#[cfg(desktop)]").count(),
        1,
        "桌面分平台门（#[cfg(desktop)]）只能收在 start_triggers 一处——门回到调用点 \
         即重现「解锁路径漏门」回归（ADR-0098 决策 4：分平台分流只在本函数一处）"
    );
    let pos = |needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("触发编排源码缺 `{needle}`——分平台门形状漂移"))
    };
    let (fn_pos, cfg_pos, sched_pos, start_pos) = (
        pos("fn start_triggers"),
        pos("#[cfg(desktop)]"),
        pos("start_sync_scheduler(app);"),
        pos("sync_on_start(app);"),
    );
    assert!(
        fn_pos < cfg_pos && cfg_pos < sched_pos && sched_pos < start_pos,
        "分平台门形状漂移：desktop 门应贴在 start_triggers 内的 start_sync_scheduler \
         调用上，sync_on_start（打开即同步，Android 兜底语义）在门外全平台跑"
    );
}
