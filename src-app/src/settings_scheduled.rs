//! 定时偏好卡（issue #2136 设置③；旧壳 `ScheduledSettings.vue` +
//! `DevicePreferenceSyncHost` 推送镜像的重生，issue #308 / ADR-0042）。
//!
//! 设备级「自动执行」开关：真源在本机平台存储（键 `auto_execution_enabled`，
//! 键值形态同旧壳 localStorage JSON 布尔，不与旧壳共享、不进 SQLite、不动
//! schema——外观卡同款契约），后端运行时镜像（[`ledger_scheduled::auto_run::
//! set_enabled`]）由本面在启动与变更时推送——`wiring.rs` 旧注「镜像默认关
//! （NSH 现无自动执行开关推送面）：未推送即空转」随本卡翻转：NSH 有了可开
//! 的推送面，追补判定随调度轮读镜像执行。开关只写偏好与推送镜像，不改写任
//! 何清单状态（关闭只停执行，计划数据不动——旧壳同口径）。
//!
//! 启动推送（旧壳 DevicePreferenceSyncHost immediate 推送同位）：[`apply_
//! startup_auto_execution`] 在 App 根组件装载期单点调用——存储读 → 镜像推送
//! 三环接缝任一拆除即红（headless「已开启偏好跨启动生效」探针）。
//!
//! 界面文案仅 zh-CN（旧壳 settings.scheduled.* zh-CN 词条原样承载）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_core::prelude::*;
use nsh_core::storage::Storage;

use crate::design::font_size;
use crate::settings::card;
use crate::views;

// ---------------------------------------------------------------------------
// 词表（旧壳 settings.scheduled.* zh-CN 词条原样；字面承载）。
// ---------------------------------------------------------------------------
const CARD_TITLE: &str = "自动执行";
const SWITCH_LABEL: &str = "开启后自动落账到期期次";
const BODY1: &str = "开启后，应用运行期间每 10 分钟检查一次，把已到期的待执行期次自动落账，交易日期忠实取期次计划日期；失败期次保持手动重试，暂停或取消的计划不会被碰。自动执行只应在一台机器开启。";
const BODY2: &str = "开关属本机设备偏好：不随备份或恢复迁移，换新机器或恢复备份后保持默认关闭。";

/// 存储键（旧壳 store `auto_execution_enabled` 同键字面；NSH 自建平台存储）。
const AUTO_EXECUTION_KEY: &str = "auto_execution_enabled";

/// 读自动执行偏好（缺失 / 损坏 → 关——出厂默认关，镜像默认关同口径）。
/// 启动推送与本卡初值的同一取值单点。
pub(crate) fn auto_execution_pref() -> bool {
    Storage::get_item(AUTO_EXECUTION_KEY)
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// 写自动执行偏好 + 推送运行时镜像（写与推送一体：推送命令的唯一写点与
/// 偏好存储同函数收口——「写偏好但漏推送」的双写漂移在形状上不可达）。
/// 写失败不静默（告警 + 会话内镜像仍生效，下次启动回关——外观卡同取舍）。
pub(crate) fn push_auto_execution(enabled: bool) {
    let value = serde_json::json!(enabled);
    if let Err(e) = Storage::set_item(AUTO_EXECUTION_KEY, value.to_string()) {
        tracing::warn!(key = AUTO_EXECUTION_KEY, error = %e, "自动执行偏好写入存储失败");
    }
    ledger_scheduled::auto_run::set_enabled(enabled);
}

/// 自动执行启动推送（App 根组件装载期单点调用，[`crate::app::App`]）：读存储
/// 偏好推送运行时镜像。删本调用（或调用点接线）→ 已开启偏好跨启动失效
///（镜像回默认关）→ headless「已开启偏好跨启动生效」探针红。
#[doc(hidden)]
pub fn apply_startup_auto_execution() {
    let enabled = auto_execution_pref();
    ledger_scheduled::auto_run::set_enabled(enabled);
    tracing::info!(enabled, "自动执行偏好启动推送完成");
}

// ---------------------------------------------------------------------------
// 视图（设置页「定时」页签消费面）。
// ---------------------------------------------------------------------------

/// 定时页签内容（旧壳 ScheduledSettings 单卡同构：开关 + 标签 + 两段说明）。
pub(crate) fn scheduled_pane(palette: views::ViewPalette) -> View {
    let checked = signal(auto_execution_pref());
    // 无障碍名随动（「开启/关闭 + 开关标签」——功能开关行先例同形）。
    let switch_label: &'static str =
        Box::leak(format!("{}{}", SWITCH_LABEL, " 开关").into_boxed_str());
    // 单一执行链（code-review：功能开关行先例同形——双按压面同源不另立第二
    // 笔写路径）：置镜像信号 + 写偏好推送只此一处，开关本体的 on_change 与
    // 行按压替身（headless id 锚点）同源调用。按压命中面恒是内层单点（
    // nsh-base switch 的 on_press 只在开关本体；行替身只在行面命中——一次
    // 点击只会达其中一面），但两面各自内联一份写链在形状上是双写漂移，
    // 收口为同一执行体。
    let apply = move |next: bool| {
        checked.set(next);
        // 写偏好 + 推送镜像一体（函数注释同口径）；删推送调用
        // → 镜像不翻转 → 期次落账旅程红（headless 探针）。
        push_auto_execution(next);
    };
    let row = view()
        .flex_row()
        .app_class("items-center gap-3")
        .child(
            nsh_base::switch()
                .accessibility_label(switch_label)
                .checked(move || checked.get())
                .on_change(move |value: &bool| apply(*value))
                .app_class("shrink-0"),
        )
        .child(
            text(SWITCH_LABEL)
                .text_size(px(font_size::SECONDARY))
                .text_color(palette.fg)
                .test_id("scheduled-auto-label"),
        );
    // 行按压替身（headless id 锚点 → 同一写链的执行体；删替身接线即红——
    // 特接 inline press 页面行为，功能开关行先例同形）。行替身不内联写链，
    // 与开关 on_change 同走上方单一执行体。
    card(
        CARD_TITLE,
        palette,
        vec![
            view()
                .test_id("scheduled-auto-toggle")
                .role(nsh_core::AccessibilityRole::Button)
                .optional_label(move || Some(SharedString::from(SWITCH_LABEL)))
                .app_class("rounded-(--radius)")
                .on_press(move |_: &PressEvent| apply(!checked.get_untracked()))
                .child(row),
            view().child(
                text(BODY1)
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.muted),
            ),
            view().child(
                text(BODY2)
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.muted),
            ),
        ],
    )
}
