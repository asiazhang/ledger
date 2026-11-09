//! 设置（issue #2053 NSH 迁移㉒；旧壳 `SettingsView.vue` 页签分域 +
//! `settings/GeneralSettings|LogSettings|DataLocationSettings|FeatureToggleSettings`
//! 的重生，ADR-0022 页签分域 / ADR-0017 设置落点 / ADR-0116 功能开关）。
//!
//! **页签容器**：旧壳设置页的分域页签（通用 → 分类 → 币种 → 数据 → 定时 →
//! 功能 → 关于，ADR-0022 修订链的完整结构）在 NSH 以 visited-once
//! keep-mounted 页签容器单点重建——旧壳 display-directive="show:lazy" + 显式
//! key 的「带 key 实践」对应物：已到访页签走 display 隐藏保持挂载（切页签不
//! 重挂重拉），未到访页签不预载（show:lazy 的 lazy 半边）。页签选中态会话内
//! 有效、不跨启动持久化，离开设置页再回默认首签（旧壳顶级页签无 v-model 同
//! 口径）。「定时」页签随功能开关响应式隐藏（旧壳 v-if 设置面联动，#1243 /
//! ADR-0116 决策 4；关闭只隐藏入口、不改写任何清单状态）。容器机制单点，
//! 设置②（#2054）的数据安全四卡复用本容器的页签面与卡片宿主。
//!
//! **本票四卡**（设置② 数据安全四卡已随 #2054 落位，卡组住
//! [`crate::settings_data`]）：
//! - **外观**（GeneralSettings 主题卡）：显示主题三态（跟随系统 / 深色 /
//!   浅色——跟随系统是 NSH 框架能力的显式开口，旧壳 Appearance 为 dark/light
//!   二值；默认深色与旧壳出厂同口径）。设备级轻量设置项（ADR-0017）：NSH 自
//!   建平台存储（键 `appearance`，不与旧壳共享、不进 SQLite、不动 schema），
//!   改动经 [`nsh_core::element::set_color_scheme`] 立即生效、跨启动记住。
//! - **日志**（LogSettings）：档位五档下拉 + 排障提示；读写直调基建 settings
//!   共享接缝（boot 单点），闭集外拒绝码 `settings.log-level-invalid`（旧壳
//!   同码同文案），运行期滤镜接管即档位立即生效；旧壳「打开日志目录」动作在
//!   本壳无对应物（NSH stderr 为日志出口、无文件日志面）——偏差见 #2053 交付
//!   报告。旧壳语言卡（i18n 界外）与检查更新卡（切轨退役）同属偏差面。
//! - **数据存储位置**（DataLocationSettings）：查看生效位置 / 待重启提示 /
//!   回退警示 / 复制路径；「更改位置 / 恢复默认」直调基础设施引导层三步校验
//!   与意图落盘（旧壳二选一确认的 NSH 对应 = danger warning 级「接管已有账
//!   本库？」确认对话框，ADR-0078 warning 分级的首个壳内消费面），真实搬迁在
//!   下次启动生效（本票引导接 [`ledger_infra::db::boot::plan_boot`] 消费账本
//!   注册表，下次启动如实兑现意图）。
//! - **功能开关**（FeatureToggleSettings）：可关九项各一行 = 功能名 + 一句简
//!   介 + 开关；写路径归 [`crate::nav::set_feature_closed`] 单点（与守卫 /
//!   侧栏 / 容器页签 / 本页页签可见性共源）；关闭只隐藏入口的知情权由静态简
//!   介文案承担，不设确认与拦截（ADR-0116 决策 1/8）。
//!
//! 界面文案仅 zh-CN（旧壳 settings.* zh-CN 词表原样承载）；码化错误 zh 原样
//! 呈现；NSH 无 toast，动作反馈落卡内状态条位（#2038 裁决）；视图本地快照、
//! 离开再回 = 全新装载（旧壳 onMounted 同口径）；阻塞 IO（档位读写、位置信
//! 息聚合与意图提交）经 `blocking` 后台执行。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_core::element::{FontWeight, pick_paths, set_clipboard, set_color_scheme};
use nsh_core::file_dialog::OpenDialog;
use nsh_core::prelude::*;
use nsh_core::storage::Storage;

use crate::app::book_dir;
use crate::assembly::CategoryOption;
use crate::danger::{DangerConfirmSpec, DangerLevel, DangerOutcome, danger_confirm};
use crate::{boot, nav, nav_model, select, views};

// ---------------------------------------------------------------------------
// 词表（旧壳 settings.* zh-CN 词条原样；i18n 框架级方案不在本票，字面承载）。
// ---------------------------------------------------------------------------
const VIEW_TITLE: &str = "设置";
const TAB_CATEGORIES: &str = "分类";
const TAB_CURRENCIES: &str = "币种";
const TAB_SCHEDULED: &str = "定时";
const TAB_ABOUT: &str = "关于";
const CARD_APPEARANCE: &str = "主题模式";
const APPEARANCE_LABEL: &str = "显示主题";
const APPEARANCE_OPTION_SYSTEM: &str = "跟随系统";
const APPEARANCE_OPTION_DARK: &str = "深色";
const APPEARANCE_OPTION_LIGHT: &str = "浅色";
const APPEARANCE_PLACEHOLDER: &str = "选择显色主题";
const CARD_LOG: &str = "日志";
const LOG_LABEL: &str = "日志等级";
const LOG_HINT: &str = "默认 info；切换后立即生效并跨启动保留。若本次启动设置了 RUST_LOG 环境变量，则启动时以它为准；此处展示的是持久化档位，手动切换会覆盖它。";
const LOG_PLACEHOLDER: &str = "选择日志等级";
const LOG_LOAD_PREFIX: &str = "读取日志等级失败：";
const LOG_SAVE_PREFIX: &str = "设置日志等级失败: ";
const LOG_LEVEL_ERROR: &str = "错误 (error)";
const LOG_LEVEL_WARN: &str = "警告 (warn)";
const LOG_LEVEL_INFO: &str = "信息 (info)";
const LOG_LEVEL_DEBUG: &str = "调试 (debug)";
const LOG_LEVEL_TRACE: &str = "追踪 (trace)";
const CARD_LOCATION: &str = "数据存储位置";
const LOCATION_HINT: &str = "当前账本的数据库所在目录。更改位置后，应用将在下次启动时把当前账本的数据库完整搬迁到新位置，原位置库文件永久保留作为安全网；其他账本不受影响，恢复默认走同一流程。";
const LOCATION_FALLBACK_TITLE: &str = "已回退到默认位置";
const LOCATION_FALLBACK_PREFIX: &str = "配置的位置不可用：";
const LOCATION_FALLBACK_SUFFIX: &str =
    "。已自动回退到默认位置，原库仍在原地未动，可重新更改位置或保持现状。";
const LOCATION_PENDING_TITLE: &str = "已更改，待重启生效";
const LOCATION_PENDING_PREFIX: &str = "新位置：";
const LOCATION_PENDING_SUFFIX: &str = "。将在下次启动时自动搬迁生效，在此之前数据仍写入当前位置。";
const LOCATION_ACTIVE_LABEL: &str = "当前生效位置：";
const LOCATION_READING: &str = "读取中…";
const BUTTON_RETRY: &str = "重试";
const BUTTON_COPY_PATH: &str = "复制路径";
const BUTTON_CHANGE: &str = "更改…";
const BUTTON_RESTORE_DEFAULT: &str = "恢复默认";
const PICK_LOCATION_TITLE: &str = "选择数据存储位置";
const MSG_COPIED: &str = "已复制完整路径";
const MSG_COMMITTED: &str = "更改已保存，将在下次启动时搬迁生效";
const MSG_CANCELLED: &str = "已取消更改，数据存储位置未变化";
const MSG_CHANGE_PREFIX: &str = "更改数据存储位置失败: ";
const MSG_LOAD_PREFIX: &str = "读取数据存储位置失败：";
const ADOPT_TITLE: &str = "接管已有账本库？";
const ADOPT_LEAD: &str = "目标目录已存在同名账本库（ledger.db）。";
const ADOPT_STRONG: &str = "接管则把该库作为活动数据；取消换位则放弃本次更改（不做任何改动）。";
const ADOPT_DETAIL: &str = "接管后原位置库文件仍会保留。";
const ADOPT_CONFIRM: &str = "接管该库";
const ADOPT_CANCEL: &str = "取消换位";
const CARD_FEATURES: &str = "功能开关";
const FEATURES_INTRO: &str =
    "关闭只隐藏入口：数据不删，统计口径与后台行为照旧运行，随时可以重新打开。";
const SESSION_MISSING: &str = "内部错误：账本库会话缺失";
const READ_TASK_FAILED: &str = "内部错误：读任务异常退出";
const WRITE_TASK_FAILED: &str = "内部错误：写入任务异常退出";

/// 功能简介词表（旧壳 settings.features.descriptions.<id> 原文；键 = 可关九
/// 项，与 [`nav_model::CLOSABLE_FEATURES`] 同集；不可关项不出现——旧壳同口径
/// 也不出现空行）。
fn feature_description(view: nav_model::ViewName) -> &'static str {
    use nav_model::ViewName as V;
    match view {
        V::Budget => "关闭只隐藏入口：预算数据与进度口径照旧，不受影响。",
        V::Reports => "关闭只隐藏入口：报表仍基于全部数据生成，统计口径不变。",
        V::Scheduled => "关闭只隐藏入口：自动执行照旧，已有计划仍按既有设置照常落账。",
        V::Merchants => "关闭只隐藏入口：交易里的商户信息与商户统计照旧显示。",
        V::Investments => "关闭只隐藏入口：持仓、收益与净资产口径照旧计入。",
        V::Items => "关闭只隐藏入口：物品记录照旧保留，使用成本口径不变。",
        V::Policies => "关闭只隐藏入口：保单档案照旧保留，保费流水仍计入统计。",
        V::PhysicalAssets => "关闭只隐藏入口：估值档案照旧计入净资产。",
        V::Insurers => "关闭只隐藏入口：保司字典照旧可用，已有保单不受影响。",
        _ => "",
    }
}
// ---------------------------------------------------------------------------
// 外观三态（持久化闭集 = system / dark / light；默认深色与旧壳出厂同口径）。
// ---------------------------------------------------------------------------

/// 外观态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Apo {
    /// 跟随系统。
    System,
    /// 永远深色。
    Dark,
    /// 永远浅色。
    Light,
}

impl Apo {
    /// 持久化名。
    pub fn as_str(self) -> &'static str {
        match self {
            Apo::System => "system",
            Apo::Dark => "dark",
            Apo::Light => "light",
        }
    }
}

/// 持久化名 → 外观态（非字符串/未知名 None，调用方回出厂）。
pub fn apo_parse(raw: &serde_json::Value) -> Option<Apo> {
    match raw.as_str() {
        Some("system") => Some(Apo::System),
        Some("dark") => Some(Apo::Dark),
        Some("light") => Some(Apo::Light),
        _ => None,
    }
}

/// 出厂外观（跟随系统——立桩验收基线是系统随动场景，本壳出厂不改判；旧壳
/// 出厂深色与本壳出厂跟随系统的偏差见 #2053 交付报告——出厂值是初装面，平
/// 价判据是「用户设置后跨启动保持」，不是出厂比对）。
pub const DEFAULT_APO: Apo = Apo::System;

/// 外观存储键（设备级轻量设置项；NSH 自建平台存储，不与旧壳共享，键值形态
/// 同旧壳 appearance 键的字面习惯）。
const APPEARANCE_HINT: &str =
    "出厂跟随系统（切系统外观即随动）；深色/浅色为显式强指、改动立即生效并跨启动记住。";
const APPEARANCE_KEY: &str = "appearance";

/// 读存储外观（缺失/损坏 → [`DEFAULT_APO`]）——显色启动钩与设置卡初值的同
/// 一取值单点。
pub(crate) fn storage_apo() -> Apo {
    Storage::get_item(APPEARANCE_KEY)
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|value| apo_parse(&value))
        .unwrap_or(DEFAULT_APO)
}

/// 写存储外观（写失败不静默：告警 + 会话内仍生效，下次启动回默认——与导航
/// 界面状态写入同一取舍）。
fn store_apo(apo: Apo) {
    let value = serde_json::json!(apo.as_str());
    if let Err(e) = Storage::set_item(APPEARANCE_KEY, value.to_string()) {
        tracing::warn!(key = APPEARANCE_KEY, error = %e, "外观设置写入存储失败");
    }
}

/// 把外观态应用到全局显示（框架 set_color_scheme 单点：跟随系统交系统、深
/// 浅色强指）。启动钩与设置卡改动双消费。
fn apply_apo(apo: Apo) {
    use nsh_core::platform::Appearance;
    let appearance = match apo {
        Apo::System => Appearance::System,
        Apo::Dark => Appearance::Dark,
        Apo::Light => Appearance::Light,
    };
    set_color_scheme(appearance);
}

/// 显色启动钩（`App` 组件体调用一次）：按存储外观应用显色。三环接缝（存储
/// 读 / 解析 / 应用）任一拆除即红——headless「外观跟随已存选择」探针见 lib
/// tests。
#[doc(hidden)]
pub fn apply_startup_appearance() {
    apply_apo(storage_apo());
}
// ---------------------------------------------------------------------------
// 装配层（纯函数与纯投影，无 UI 依赖；渲染期零格式化——路径 / 档位原文即展
// 示文本）。选项闭集、数据位置展示投影、页签可见序都是独立可测面。
// ---------------------------------------------------------------------------

/// 外观选项闭集（下拉数据源；值 = 持久化名闭集）。
pub(crate) fn appearance_options() -> Vec<CategoryOption> {
    [
        (Apo::System, APPEARANCE_OPTION_SYSTEM),
        (Apo::Dark, APPEARANCE_OPTION_DARK),
        (Apo::Light, APPEARANCE_OPTION_LIGHT),
    ]
    .into_iter()
    .map(|(apo, label)| CategoryOption {
        id: apo.as_str().to_owned(),
        label: label.to_owned(),
    })
    .collect()
}

/// 日志档位选项（五档闭集；值 = 档位指令串〔基建值域闭集原值〕，标签
/// zh-CN）。
pub(crate) fn log_level_options() -> Vec<CategoryOption> {
    [
        ("error", LOG_LEVEL_ERROR),
        ("warn", LOG_LEVEL_WARN),
        ("info", LOG_LEVEL_INFO),
        ("debug", LOG_LEVEL_DEBUG),
        ("trace", LOG_LEVEL_TRACE),
    ]
    .into_iter()
    .map(|(id, label)| CategoryOption {
        id: id.to_owned(),
        label: label.to_owned(),
    })
    .collect()
}

/// DataLocation 卡展示投影（旧壳 DataLocationSettings.vue 信息块派生逻辑的
/// 提纯）：生效目录原文回显、待重启横幅（意图目录随带）、回退警示横幅与
/// 「恢复默认」可用性（契约不变量：configured_dir 非空 ⇔ 存在自定义位置意
/// 图——意图缺席时「恢复默认」无意义故禁用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LocationView {
    /// 当前生效目录原文（完整路径）。
    pub active_dir: String,
    /// 待重启横幅文案（Some = 横幅在屏）。
    pub pending: Option<String>,
    /// 回退警示横幅文案（Some = 横幅在屏）。
    pub fallback: Option<String>,
    /// 「恢复默认」禁用与否。
    pub restore_disabled: bool,
}

/// 数据位置卡展示投影单点（入参 = 域层聚合原样结构）。
pub(crate) fn location_view(
    info: &ledger_infra::db::data_location::DataLocationInfo,
) -> LocationView {
    let pending = info.pending_restart.then(|| {
        format!(
            "{LOCATION_PENDING_PREFIX}{}{LOCATION_PENDING_SUFFIX}",
            info.configured_dir.as_deref().unwrap_or_default(),
        )
    });
    let fallback = info
        .fallback_reason
        .as_ref()
        .map(|reason| format!("{LOCATION_FALLBACK_PREFIX}{reason}{LOCATION_FALLBACK_SUFFIX}"));
    LocationView {
        active_dir: info.active_dir.clone(),
        pending,
        fallback,
        restore_disabled: info.configured_dir.is_none(),
    }
}

/// 设置页页签（旧壳分域页签闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    /// 通用（外观卡 + 日志卡）。
    General,
    /// 分类（未迁占位）。
    Categories,
    /// 币种（未迁占位）。
    Currencies,
    /// 数据（存储位置卡 + #2054 数据安全四卡，卡组住 [`crate::settings_data`]）。
    Data,
    /// 定时（功能开关关闭即页签整行隐藏，旧壳 v-if 联动）。
    Scheduled,
    /// 功能（功能开关管理面）。
    Features,
    /// 关于（未迁占位）。
    About,
}

/// 页签闭集序（页面呈现序 = 旧壳分域序）。
const TABS: [SettingsTab; 7] = [
    SettingsTab::General,
    SettingsTab::Categories,
    SettingsTab::Currencies,
    SettingsTab::Data,
    SettingsTab::Scheduled,
    SettingsTab::Features,
    SettingsTab::About,
];

impl SettingsTab {
    /// zh-CN 屏名（页签字面）。
    fn label(self) -> &'static str {
        match self {
            SettingsTab::General => "通用",
            SettingsTab::Categories => TAB_CATEGORIES,
            SettingsTab::Currencies => TAB_CURRENCIES,
            SettingsTab::Data => "数据",
            SettingsTab::Scheduled => TAB_SCHEDULED,
            SettingsTab::Features => "功能",
            SettingsTab::About => TAB_ABOUT,
        }
    }

    /// headless id 锚点。
    fn id_str(self) -> &'static str {
        match self {
            SettingsTab::General => "set-tab-general",
            SettingsTab::Categories => "set-tab-categories",
            SettingsTab::Currencies => "set-tab-currencies",
            SettingsTab::Data => "set-tab-data",
            SettingsTab::Scheduled => "set-tab-scheduled",
            SettingsTab::Features => "set-tab-features",
            SettingsTab::About => "set-tab-about",
        }
    }
}

/// 页签可见序（纯决策半边）：「定时」页签在定时功能关闭时隐藏（旧壳
/// SettingsView v-if 设置面联动同语义——关闭只隐藏入口），其余恒在。
pub(crate) fn visible_tabs(closed: &[nav_model::ViewName]) -> Vec<SettingsTab> {
    TABS.into_iter()
        .filter(|tab| {
            tab != &SettingsTab::Scheduled || !closed.contains(&nav_model::ViewName::Scheduled)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 卡片宿主与状态条位（描边圆角容器：旧壳 NCard size small 的近形；全部设置
// 卡共用，#2054 数据安全四卡复用）。
// ---------------------------------------------------------------------------

/// 卡容器（标题 + 竖排内容行）。pub(crate)：#2054 数据安全四卡（
/// [`crate::settings_data`]）复用同一卡片宿主形态，单点不重建。
pub(crate) fn card(title: &'static str, palette: views::ViewPalette, rows: Vec<View>) -> View {
    let mut body = view().flex_col().app_class("gap-2");
    for row in rows {
        body = body.child(row);
    }
    view()
        .flex_col()
        .app_class("rounded-md border border-border p-3 gap-2")
        .child(
            text(title)
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.fg),
        )
        .child(body)
}

/// 状态条位（卡内单行动作反馈：旧壳 message.* 的条位对应物，#2038 裁决；
/// None = 无反馈行）。pub(crate)：#2054 数据安全四卡复用同一反馈条位。
pub(crate) fn status_line(
    id: &'static str,
    status: Signal<Option<String>>,
    palette: views::ViewPalette,
) -> impl IntoElement {
    view().child(
        when(
            move || status.get().is_some(),
            move || {
                text(move || status.get().unwrap_or_default())
                    .text_size(px(12.0))
                    .text_color(palette.muted)
                    .test_id(id)
                    .number_of_lines(3)
            },
        )
        .otherwise(view),
    )
}
// ---------------------------------------------------------------------------
// 设置视图（页签容器 + 卡片）。
// ---------------------------------------------------------------------------

/// 设置（导航骨架「设置」项的真实组件，#2053）：设置是无容器页签的单视图
/// ——守卫/侧栏骨架自带，页签容器本票单点重建；离开设置页 = 全宿主卸载，
/// 会话态随宿主消亡（旧壳路由卸载同口径）。
#[component]
pub fn SettingsView() -> impl IntoElement {
    let palette = views::view_palette();
    let nav = nav::store();

    // 页签会话槽：选中页签（默认首签；离开再回 = 全新装载回首签，旧壳默认
    // 态同口径）+ 到访标记（visited-once keep-mounted：先到的页签保持挂载）。
    let active: Signal<SettingsTab> = signal(SettingsTab::General);
    let visited: Signal<Vec<bool>> = signal({
        let mut flags = vec![false; TABS.len()];
        flags[0] = true; // 默认首签挂载即呈现
        flags
    });
    effect(move || {
        let current = active.get();
        if let Some(index) = TABS.iter().position(|tab| *tab == current) {
            visited.update(|flags| flags[index] = true);
        }
    });

    // 页签行（nsh-base Tabs 行为件：roving 焦点 + 无障碍页签列表；可见序随
    // 功能开关响应式——定时关闭即页签行隐）。
    let visible = computed(move || visible_tabs(&nav.closed.get()));
    let tab_bar = view()
        .px_4()
        .py_2()
        .app_class("border-b border-border")
        .child(nsh_base::tabs().indicator_class("bg-accent").child(
            view().flex_row().gap_2().child(each(
                move || visible.get(),
                |tab: &SettingsTab| tab.id_str().to_owned(),
                move |item: ReadSignal<SettingsTab>, _index: ReadSignal<usize>| {
                    let tab = item.get();
                    let is_active = move || active.get() == tab;
                    nsh_base::tab()
                        .selected(is_active)
                        .accessibility_label(tab.label())
                        .app_class("px-2 py-1 rounded-md selected:bg-accent")
                        .child(
                            text(tab.label())
                                .text_size(px(13.0))
                                .text_color(move || {
                                    if is_active() {
                                        palette.fg
                                    } else {
                                        palette.muted
                                    }
                                })
                                .test_id(tab.id_str()),
                        )
                        .on_press(move |_: &()| active.set(tab))
                },
            )),
        ));

    // 内容区（visited-once keep-mounted 页签宿主闭集；页签顺序呈现，激活切
    // 换只变 display 不重挂）。
    let content_host = view()
        .flex_1()
        .flex_col()
        .app_class("p-4 gap-3 overflow-hidden")
        .children(TABS.map(|tab| keep_mounted_pane(tab, active, visited, palette)));

    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(
            view().px_4().py_2().child(
                text(VIEW_TITLE)
                    .text_size(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.fg)
                    .test_id("view-title"),
            ),
        )
        .child(tab_bar)
        .child(content_host)
}

/// keep-mounted 页签宿主：未到访不挂载（不预载）；已到访的宿主随激活状态
/// display 切换（隐藏样式，退出布局流、不收点位）——「切页签不重挂重拉」
/// 的呈现本体。删除 display 切换接线（改回卸载重建）即红：headless
/// keep-mounted 探针见 tests/app_headless.rs（切回后已访页签的会话态随卸载
/// 丢失 → 红）。
fn keep_mounted_pane(
    tab: SettingsTab,
    active: Signal<SettingsTab>,
    visited: Signal<Vec<bool>>,
    palette: views::ViewPalette,
) -> impl IntoElement {
    let index = TABS.iter().position(|t| *t == tab).unwrap_or(0);
    when(
        move || visited.get()[index],
        move || {
            view()
                .flex_col()
                .flex_1()
                .style_when(move || active.get() != tab, hide_when_inactive)
                .child(pane_body(tab, palette))
        },
    )
    .otherwise(view)
}

/// 不活动页签宿主的隐藏样式（display none —— 旧壳「非激活 pane 不在文档流」
/// 的等效物；独立 fn 让删除映射即编译红）。
fn hide_when_inactive(
    style: nsh_core::element::StyleRefinement,
) -> nsh_core::element::StyleRefinement {
    use nsh_core::element::Styled;
    style.hidden()
}

/// 页签内容分支（闭集；未迁四页签 = 占位说明）。
fn pane_body(tab: SettingsTab, palette: views::ViewPalette) -> View {
    match tab {
        SettingsTab::General => general_pane(palette),
        SettingsTab::Data => data_pane(palette),
        SettingsTab::Features => features_pane(palette),
        SettingsTab::Categories
        | SettingsTab::Currencies
        | SettingsTab::Scheduled
        | SettingsTab::About => placeholder_pane(tab, palette),
    }
}

/// 占位页签（未迁内容明确「迁移中」反馈；分类 / 币种 / 定时管理面与关于面
/// 随各自迁移票就位——设置链后续票落点）。
fn placeholder_pane(tab: SettingsTab, palette: views::ViewPalette) -> View {
    view()
        .flex_col()
        .app_class("gap-2")
        .child(
            text(format!("{} · 迁移中", tab.label()))
                .text_size(px(13.0))
                .text_color(palette.fg),
        )
        .child(
            text("此页签内容将随迁移系列票就位")
                .text_size(px(12.0))
                .text_color(palette.muted),
        )
}
// ---------------------------------------------------------------------------
// 通用页签：外观卡 + 日志卡（旧壳 GeneralSettings 卡栈：主题卡 + 日志卡；语
// 言卡〔i18n 界外〕与检查更新卡〔切轨退役〕同属偏差面，见模块文档）。
// ---------------------------------------------------------------------------

/// 通用页签内容。
fn general_pane(palette: views::ViewPalette) -> View {
    view()
        .flex_col()
        .app_class("gap-3")
        .children(vec![appearance_card(palette), log_card(palette)])
}

/// 外观卡（三态下拉；选择 → 存储 → 全局显色，读/写/应用任一环拆除即红——
/// lib tests 与 headless 探针钉存储接缝，set_color_scheme 视觉面归人工）。
fn appearance_card(palette: views::ViewPalette) -> View {
    let selected: Signal<Option<String>> = signal(Some(storage_apo().as_str().to_owned()));
    // 写入链（选中变化的直接应用点，effect 单点接线）：存储落盘 + 全局显色。
    effect(move || {
        if let Some(value) = selected.get()
            && let Some(apo) = apo_parse(&serde_json::json!(value.as_str()))
        {
            store_apo(apo);
            apply_apo(apo);
        }
    });
    card(
        CARD_APPEARANCE,
        palette,
        vec![
            view()
                .flex_row()
                .app_class("items-center gap-2")
                .child(
                    text(APPEARANCE_LABEL)
                        .text_size(px(13.0))
                        .text_color(palette.fg),
                )
                .child(select::app_select(
                    "set-appearance",
                    appearance_options,
                    selected,
                    APPEARANCE_PLACEHOLDER,
                    false,
                )),
            view().child(
                text(APPEARANCE_HINT)
                    .text_size(px(11.0))
                    .text_color(palette.muted),
            ),
        ],
    )
}

/// 日志卡（档位下拉：读 boot 直调基建设置接缝；写入链同点；失败回显保持原
/// 档位——旧壳 handleLevelChange 同语义）。
fn log_card(palette: views::ViewPalette) -> View {
    let owner = Owner::current();
    // 会话句柄（快照一次；读写两链共享同一快照——离开再回视图重挂即重新置零）
    let db_slot = crate::app::db_slot().clone();
    // 档位信号（Some = 已装载）与已落库档位镜像（写链的去重基准：初读同写
    // 两信号，只有选点变化才触发写入）。
    let level: Signal<Option<String>> = signal(None);
    let persisted: Signal<Option<String>> = signal(None);
    let status: Signal<Option<String>> = signal(None);

    // 初读（视图本地快照，离开再回全新装载）：失败诚实呈现（旧壳 loadFailed
    // 同口径，读路径不静默假空态）。
    {
        let db_slot = db_slot.clone();
        owner.run(move || {
            spawn(async move {
                let Some(db) = db_slot else {
                    status.set(Some(SESSION_MISSING.to_owned()));
                    return;
                };
                match blocking(move || boot::log_level(&db)).await {
                    Ok(Ok(level_str)) => {
                        persisted.set(Some(level_str.clone()));
                        level.set(Some(level_str));
                        status.set(None);
                    }
                    Ok(Err(e)) => {
                        status.set(Some(format!("{LOG_LOAD_PREFIX}{e}")));
                    }
                    Err(_) => status.set(Some(READ_TASK_FAILED.to_owned())),
                }
            });
        });
    }

    // 写链（档位变化 → boot::set_log_level 单点校验与落库 + 运行期滤镜接管；
    // 失败回滚显示回原档位）。effect 只在 persisted 已就绪后比较——初读同
    // 源双写不触发，用户选点变化才进入。
    effect(move || {
        let current = level.get();
        let base = persisted.get_untracked();
        if current.is_some() && base.is_some() && current != base {
            let target = current.clone().unwrap_or_default();
            let written_target = target.clone();
            let reset = base.clone();
            let db_slot = db_slot.clone();
            owner.run(move || {
                spawn(async move {
                    let Some(db) = db_slot else {
                        level.set(reset);
                        status.set(Some(SESSION_MISSING.to_owned()));
                        return;
                    };
                    match blocking(move || boot::set_log_level(&db, &target)).await {
                        Ok(Ok(())) => {
                            persisted.set(Some(written_target.clone()));
                            level.set(Some(written_target));
                            status.set(None);
                        }
                        Ok(Err(e)) => {
                            level.set(reset);
                            status.set(Some(format!("{LOG_SAVE_PREFIX}{e}")));
                        }
                        Err(_) => {
                            level.set(reset);
                            status.set(Some(WRITE_TASK_FAILED.to_owned()));
                        }
                    }
                });
            });
        }
    });

    card(
        CARD_LOG,
        palette,
        vec![
            view()
                .flex_col()
                .app_class("items-start gap-2")
                .child(text(LOG_LABEL).text_size(px(13.0)).text_color(palette.fg))
                .child(select::app_select(
                    "log-level",
                    log_level_options,
                    level,
                    LOG_PLACEHOLDER,
                    false,
                ))
                .child(view().child(text(LOG_HINT).text_size(px(11.0)).text_color(palette.muted))),
            view().child(status_line("log-status", status, palette)),
        ],
    )
}
// ---------------------------------------------------------------------------
// 数据存储位置卡（查看 / 更改 / 恢复默认 / 二选一接管确认；流程分支与旧壳
// DataLocationSettings.vue 的 submitWithChoice / applyOutcome / confirmAdopt
// 同形）。
// ---------------------------------------------------------------------------

/// 数据页签：存储位置卡 + 设置② 数据安全四卡（备份 / 加密 / 多端同步 /
/// 账本切换；[`crate::settings_data`]——卡片组住独立模块，页签容器面单点
/// 不变）。
fn data_pane(palette: views::ViewPalette) -> View {
    view()
        .flex_col()
        .app_class("gap-3")
        .child(location_card(palette))
        .child(crate::settings_data::data_safety_cards())
}

/// 二选一挂起目标（接管确认对话框开着时的落点集：目标目录原文 + 通道是否
/// 恢复默认语义——确认后的二次提交重放同一通道，旧壳 pendingAdopt 重放闭包
/// 同语义）。
#[derive(Clone, PartialEq)]
struct PendingChoice {
    /// 目标目录原文（接管提示文案归词表，目标随提交通道走）。
    target: String,
    /// 通道语义：true = 恢复默认通道（二次提交仍走恢复默认）。
    restore: bool,
}

/// 提交流分支（提交通道共同出口的呈现诉求集，旧壳 submitWithChoice 分支同
/// 形：意图落盘成功 / 待二选一挂起 / 校验拒绝三枝呈现）。从 boot 结果投影
/// 的纯映射单点（二选一挂起时目标目录随行，接管对话框文案归视图词表）。
enum SubmitFlow {
    /// 意图已落盘：成功提示 + 重载刷新。
    Committed,
    /// 目标已有同名库：挂起二选一（目标目录原文 + 通道语义）。
    NeedsChoice(String, bool),
    /// 校验拒绝：错误反馈，状态不变。
    Rejected(String),
}

/// 提交结果 → 呈现分支（显式 fn 让 into 目标不再依赖尾表达式的推断面）。
fn to_flow(
    result: Result<
        ledger_infra::db::data_location::DataLocationChangeOutcome,
        ledger_infra::error::AppError,
    >,
) -> SubmitFlow {
    result.into()
}

impl
    From<
        Result<
            ledger_infra::db::data_location::DataLocationChangeOutcome,
            ledger_infra::error::AppError,
        >,
    > for SubmitFlow
{
    fn from(
        outcome: Result<
            ledger_infra::db::data_location::DataLocationChangeOutcome,
            ledger_infra::error::AppError,
        >,
    ) -> Self {
        match outcome {
            Ok(outcome) if outcome.requires_choice => {
                SubmitFlow::NeedsChoice(outcome.target_dir.unwrap_or_default(), false)
            }
            Ok(_) => SubmitFlow::Committed,
            Err(e) => SubmitFlow::Rejected(format!("{MSG_CHANGE_PREFIX}{e}")),
        }
    }
}

/// 数据存储位置卡（信息聚合 → 展示；更改/恢复 → 提交通道；二选一 → 接管
/// 对话框）。
fn location_card(palette: views::ViewPalette) -> View {
    let owner = Owner::current();
    let load_seq = signal(0u64);
    let info: Signal<Option<LocationView>> = signal(None);
    let status: Signal<Option<String>> = signal(None);
    let load_error = signal(false);
    let submitting = signal(false);
    // 二选一待决目标（Some = 接管确认对话框开着；确认/取消落 None——旧壳
    // pendingAdopt + adoptConfirmShow 的合流单信号形态；restore 标志让接管
    // 二次提交回到与首次提交相同的通道——旧壳 pendingAdopt 重放同一闭包
    // 同语义）。
    let pending = signal(None::<PendingChoice>);

    // 信息装载批（意图文件读 + 引导记录聚合是文件 IO；blocking 面）：失败
    // 诚实呈现 + 手动重试（旧壳 loadFailed + retry 同口径）。重载前若上一轮
    // 是装载错误（重试路径），成功后清回无反馈；提交通道的反馈（更改已保存 /
    // 已取消等）由重载保留不清——提交反馈不被下一个快照吞掉。共享动作面全
    // Copy 捕获（信号束 Copy，嵌套闭包自动 Fn + Copy，引擎 handler 界直受）。
    let refresh = {
        let (info, status, load_error, load_seq) = (info, status, load_error, load_seq);
        move || {
            let retrying = load_error.get_untracked();
            load_error.set(false);
            load_seq.update(|seq| *seq += 1);
            let seq = load_seq.get();
            let launch = book_dir();
            owner.run(move || {
                spawn(async move {
                    let Some(launch) = launch else {
                        info.set(None);
                        status.set(Some(SESSION_MISSING.to_owned()));
                        return;
                    };
                    let outcome = blocking(move || boot::data_location_info(&launch)).await;
                    if load_seq.get() != seq {
                        return; // latest-wins：后发不落位（重载竞态）
                    }
                    match outcome {
                        Ok(raw) => {
                            info.set(Some(location_view(&raw)));
                            load_error.set(false);
                            if retrying {
                                status.set(None);
                            }
                        }
                        Err(e) => {
                            info.set(None);
                            load_error.set(true);
                            status.set(Some(format!("{MSG_LOAD_PREFIX}{e}")));
                        }
                    }
                });
            });
        }
    };
    refresh();

    // 分支呈现（提交通道共同出口，旧壳 applyOutcome 同形）：成功提示 + 重载
    // / 挂起二选一 / 拒绝反馈。
    let apply_flow = {
        let (status, pending, refresh) = (status, pending, refresh);
        move |flow: SubmitFlow| {
            submitting.set(false);
            match flow {
                SubmitFlow::Committed => {
                    status.set(Some(MSG_COMMITTED.to_owned()));
                    pending.set(None);
                    refresh();
                }
                SubmitFlow::NeedsChoice(target, restore) => {
                    pending.set(Some(PendingChoice { target, restore }))
                }
                SubmitFlow::Rejected(message) => status.set(Some(message)),
            }
        }
    };

    // 提交通道（首次提交与二选一二次提交的同一执行通道；旧壳 submitWithChoice
    // 的 NSH 分流形态 adoptExisting 离散化进 SubmitKind）。
    let run_submit = {
        let (apply_flow, submitting, load_seq) = (apply_flow, submitting, load_seq);
        move |submit: SubmitKind| {
            if submitting.get_untracked() {
                return; // 单飞：提交中不重入（按钮禁用第一道，本判定兜底）
            }
            submitting.set(true);
            owner.run(move || {
                spawn(async move {
                    let apply_flow = apply_flow;
                    let load_seq = load_seq;
                    let launch = book_dir();
                    let back_launch = launch.clone();
                    let back_submit = submit.clone();
                    let flow = blocking(move || match (launch, submit) {
                        (Some(launch), SubmitKind::ChangeView { target }) => {
                            to_flow(boot::submit_data_location_change(&launch, &target, false))
                        }
                        (Some(launch), SubmitKind::ChangeAdopt { target }) => {
                            to_flow(boot::submit_data_location_change(&launch, &target, true))
                        }
                        (Some(launch), SubmitKind::Restore) => {
                            to_flow(boot::restore_default_data_location(&launch, false))
                        }
                        (Some(launch), SubmitKind::RestoreAdopt) => {
                            to_flow(boot::restore_default_data_location(&launch, true))
                        }
                        (None, _) => SubmitFlow::Rejected(SESSION_MISSING.to_owned()),
                    })
                    .await;
                    // 二选一挂起目标回填：基础设施二选一信号不带目标（target_dir
                    // 只在 committed 分支有值），挂起目标 = 本次提交的意图目录
                    //（恢复默认通道 = 启动目录），供接管对话框与二次提交通道
                    // 复用（旧壳 pendingAdopt 重放同闭包同语义）。
                    let flow = match flow {
                        Ok(SubmitFlow::NeedsChoice(dir, restore)) if dir.is_empty() => {
                            Ok(SubmitFlow::NeedsChoice(
                                match (back_launch.as_ref(), &back_submit) {
                                    (
                                        Some(_launch),
                                        SubmitKind::ChangeView { target, .. }
                                        | SubmitKind::ChangeAdopt { target, .. },
                                    ) => target.clone(),
                                    (Some(launch), _) => launch.to_string_lossy().to_string(),
                                    _ => dir,
                                },
                                restore,
                            ))
                        }
                        flow => flow,
                    };
                    match flow {
                        Ok(flow) => {
                            if let SubmitFlow::Committed = flow {
                                load_seq.update(|seq| *seq += 1);
                            }
                            apply_flow(flow);
                        }
                        Err(_) => apply_flow(SubmitFlow::Rejected(WRITE_TASK_FAILED.to_owned())),
                    }
                });
            });
        }
    };

    // 更改位置入口：目录选择对话框 → 选定后进提交通道（取消选择零副作用，
    // 旧壳 open() 返回非字符串同分支；对话框标题旧壳 pickTitle 同词）。
    let change_entry = {
        move || {
            owner.run(move || {
                spawn(async move {
                    let apply = run_submit;
                    let answer = pick_paths(OpenDialog::folders().title(PICK_LOCATION_TITLE)).await;
                    match answer {
                        Ok(Some(paths)) if !paths.is_empty() => {
                            let target = paths[0].to_string_lossy().to_string();
                            apply(SubmitKind::ChangeView { target });
                        }
                        // 取消（None）或对话框失败：零副作用（不开守卫、不写
                        // 意图——select 取消不投递）。
                        _ => {}
                    }
                });
            });
        }
    };

    // 恢复默认入口：目标 = 本壳默认数据目录（启动目录），同一确认与反馈形态
    //（旧壳 restoreDefault 直接走 submitWithChoice 同形）。二选一同样可能出现
    //（默认目录可能保留旧库 / 已登记他本——基础设施同一入口处理）。
    let restore_entry = {
        let apply = run_submit;
        move || apply(SubmitKind::Restore)
    };

    // 二选一接管确认（warning 级，danger 封装的首个壳内消费面）：开着 = 待决
    // 目标在场；确认以 adopt_existing = true 二次提交（弹窗关窗先行，与既有
    // submitting 旋钮衔接，旧壳 confirmAdopt 同形）；取消 / ESC 同为放弃（清
    // 挂起 + 提示状态未变，不让挂起目标悬置）。
    let adopt_outcome = {
        let (pending, status, submitting) = (pending, status, submitting);
        move |outcome: DangerOutcome| {
            let chosen = pending.get_untracked();
            pending.set(None);
            match outcome {
                DangerOutcome::Confirmed => {
                    if let Some(chosen) = chosen {
                        let apply = run_submit;
                        let tool = match chosen.restore {
                            false => SubmitKind::ChangeAdopt {
                                target: chosen.target,
                            },
                            true => SubmitKind::RestoreAdopt,
                        };
                        apply(tool);
                    }
                }
                DangerOutcome::Dismissed => {
                    status.set(Some(MSG_CANCELLED.to_owned()));
                    submitting.set(false);
                }
            }
        }
    };
    let adopt_dialog = danger_confirm(
        move || pending.get().is_some(),
        DangerConfirmSpec {
            level: DangerLevel::Warning,
            title: ADOPT_TITLE.to_owned(),
            lead: Some(ADOPT_LEAD.to_owned()),
            strong_warning: Some(ADOPT_STRONG.to_owned()),
            detail: Some(ADOPT_DETAIL.to_owned()),
            confirm_label: ADOPT_CONFIRM.to_owned(),
            cancel_label: ADOPT_CANCEL.to_owned(),
        },
        adopt_outcome,
    );

    // 生效位置行（路径原文回显旧壳 active_dir；装载中出现「读取中…」态）。
    let active_row = view()
        .flex_row()
        .app_class("items-center gap-2 flex-wrap")
        .child(
            text(LOCATION_ACTIVE_LABEL)
                .text_size(px(13.0))
                .text_color(palette.fg),
        )
        .child(
            text(move || {
                info.get()
                    .as_ref()
                    .map(|view| view.active_dir.clone())
                    .unwrap_or_else(|| LOCATION_READING.to_owned())
            })
            .text_size(px(12.0))
            .text_color(palette.fg)
            .test_id("loc-active-path"),
        )
        .child(copy_button(
            "loc-copy-active",
            move || info.get_untracked().map(|view| view.active_dir.clone()),
            move || {
                status.set(Some(MSG_COPIED.to_owned()));
            },
        ));

    // 回退警示行（回退原因随带；旧壳 NAlert error 横幅卡内平价）。
    let fallback_row = view().child(
        when(
            move || info.get().is_some_and(|view| view.fallback.is_some()),
            move || {
                view()
                    .flex_col()
                    .app_class(
                        "rounded-md border border-destructive/30 bg-destructive/10 p-2 gap-1",
                    )
                    .child(
                        text(LOCATION_FALLBACK_TITLE)
                            .text_size(px(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.fg)
                            .test_id("loc-fallback-title"),
                    )
                    .child(
                        text(move || {
                            info.get_untracked()
                                .and_then(|view| view.fallback.clone())
                                .unwrap_or_default()
                        })
                        .text_size(px(12.0))
                        .text_color(palette.muted)
                        .test_id("loc-fallback"),
                    )
            },
        )
        .otherwise(view),
    );

    // 待重启行（意图已落盘未搬迁：新位置随行 + 复制路径入口——旧壳 pending
    // 横幅 + copy-pending-path 同形）。
    let pending_row = view().child(
        when(
            move || info.get().is_some_and(|view| view.pending.is_some()),
            move || {
                view()
                    .flex_col()
                    .app_class("rounded-md border border-border p-2 gap-1")
                    .child(
                        text(LOCATION_PENDING_TITLE)
                            .text_size(px(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.fg)
                            .test_id("loc-pending-title"),
                    )
                    .child(
                        view()
                            .flex_row()
                            .app_class("items-center gap-2 flex-wrap")
                            .child(
                                text(move || {
                                    info.get_untracked()
                                        .and_then(|view| view.pending.clone())
                                        .unwrap_or_default()
                                })
                                .text_size(px(12.0))
                                .text_color(palette.muted)
                                .test_id("loc-pending"),
                            )
                            .child(copy_button(
                                "loc-copy-pending",
                                move || info.get_untracked().and_then(|view| view.pending.clone()),
                                move || {
                                    status.set(Some(MSG_COPIED.to_owned()));
                                },
                            )),
                    )
            },
        )
        .otherwise(view),
    );

    // 装载失败重试行（旧壳 retry 同口径；成功装载行不出现）。
    let retry_row = view().child(
        when(
            move || load_error.get(),
            move || {
                view().flex_row().app_class("items-center gap-2").child(
                nsh_base::button()
                    .app_class(
                        "px-3 py-1 rounded-md border border-border text-foreground hover:bg-accent",
                    )
                    .child(
                        text(BUTTON_RETRY)
                            .text_size(px(12.0))
                            .text_color(palette.fg)
                            .test_id("loc-retry"),
                    )
                    .on_press(move |_: &()| refresh()),
            )
            },
        )
        .otherwise(view),
    );

    // 动作行：更改入口 + 恢复默认（意图缺席时禁用；单飞 submitting 贯穿两条
    // 通道与二次提交）。
    let actions_row = view()
        .flex_row()
        .app_class("items-center gap-2")
        .child(
            nsh_base::button()
                .app_class("px-3 py-1.5 rounded-md bg-primary hover:opacity-90 disabled:opacity-40")
                .disabled(move || submitting.get())
                .child(
                    text(BUTTON_CHANGE)
                        .text_size(px(13.0))
                        .test_id("loc-change"),
                )
                .on_press(move |_: &()| change_entry()),
        )
        .child(
            nsh_base::button()
                .app_class(
                    "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent disabled:opacity-40",
                )
                .disabled(move || {
                    submitting.get()
                        || info
                            .get_untracked()
                            .is_none_or(|view| view.restore_disabled)
                })
                .child(
                    text(BUTTON_RESTORE_DEFAULT)
                        .text_size(px(13.0))
                        .test_id("loc-restore"),
                )
                .on_press(move |_: &()| restore_entry()),
        );

    view()
        .child(card(
            CARD_LOCATION,
            palette,
            vec![
                view().child(
                    text(LOCATION_HINT)
                        .text_size(px(11.0))
                        .text_color(palette.muted),
                ),
                fallback_row,
                retry_row,
                pending_row,
                active_row,
                actions_row,
                view().child(status_line("loc-status", status, palette)),
            ],
        ))
        .child(adopt_dialog)
}

#[derive(Clone)]
enum SubmitKind {
    /// 更改位置（首次提交，目标 = 对话框选定目录）。
    ChangeView { target: String },
    /// 更改位置·接管确认后的二次提交（adopt_existing = true）。
    ChangeAdopt { target: String },
    /// 恢复默认（首次提交）。
    Restore,
    /// 恢复默认·接管确认后的二次提交（adopt_existing = true）。
    RestoreAdopt,
}

/// 复制路径按钮（旧壳 copy-text 通道：set_clipboard 写剪贴板 + 卡内状态条位
/// 反馈「已复制完整路径」；value_of 返回 None = 目标未装载不动作）。
fn copy_button(
    id: &'static str,
    value_of: impl Fn() -> Option<String> + Copy + 'static,
    on_copied: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    nsh_base::button()
        .app_class("px-2 py-0.5 rounded-md border border-border text-foreground hover:bg-accent")
        .child(
            text(BUTTON_COPY_PATH)
                .text_size(px(11.0))
                .text_color(views::view_palette().fg)
                .test_id(id),
        )
        .on_press(move |_: &()| {
            if let Some(value) = value_of() {
                set_clipboard(value);
                on_copied();
            }
        })
}
// ---------------------------------------------------------------------------
// 功能开关管理面（可关九项 × 一行一开关；写路径归 nav::set_feature_closed
// 单点，与守卫 / 侧栏 / 容器页签 / 本页页签可见性共源）。
// ---------------------------------------------------------------------------

/// 功能页签：功能开关卡（旧壳 FeatureToggleSettings 同构：九行各 = 功能名 +
/// 一句简介 + 开关；不可关六项不出现也不出现空行）。关闭只隐藏入口的源头语
/// 义归骨架票裁决；本面只写状态源单点（卡片无确认无拦截，ADR-0116 决策 8）
/// 与设计页可见性。
fn features_pane(palette: views::ViewPalette) -> View {
    let intro = view().child(
        text(FEATURES_INTRO)
            .text_size(px(12.0))
            .text_color(palette.muted),
    );
    let rows = {
        let mut rows = vec![intro];
        for view in nav_model::CLOSABLE_FEATURES {
            rows.push(feature_row(view, palette));
        }
        rows
    };
    card(CARD_FEATURES, palette, rows)
}

/// 开关行（旧壳 FeatureToggleSettings 一行 = 功能名 + 一句简介 + NSwitch 的
/// 反演面：开态 = 功能开着）。行 testid 同旧壳 `feature-toggle-<id>` 形态——
/// 行是 old-shell 行面 testid 的承载 + headless 的按压替身（switch 行为件本
/// 体不暴露 testid 面盘；行按压写向同一单点 nav::set_feature_closed，与开关
/// on_change 同一执行通道——双按压面同源不另立第二笔写路径）。无障碍角色按
/// 钮（name = 功能名）以 rows 无障碍读法平价。
fn feature_row(feature: nav_model::ViewName, palette: views::ViewPalette) -> View {
    let closed = nav::store().closed;
    // 无障碍名（每行一次的静态化：'static 锚点借用——九行 × 一次格式化的静态
    // 字面量，进程生命周期一致，可接受）。
    let switch_label: &'static str =
        Box::leak(format!("{}{}", feature.label(), " 开关").into_boxed_str());
    let row = view()
        .flex_row()
        .app_class("items-center justify-between gap-4 py-1")
        .child(
            view()
                .flex_col()
                .app_class("gap-0.5")
                .child(
                    text(feature.label())
                        .text_size(px(13.0))
                        .text_color(palette.fg),
                )
                .child(
                    text(feature_description(feature))
                        .text_size(px(11.0))
                        .text_color(palette.muted),
                ),
        )
        .child(
            nsh_base::switch()
                // 开关态 = 「功能开着」（关闭集合的反演读）；无障碍名随动
                //（「开启/关闭 + 功能名」）。
                .accessibility_label(switch_label)
                .checked(move || !closed.get().contains(&feature))
                .on_change(move |value: &bool| nav::set_feature_closed(feature, !*value))
                .app_class("shrink-0"),
        );
    // 行按压替身（headless id 锚点 → 同一写链的执行体；删双按压面中的替身
    // 接线即红——特接 inline press 页面行为）。
    view()
        .test_id(format!("feature-toggle-{}", feature.as_str()).as_str())
        .role(nsh_core::AccessibilityRole::Button)
        .optional_label(move || Some(SharedString::from(feature.label())))
        .app_class("rounded-md")
        .on_press(move |_: &PressEvent| {
            let is_closed = closed.get_untracked().contains(&feature);
            nav::set_feature_closed(feature, !is_closed);
        })
        .child(row)
}

// ---------------------------------------------------------------------------
// lib 单测：设置面的可离线口径（选项闭集 / 数据位置投影 / 页签可见序 / 外观
// 闭集与词表钉子）。headless 组件测试见 tests/app_headless.rs 设置节。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 外观持久化闭集往返：as_str ↔ 持久化名恒等；未知名 / 非字符串回 None
    /// （调用方回 [`DEFAULT_APO`] 深色出厂）。
    #[test]
    fn apo_round_trip_covers_closed_set() {
        for apo in [Apo::System, Apo::Dark, Apo::Light] {
            assert_eq!(apo_parse(&serde_json::json!(apo.as_str())), Some(apo));
        }
        assert_eq!(apo_parse(&serde_json::json!("midnight")), None);
        assert_eq!(apo_parse(&serde_json::json!(3)), None);
        assert_eq!(apo_parse(&serde_json::json!(null)), None);
        assert_eq!(
            DEFAULT_APO,
            Apo::System,
            "出厂跟随系统（立桩验收基线；旧壳出厂深色的偏差见 #2053）"
        );
    }

    /// 外观选项闭集：id = 持久化名闭集、label = 词表原文。
    #[test]
    fn appearance_options_cover_the_closed_set() {
        let options = appearance_options();
        let ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["system", "dark", "light"]);
        let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["跟随系统", "深色", "浅色"]);
    }

    /// 日志档位选项闭集：五档 id = 基建值域闭集原值（boot::set_log_level 的
    /// 接受域同值）、标签汉英双名旧壳词表原文（headless 触发件文本同源）。
    #[test]
    fn log_level_options_match_backend_closed_set() {
        let options = log_level_options();
        for option in &options {
            assert!(
                ledger_infra::settings::LOG_LEVEL_DIRECTIVES.contains(&option.id.as_str()),
                "选项 id 必须是后端日志档位闭集原值：{}",
                option.id
            );
        }
        let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "错误 (error)",
                "警告 (warn)",
                "信息 (info)",
                "调试 (debug)",
                "追踪 (trace)"
            ]
        );
    }

    /// 数据位置卡展示投影：意图在案 → 待重启横幅随意图目录、「恢复默认」可
    /// 用；意图缺席 → 无待重启横幅、「恢复默认」禁用（configured_dir 非空 ⇔
    /// 存在自定义位置意图的契约不变量，卡内禁用是其直接投影）。
    #[test]
    fn location_view_projects_pending_fallback_and_restore_state() {
        let view = location_view(&ledger_infra::db::data_location::DataLocationInfo {
            active_dir: "/home/user/ledger".to_owned(),
            configured_dir: Some("/mnt/nas/ledger".to_owned()),
            pending_restart: true,
            fallback_reason: None,
        });
        assert_eq!(view.active_dir, "/home/user/ledger");
        assert!(
            view.pending
                .as_deref()
                .unwrap_or_default()
                .contains("/mnt/nas/ledger"),
            "待重启横幅必须呈现意图目录原文：{:?}",
            view.pending
        );
        assert!(view.fallback.is_none());
        assert!(!view.restore_disabled);

        let view = location_view(&ledger_infra::db::data_location::DataLocationInfo {
            active_dir: "/home/user/ledger".to_owned(),
            configured_dir: None,
            pending_restart: false,
            fallback_reason: Some("旧指针无法读取".to_owned()),
        });
        assert!(view.pending.is_none());
        assert!(
            view.fallback
                .as_deref()
                .unwrap_or_default()
                .contains("旧指针无法读取")
        );
        assert!(view.restore_disabled);
    }

    /// 页签可见序：全开态 = 分域序闭集；定时关闭 → 「定时」页签退出、其余原
    /// 位；非定时项关闭不涉本页签面（关闭只隐藏入口，重开即回）。
    #[test]
    fn visible_tabs_hides_closed_scheduled_only() {
        assert_eq!(visible_tabs(&[]).len(), 7);
        let without = visible_tabs(&[nav_model::ViewName::Scheduled]);
        assert_eq!(without.len(), 6);
        for tab in visible_tabs(&[]) {
            assert_eq!(
                without.contains(&tab),
                tab != SettingsTab::Scheduled,
                "页签「{}」可见性应符合关闭集合",
                tab.label()
            );
        }
        assert_eq!(
            visible_tabs(&[nav_model::ViewName::Budget]).len(),
            7,
            "非定时项关闭不改设置页页签面"
        );
    }

    /// 功能简介词表钉子：可关九项各有一句简介；不可关六项空串（行面不出现，
    /// 也不出现空行）。
    #[test]
    fn feature_descriptions_cover_closable_nine() {
        for view in nav_model::CLOSABLE_FEATURES {
            assert!(
                !feature_description(view).is_empty(),
                "可关项「{}」必须有简介文案",
                view.as_str()
            );
        }
        for view in [
            nav_model::ViewName::Dashboard,
            nav_model::ViewName::Transactions,
            nav_model::ViewName::Accounts,
            nav_model::ViewName::Search,
            nav_model::ViewName::Ai,
            nav_model::ViewName::Settings,
        ] {
            assert_eq!(feature_description(view), "");
        }
    }
}
