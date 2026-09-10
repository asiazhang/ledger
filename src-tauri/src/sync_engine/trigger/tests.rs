//! 触发编排域单测（issue #863 / ADR-0091 决策 9）：通道配置单点、轮次编排与
//! 自动触发的零动作语义、会话信封形态——域行为的唯一断言权威层（ADR-0087）。
//!
//! 轮次协议与双端语义归 [`super::super::tests::channel`] 的既有套件；本套件只
//! 钉触发侧新增的事实：配置读取/构库单点、未配置即零动作（不触网、不落成功
//! 时刻）、一轮成功即落成功时刻、会话记忆决定信封形态。

use crate::settings::{self, SettingKey};
use crate::sync_engine::EnvelopeMode;
use crate::sync_engine::SyncChannelConfig;
use crate::sync_engine::tests::common::make_expense;
use crate::sync_engine::transport::Transport;
use crate::sync_engine::trigger::{
    SessionEnvelope, build_channel, configured_channel, run_auto_round, run_round_once,
};
use crate::test_support::{self, seed_account};
use crate::transaction::behavior;

/// 通道配置单点：未配置回 `None`；保存后读回同值。
#[test]
fn channel_config_roundtrip_through_settings_single_point() {
    let conn = test_support::open();
    assert!(
        configured_channel(&conn).unwrap().is_none(),
        "新库未配置通道"
    );

    let config = SyncChannelConfig {
        base_url: "http://127.0.0.1:9/dav/".into(),
        username: "alice".into(),
        password: "app-pass".into(),
        space_id: "family".into(),
    };
    settings::set(&conn, SettingKey::SyncChannelConfig, &config).unwrap();
    assert_eq!(
        configured_channel(&conn).unwrap().as_ref(),
        Some(&config),
        "读回经同一单点，值与写入一致"
    );
}

/// 空间字段缺省回 `default`（旧配置升级路径）：反序列化缺字段不报错。
#[test]
fn missing_space_id_deserializes_to_default() {
    let config: SyncChannelConfig =
        serde_json::from_str(r#"{"base_url":"u","username":"n","password":"p"}"#).unwrap();
    assert_eq!(config.space_id, "default");
}

/// 构库单点：非法空间（清洗后为空）报码化错误；保存路径与本单点同源。
#[test]
fn build_channel_rejects_invalid_space_with_coded_error() {
    let config = SyncChannelConfig {
        base_url: "http://127.0.0.1:9/dav/".into(),
        username: "u".into(),
        password: "p".into(),
        space_id: "///".into(),
    };
    // `SyncChannel` 不实现 `Debug`（内含 reqwest 客户端），故不用 `unwrap_err`：
    // 经 match 取错误，语义等价且不引入 Debug 约束。
    let err = match build_channel(&config) {
        Ok(_) => panic!("非法空间应被拒"),
        Err(e) => e,
    };
    assert!(err.is_code("sync-channel.book-id-invalid"), "实际: {err:?}");
}

/// 会话信封形态（ADR-0098）：无会话口令 → 明文；有 → 密文并以之封包。
/// 「自动轮询不读钥匙串」由本单点保证（读会话记忆，不读钥匙串）。
#[test]
fn session_envelope_follows_session_passphrase() {
    SessionEnvelope::forget();
    assert_eq!(
        SessionEnvelope::current(),
        SessionEnvelope::Plaintext,
        "无会话记忆 = 明文形态（明文库的正常形态）"
    );

    SessionEnvelope::remember(SessionEnvelope::Encrypted("master-pass".into()));
    assert_eq!(
        SessionEnvelope::current(),
        SessionEnvelope::Encrypted("master-pass".into())
    );
    assert!(matches!(
        SessionEnvelope::current().mode(),
        EnvelopeMode::Encrypted { passphrase } if passphrase == "master-pass"
    ));

    // 换库清空（原位重引导 / 忘记口令重置 / 关闭加密）：新库形态未知，回明文
    // 形态等下一次解锁或手动同步重新记入——避免拿旧库口令去封新库的段。
    SessionEnvelope::forget();
    assert_eq!(SessionEnvelope::current(), SessionEnvelope::Plaintext);
    assert_eq!(
        SessionEnvelope::Plaintext.mode(),
        EnvelopeMode::Plaintext,
        "明文形态对应的信封模式是明文直通"
    );
}

/// 自动轮次：通道未配置时零动作（`None`，不触网、不落成功时刻）。
#[test]
fn auto_round_without_channel_is_a_noop() {
    let conn = test_support::open();
    let outcome = run_auto_round(&conn, &SessionEnvelope::Plaintext).unwrap();
    assert!(outcome.is_none(), "未配置通道：自动轮次零动作");
    assert_eq!(
        settings::get::<Option<String>>(&conn, SettingKey::SyncLastSyncAt, None).unwrap(),
        None,
        "零动作轮次不更新成功时刻"
    );
}

/// 轮次编排单点：一轮成功即更新「上次成功同步时刻」（手动与自动入口共用）。
#[test]
fn round_once_stamps_last_sync_on_success() {
    let stub = test_support::spawn_webdav_stub(Some(("alice", "app-pass")));
    let conn = test_support::open();
    seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);
    behavior::create(&conn, make_expense("acc-1", 10000, "午饭")).unwrap();
    let config = SyncChannelConfig {
        base_url: stub.base_url.clone(),
        username: "alice".into(),
        password: "app-pass".into(),
        space_id: "default".into(),
    };
    settings::set(&conn, SettingKey::SyncChannelConfig, &config).unwrap();

    let channel = build_channel(&config).unwrap();
    let report = run_round_once(&conn, &channel, &EnvelopeMode::Plaintext).unwrap();
    assert!(report.uploaded_ops >= 1, "本轮应上传本机 op");
    assert!(
        settings::get::<Option<String>>(&conn, SettingKey::SyncLastSyncAt, None)
            .unwrap()
            .is_some(),
        "成功轮次更新上次同步时刻"
    );
    assert!(
        channel
            .transport()
            .read_file(&channel.layout().manifest_path())
            .unwrap()
            .is_some(),
        "轮次应写出 manifest（编排确实跑了轮次协议，而非空转）"
    );
}

/// 写后触发的去抖合流（ADR-0091 决策 9「写 op 后即时入队上传」）：调用方形态
/// 是「外层 `recv_timeout` 已消费首个写信号，再进入吸干」——单次写必须跑一轮
/// （否则记完账永不上传）；窗口中途到达的写被吸干、合流进同一轮（不是每写一笔
/// 传一次）；通道断裂也不得挂死。
#[test]
fn write_after_sync_debounce_runs_one_round_per_burst() {
    let window = std::time::Duration::from_millis(80);

    // 单次写（最常见的「记一笔账」）：外层消费首个信号 → 吸干 → 本轮确实跑。
    // 返回值已取消（吸干后必然跑一轮，没有可假的分支），故断言「不挂死且无
    // 残留」：残留即第二轮空转。
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    tx.send(()).unwrap();
    assert!(
        rx.recv_timeout(window).is_ok(),
        "外层应先消费触发收尾的那个写信号"
    );
    crate::sync_engine::trigger::drain_write_signals(&rx, window);
    assert!(
        rx.try_recv().is_err(),
        "单次写不应残留信号（否则第二轮空转）"
    );

    // 窗口中途到达的写：被吸干、合流进同一轮（去抖窗口真实生效），不残留。
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    tx.send(()).unwrap();
    assert!(rx.recv_timeout(window).is_ok(), "外层先消费首个写信号");
    let late = tx.clone();
    let sender = std::thread::spawn(move || {
        std::thread::sleep(window / 2);
        late.send(()).expect("中途写应可投递");
    });
    crate::sync_engine::trigger::drain_write_signals(&rx, window);
    sender.join().expect("投递线程应结束");
    assert!(
        rx.try_recv().is_err(),
        "窗口中途的写应被吸干合流（不触发第二轮）"
    );

    // 通道断裂（发送端全部丢弃）：吸干不得挂死、不得 panic，照常返回跑一轮。
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    tx.send(()).unwrap();
    assert!(rx.recv_timeout(window).is_ok(), "外层先消费首个写信号");
    drop(tx);
    crate::sync_engine::trigger::drain_write_signals(&rx, window);
}

/// 写后触发在「调度未拉起」时是零动作（写路径对同步域无感）：投递不 panic、
/// 不触库、不触网——单测与未配置同步的进程都走这一形态。
#[test]
fn write_after_sync_without_scheduler_is_a_noop() {
    crate::sync_engine::sync_after_write();
}

/// 写信号投递侧（与吸干侧对称的参数接缝）：通道在位即投递一次；未装通道
/// 零动作。两侧共用显式通道参数，不依赖进程级单例、不需测试后门。
#[test]
fn write_signal_is_delivered_into_the_channel() {
    use std::sync::OnceLock;
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let slot: OnceLock<std::sync::mpsc::Sender<()>> = OnceLock::new();
    assert!(slot.set(tx).is_ok());

    crate::sync_engine::trigger::notify_write_signal(&slot);
    assert!(
        rx.try_recv().is_ok(),
        "通道在位：本地产出 op 应投递一次写信号"
    );
    assert!(rx.try_recv().is_err(), "一次产出一次信号（不重复投递）");

    // 未装通道（未拉起调度 / 单测环境）：零动作，不 panic。
    let empty: OnceLock<std::sync::mpsc::Sender<()>> = OnceLock::new();
    crate::sync_engine::trigger::notify_write_signal(&empty);
}

// ---------------------------------------------------------------------------
// 触发接线的源码扫描守门（issue #959 / ADR-0098 决策 4；`signals_cross_check`
// 先例的文本级核对，仅测试可见）：触发编排的「接线」行为半边由 mock 应用集成
// 测试（`tests/commands/sync_trigger.rs`）钉住，但「分平台门收在 start_triggers
// 单点、三业务可用起点统一调 start_triggers」是源码形状事实——删掉 lib.rs 的
// start_triggers 调用或把门搬回调用点，行为测试无从得知（集成测试不运行
// setup），只有扫描即红。
// ---------------------------------------------------------------------------

use crate::signals_cross_check::mask_non_code;
use std::path::Path;

/// `src/**` 下全部 .rs 的（仓库相对路径, 源文本），按路径排序（输出确定）。
fn all_rust_sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("扫描目录不可读 {dir:?}: {e}"))
            .flatten()
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
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
    let mut out = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    out
}

/// 生产文本：掩码注释与字符串后，逐个剔除 `#[cfg(test)]` 附属的模块/函数体
/// ——测试模块内合法直呼触发入口不参与守门。剔除在**掩码文本**上做花括号
/// 配对（字符串与注释已空白化，配对可靠；lib.rs 的 cfg(test) 模块声明在文件
/// 前部，不能按「首个锚点截到文件尾」——会误伤其后的 setup 段，issue #959
/// 守门首版踩过）。掩码与剔除全程保长，切片下标同位。
fn production_text(src: &str) -> String {
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

/// 触发编排单一入口被全部业务可用起点调用（ADR-0098 决策 4）：反向核对「域外
/// 不得直呼两个内部触发入口」（绕开单点即红），正向核对「start_triggers 调用方
/// 恰为三起点 + 定义点」——删掉任一起点的 start_triggers 调用（#863 第二版曾
/// 漏 restart_app 落 Ready 这个起点），或新增调用点未登记，在此即红。
#[test]
fn sync_triggers_start_from_every_business_surface_via_single_entry() {
    let sources = all_rust_sources();

    for (path, src) in &sources {
        if path.starts_with("src/sync_engine/") {
            continue;
        }
        let text = production_text(src);
        for entry in ["start_sync_scheduler(", "sync_on_start("] {
            assert!(
                !text.contains(entry),
                "{path} 直呼触发内部入口 `{entry}`——分平台门与触发时机编排只收在 \
                 start_triggers 单点（ADR-0098 决策 4），业务可用起点应调 start_triggers"
            );
        }
        // 调用点贴身窗口（60 字符）不得出现平台门属性/宏：把 start_triggers
        // 调用包进 #[cfg(desktop)]（移动端「打开即同步」静默失效，ADR-0098 决策
        // 4 的「加门」回归形态）是文本可查的。窗口贴身即不含远处合法的 desktop
        // 门（lib.rs 的 api_server 等）；文本级扫描的其余盲区（间接别名、远距
        // 包装）按 signals_cross_check 同款纪律靠评审兜底。
        let mut search_from = 0usize;
        while let Some(rel) = text[search_from..].find("start_triggers(") {
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
                    "{path} 的 start_triggers 调用贴身出现平台门 `{gate}`——调用点不得 \
                     分平台包装（门只收在 start_triggers 单点，移动端只保留打开即同步）"
                );
            }
            search_from = call_pos + "start_triggers(".len();
        }
    }

    let mut callers: Vec<&str> = sources
        .iter()
        .filter(|(_, src)| production_text(src).contains("start_triggers("))
        .map(|(path, _)| path.as_str())
        .collect();
    callers.sort_unstable();
    // 定义点不在本清单：`start_triggers` 的定义随泛型化带 `<R: Runtime>` 参数
    // 列表，不含裸 `start_triggers(` 子串；定义点自身（单点形状）由
    // `sync_desktop_gate_is_a_single_point_in_start_triggers_body` 钉住。
    let mut expected = [
        "src/commands/boot.rs",
        "src/commands/encryption.rs",
        "src/lib.rs",
    ];
    expected.sort_unstable();
    assert_eq!(
        callers, expected,
        "start_triggers 调用方漂移——三业务可用起点（setup 就绪 / \
         resume_business_surface / restart_app 落 Ready）缺一不可，新增起点须同步登记本守门\
         （ADR-0098 决策 4：起点不在本清单即「打开即同步」本会话不生效）"
    );
}

/// 分平台门收在 start_triggers 单点（ADR-0098 决策 4）：`#[cfg(desktop)]` 在
/// 触发编排源码恰一处、贴在 start_triggers 内的调度线程拉起上、打开即同步
/// （sync_on_start）在门外全平台无条件跑——门被搬回调用点、复制多份或打开即
/// 同步从单点消失（#863 首两版各漏过门位与起点的真实回归形态）在此即红。
#[test]
fn sync_desktop_gate_is_a_single_point_in_start_triggers_body() {
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/sync_engine/trigger.rs"),
    )
    .expect("trigger.rs 应可读");
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
