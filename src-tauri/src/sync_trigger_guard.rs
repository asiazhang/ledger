//! 同步触发接线的源码扫描守门（issue #959 / ADR-0098 决策 4）：触发编排的行为
//! 半边由 mock 应用集成测试钉住，但「分平台门收在 start_triggers 单点、三业务
//! 可用起点统一调 start_triggers」是根包源码形状事实——删掉 `lib.rs` 的
//! `start_triggers` 调用或把门搬回调用点，行为测试无从得知（集成测试不运行
//! setup），只有扫描即红。
//!
//! 本守门置于根包侧（#1107 同步域拆出后仍如此）：它核对的是壳层启动接线与
//! 根包源码形状，不随领域行为迁入 `ledger-sync-engine`。扫描器具复用
//! [`crate::signals_cross_check::mask_non_code`]，规则无第二份。

use crate::signals_cross_check::mask_non_code;
use std::path::Path;

/// `src/**` 下全部 .rs 的（仓库相对路径, 源文本），按路径排序（输出确定）。
fn all_rust_sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    walk_rust_sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    out
}

/// 遍历一个目录下的全部 `.rs`，产出（相对 `src-tauri` 路径, 源文本），按路径排序
/// （输出确定）。扫描器单点：同步触发守门与连接槽独占守门（`db_slot_guard`）共用，
/// 扫描面差异只在根清单，不在遍历实现。
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

/// 生产文本：掩码注释与字符串后，逐个剔除 `#[cfg(test)]` 附属的模块/函数体
/// ——测试模块内合法直呼触发入口不参与守门。剔除在**掩码文本**上做花括号
/// 配对（字符串与注释已空白化，配对可靠；lib.rs 的 cfg(test) 模块声明在文件
/// 前部，不能按「首个锚点截到文件尾」——会误伤其后的 setup 段，issue #959
/// 守门首版踩过）。掩码与剔除全程保长，切片下标同位。
// 生产文本抽取在守门间复用（连接槽独占守门 `db_slot_guard` 与本守门同款扫描面）。
pub(crate) fn production_text(src: &str) -> String {
    let mut masked = mask_non_code(src);
    while let Some(anchor) = masked.find("#[cfg(test)]") {
        let rest = &masked[anchor..];
        let declaration_end = rest.find(';').map(|p| anchor + p);
        let brace = rest.find('{').map(|p| anchor + p);
        let end = match (declaration_end, brace) {
            // 声明式模块（`#[cfg(test)] mod tests;`）：剔到分号。
            (Some(semi), None) => semi + 1,
            // 内联体（`#[cfg(test)] mod tests { … }`）：花括号配对剔到闭括号。
            (_, Some(brace)) => {
                let mut depth = 0usize;
                let mut end = masked.len();
                for (idx, ch) in masked[brace..].char_indices() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = brace + idx + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                end
            }
            // 分号与花括号都没有：异常源码形状，剔到行尾保守处理。
            (None, None) => rest.find('\n').map_or(masked.len(), |p| anchor + p),
        };
        let blank = " ".repeat(end - anchor);
        masked.replace_range(anchor..end, &blank);
    }
    masked
}

/// 触发编排单一入口被全部业务可用起点调用（ADR-0098 决策 4；#961 成对编排点）：
/// 反向核对「域外不得直呼两个内部触发入口」（绕开单点即红），正向核对两层接线：
/// ① `start_triggers(` 的生产调用方恰为壳层唯一编排点 `start_background_services`
///    （issue #961：后台服务成对拉起收进单点；与 check-background-services.ts
///    的住址规则互为冗余）；
/// ② `start_background_services(` 的调用方恰为三业务可用起点——删掉任一起点的
///    编排点调用（#863 第二版曾漏 restart_app 落 Ready 这个起点），或新增起点
///    未登记，在此即红。起点覆盖是本守门独有职责：check-background-services
///    只钉域入口住址与编排点内成对性，不钉起点都调编排点。
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
        // 调用点贴身窗口（60 字符）不得出现平台门属性/宏：把接线调用包进
        // #[cfg(desktop)]（移动端「打开即同步」静默失效，ADR-0098 决策 4 的
        // 「加门」回归形态）是文本可查的。对 start_triggers（编排点体内）与
        // start_background_services（三起点）两个标识符都查——门搬去哪一层都红。
        // 窗口贴身即不含远处合法的 desktop 门（lib.rs 的 api_server 等）；文本级
        // 扫描的其余盲区（间接别名、远距包装）按 signals_cross_check 同款纪律
        // 靠评审兜底。
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

    // ① 域入口的唯一生产调用方 = 唯一编排点。定义点不在本清单：`start_triggers`
    //    的定义在 `ledger-sync-engine` 内且带泛型参数列表，不含裸 `start_triggers(`
    //    子串；定义点自身的分平台门形状由
    //    `sync_desktop_gate_is_a_single_point_in_start_triggers_body` 钉住。
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

    // ② 三业务可用起点都调唯一编排点（编排点定义在 lib.rs，故 lib.rs 也在清单：
    //    定义带裸 `start_background_services(` 子串，随清单一并核销）。
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

/// 会话信封记忆的生产调用点闭集与 resume 体内位次（issue #1395 / ADR-0098 决策
/// 3 修订注记）：「业务可用起点 → SessionEnvelope 记忆」的关系已编码进
/// `resume_business_surface` 签名（纯值参数 `session`，写入为函数体首行），
/// 生产调用只许住在闭集三处（各有时序理由，ADR-0098 决策 3 修订注记）：
/// - `encryption.rs`：resume 体内写入（解锁/重置三起点随签名声明）+ 关闭加密
///   记明文（待重启、不经 resume）；
/// - `boot.rs`：`restart_app` 记 forget（必须先于换库）；
/// - `sync_channel.rs`：`sync_now` 成功轮次后记入（时机归域侧）。
///
/// 位次半边：resume 体内信封写入必须先于尾部 `start_background_services`
/// 调用——打开即同步（sync_on_start）与写后触发即时消费会话形态；写入移到
/// 拉起之后或整体删除（后者另使独立集成测试红，双保险）即在此红。
#[test]
fn session_envelope_writes_stay_in_closed_set_with_resume_write_first() {
    let sources = all_rust_sources();

    // 闭集：生产调用点（注释与字符串掩码后）只许住三处保留原位。
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

    // 位次：resume 体内信封写入先于尾部 start_background_services 调用
    //（desktop-gate 位次比较同款）。写入整体删除时同样红——命中退化为文件内
    // disable_encryption 的保留原位，必在编排点之后。
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

/// 分平台门收在 start_triggers 单点（ADR-0098 决策 4）：`#[cfg(desktop)]` 在
/// 触发编排源码恰一处、贴在 start_triggers 内的调度线程拉起上、打开即同步
/// （sync_on_start）在门外全平台无条件跑——门被搬回调用点、复制多份或打开即
/// 同步从单点消失（#863 首两版各漏过门位与起点的真实回归形态）在此即红。
/// 域本体已随 #1107 迁入 `ledger-sync-engine`，扫描目标随之改路径，断言不变。
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
         调用上，sync_on_start（打开即同步，Android 兑底语义）在门外全平台跑"
    );
}
