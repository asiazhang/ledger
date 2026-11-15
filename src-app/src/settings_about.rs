//! 关于卡（issue #2136 设置③；旧壳 `AboutSettings.vue` + `app-links` +
//! `useAppBuildInfo` 的重生，issue #930 / #1957 / ADR-0022 修订注记）：只读元
//! 信息 + 读型动作 + 静态简介（旧壳 #1959 同口径：卡片顶部定位语与简介，纯
//! 只读无入口、不进复制文本）。设置入口不在此——日志归「通用」日志卡、应用
//! 更新归「通用」更新卡、数据位置归「数据」页签（ADR-0022「一问一处」）。
//!
//! **构建元信息三环**（旧壳三字段真源各归其位的 NSH 对应）：版本 = 编译期
//! 包版本（[`crate::backup_entry::APP_VERSION`]，与安装包 / 更新比较同源）；
//! 运行平台 = 编译期常量（`std::env::consts`，平台 / 架构显示名旧壳
//! `formatPlatform` 同映射）；Git 版本 = 构建期注入的 sha
//!（`src-app/build.rs` 的 `NSH_GIT_SHA`，与 Web 参照前端 vite define 同口径，
//! 包内 build.rs 模块文档同源）。构建元信息未到位时对应行不渲染——宁缺不
//! 填假值（旧壳同口径）。
//!
//! **信息清单与复制文本同源**：屏幕上的每一行都由同一 [`about_rows`] 派生，
//! 复制出的文本就是这些行本身——不出现「复制的里面有、屏幕上没有」的隐藏
//! 字段（旧壳 #1957 同裁决）。Git 行点击复制完整 40 位 sha（行内展示 7 位短
//! sha + dirty 后缀，旧壳同口径）；外部链接经 [`open_url`] 宿主请求通道
//!（Linux 平台实现 = 系统浏览器，headless 记录断言）。
//!
//! 界面文案仅 zh-CN（旧壳 settings.about.* zh-CN 词条原样承载）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::backup_entry::APP_VERSION;
use crate::design::font_size;
use crate::settings::card;
use crate::views;

// ---------------------------------------------------------------------------
// 词表（旧壳 settings.about.* zh-CN 词条原样；字面承载）。
// ---------------------------------------------------------------------------
pub(crate) const CARD_TITLE: &str = "关于开源记账";
const TAGLINE: &str = "本地优先的开源个人账本";
const INTRO: &str = "开源记账把账本完全交给你自己：数据只存在你的设备上，离线可用、可选加密，跨设备同步由你掌控。应用完全开源，桌面与 Android 皆可用。";
const LABEL_APP_NAME: &str = "应用名称";
const LABEL_VERSION: &str = "版本号";
const LABEL_GIT: &str = "Git 版本";
const LABEL_PLATFORM: &str = "运行平台";
const LABEL_LICENSE: &str = "许可证";
const LABEL_COPYRIGHT: &str = "版权";
const BUTTON_HOMEPAGE: &str = "项目主页";
const BUTTON_ISSUES: &str = "反馈问题";
const BUTTON_COPY_INFO: &str = "复制版本信息";
const MSG_COPY_OK: &str = "已复制版本信息";
const MSG_COPY_SHA_OK: &str = "已复制完整版本号";
const VALUE_SEPARATOR: &str = "：";

/// 应用名（旧壳 common.app.name 同值；NSH 单语言 zh 硬编码）。
const APP_NAME: &str = "开源记账";

/// 许可证与版权：逐字取仓库 LICENSE（旧壳同裁决——年份刻意不动态取当前年，
/// 动态年份跨年后就与 LICENSE 不一致）。
const LICENSE_NAME: &str = "MIT";
const COPYRIGHT_NOTICE: &str = "© 2026 asiazhang";

/// 项目主页（旧壳 app-links `PROJECT_HOME_URL` 同值，常量单点随卡承载）。
pub(crate) const PROJECT_HOME_URL: &str = "https://github.com/asiazhang/ledger";
/// 反馈入口（旧壳 app-links `ISSUES_URL` 同值）。
pub(crate) const ISSUES_URL: &str = "https://github.com/asiazhang/ledger/issues";

// ---------------------------------------------------------------------------
// 装配层（纯投影，无 UI 依赖）。
// ---------------------------------------------------------------------------

/// 构建期 Git sha（`src-app/build.rs` 注入；非 Git 构建 / 注入缺席 = 空串，
/// Git 行按「宁缺不填假值」不渲染）。
pub(crate) fn git_sha_full() -> &'static str {
    match option_env!("NSH_GIT_SHA") {
        Some(sha) if !sha.is_empty() => sha,
        _ => "",
    }
}

/// 短 sha 展示（前 7 位，脏树追加 `-dirty`；无法读取 Git 信息时返回空串——
/// 旧壳 `gitVersionLabel` 同口径）。
pub(crate) fn git_version_label() -> String {
    let sha = git_sha_full();
    if sha.is_empty() {
        return String::new();
    }
    let dirty = option_env!("NSH_GIT_DIRTY")
        .map(|flag| flag == "true")
        .unwrap_or(false);
    format!(
        "{}{}",
        &sha[..7.min(sha.len())],
        if dirty { "-dirty" } else { "" }
    )
}

/// 运行平台显示值（旧壳 `formatPlatform` 同映射：平台显示名 + 架构显示名，
/// 「Linux · x64」形态；未登记取值原样透出——显示错比显示陌生值更糟）。
pub(crate) fn platform_label() -> String {
    let platform = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        "android" => "Android",
        "ios" => "iOS",
        "freebsd" => "FreeBSD",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    format!("{platform} · {arch}")
}

/// 元信息行（key 只作渲染与测试定位，label/value 即用户所见）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AboutRow {
    pub key: &'static str,
    pub label: &'static str,
    pub value: String,
}

/// 屏幕行清单（复制文本的唯一来源）：构建元信息未到位时对应行不渲染，与
/// Git 版本行同形——宁缺不填假值（旧壳 rows computed 同口径）。
pub(crate) fn about_rows() -> Vec<AboutRow> {
    let mut rows = vec![AboutRow {
        key: "app-name",
        label: LABEL_APP_NAME,
        value: APP_NAME.to_owned(),
    }];
    rows.push(AboutRow {
        key: "version",
        label: LABEL_VERSION,
        value: APP_VERSION.to_owned(),
    });
    let git = git_version_label();
    if !git.is_empty() {
        rows.push(AboutRow {
            key: "git",
            label: LABEL_GIT,
            value: git,
        });
    }
    rows.push(AboutRow {
        key: "platform",
        label: LABEL_PLATFORM,
        value: platform_label(),
    });
    rows.push(AboutRow {
        key: "license",
        label: LABEL_LICENSE,
        value: LICENSE_NAME.to_owned(),
    });
    rows.push(AboutRow {
        key: "copyright",
        label: LABEL_COPYRIGHT,
        value: COPYRIGHT_NOTICE.to_owned(),
    });
    rows
}

/// 一行文本（`标签：值`）——复制文本的行形态。
fn row_line(row: &AboutRow) -> String {
    format!("{}{}{}", row.label, VALUE_SEPARATOR, row.value)
}

/// 复制版本信息文本（整屏可见行，报障时一次粘贴即可——旧壳 copyVersionInfo
/// 同语义；行清单与 [`about_rows`] 同源，不出现隐藏字段）。
pub(crate) fn version_info_text() -> String {
    about_rows()
        .iter()
        .map(row_line)
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// 视图（设置页「关于」页签消费面）。
// ---------------------------------------------------------------------------

/// 关于页签内容（只读元信息 + 复制 + 外部链接）。
pub(crate) fn about_pane(palette: views::ViewPalette) -> View {
    let status = signal(None::<String>);
    // 行集（元信息构建期定格，进程内不变化——一次性快照即可）。
    let rows = about_rows();

    let mut rows_view = view().flex_col().app_class("gap-1");
    for row in &rows {
        let value = row.value.clone();
        let is_git = row.key == "git";
        let value_id: &'static str = Box::leak(format!("about-row-{}", row.key).into_boxed_str());
        let mut value_view = text(value)
            .text_size(px(font_size::SECONDARY))
            .text_color(palette.fg)
            .test_id(value_id);
        // Git 行点击复制完整 sha（旧壳 onRowClick 同语义：只有 Git 行是动作，
        // 其余行无交互——不把整张卡变成可点区域）。
        if is_git {
            value_view = value_view.on_press(move |_: &PressEvent| {
                set_clipboard(git_sha_full());
                status.set(Some(MSG_COPY_SHA_OK.to_owned()));
            });
        }
        rows_view = rows_view.child(
            view()
                .flex_row()
                .app_class("gap-2")
                .child(
                    text(row.label)
                        .text_size(px(font_size::SECONDARY))
                        .text_color(palette.muted),
                )
                .child(value_view),
        );
    }

    // 动作行：复制版本信息 + 外部链接（打开失败落状态条位——不静默吞）。
    let status_copy = status;
    let status_home = status;
    let status_issues = status;
    let owner = Owner::current();
    let owner_home = owner;
    let owner_issues = owner;
    card(
        CARD_TITLE,
        palette,
        vec![
            view()
                .flex_col()
                .app_class("gap-1")
                .test_id("about-intro")
                .child(
                    text(TAGLINE)
                        .text_size(px(font_size::SECONDARY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.fg),
                )
                .child(
                    text(INTRO)
                        .text_size(px(font_size::CAPTION))
                        .text_color(palette.muted),
                ),
            rows_view,
            view()
                .flex_row()
                .app_class("items-center gap-2")
                .child(
                    nsh_base::button()
                        .app_class("px-2 py-0.5 rounded-sm border border-border text-foreground hover:bg-accent")
                        .on_press(move |_: &()| {
                            set_clipboard(version_info_text());
                            status_copy.set(Some(MSG_COPY_OK.to_owned()));
                        })
                        .child(
                            text(BUTTON_COPY_INFO)
                                .text_size(px(font_size::CAPTION))
                                .text_color(palette.fg)
                                .test_id("about-copy-info"),
                        ),
                )
                .child(
                    nsh_base::button()
                        .app_class("px-2 py-0.5 rounded-sm border border-border text-foreground hover:bg-accent")
                        .on_press(move |_: &()| {
                            owner_home.run(move || {
                                spawn(async move {
                                    if !open_url(PROJECT_HOME_URL).await {
                                        status_home
                                            .set(Some("打开项目主页失败：浏览器未就绪".to_owned()));
                                    }
                                });
                            });
                        })
                        .child(
                            text(BUTTON_HOMEPAGE)
                                .text_size(px(font_size::CAPTION))
                                .text_color(palette.fg)
                                .test_id("about-link-home"),
                        ),
                )
                .child(
                    nsh_base::button()
                        .app_class("px-2 py-0.5 rounded-sm border border-border text-foreground hover:bg-accent")
                        .on_press(move |_: &()| {
                            owner_issues.run(move || {
                                spawn(async move {
                                    if !open_url(ISSUES_URL).await {
                                        status_issues.set(Some(
                                            "打开反馈入口失败：浏览器未就绪".to_owned(),
                                        ));
                                    }
                                });
                            });
                        })
                        .child(
                            text(BUTTON_ISSUES)
                                .text_size(px(font_size::CAPTION))
                                .text_color(palette.fg)
                                .test_id("about-link-issues"),
                        ),
                ),
            view().child(crate::settings::status_line("about-status", status, palette)),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 行清单（复制文本的唯一来源）：应用名 / 版本 / 平台 / 许可证 / 版权
    /// 恒在；Git 行按「宁缺不填假值」——构建期注入缺席时不渲染。
    #[test]
    fn about_rows_cover_the_screen_closure() {
        let rows = about_rows();
        let keys: Vec<&str> = rows.iter().map(|r| r.key).collect();
        // Git 行随构建期注入在场（本仓构建经 build.rs 注入则含 git）；缺席时
        // 不渲染（宁缺不填假值）。
        let core = ["app-name", "version", "platform", "license", "copyright"];
        for key in core {
            assert!(keys.contains(&key), "关于行「{key}」在屏");
        }
        assert_eq!(keys.len(), core.len() + usize::from(keys.contains(&"git")));
        let version = rows.iter().find(|r| r.key == "version").unwrap();
        assert_eq!(version.value, APP_VERSION, "版本号 = 编译期包版本");
        let license = rows.iter().find(|r| r.key == "license").unwrap();
        assert_eq!(license.value, "MIT");
    }

    /// 复制版本信息文本（行清单同源，`标签：值` 形态；静态简介不进复制
    /// 文本——旧壳 #1959 同口径）。
    #[test]
    fn version_info_text_derives_from_about_rows_only() {
        let text = version_info_text();
        for row in about_rows() {
            assert!(
                text.contains(&row_line(&row)),
                "复制文本缺屏上行「{}」",
                row.label
            );
        }
        assert!(!text.contains(TAGLINE), "静态简介不进复制文本");
        assert!(!text.contains(INTRO), "静态简介不进复制文本");
    }

    /// 链接常量（旧壳 app-links 同值）。
    #[test]
    fn link_constants_match_old_shell_app_links() {
        assert_eq!(PROJECT_HOME_URL, "https://github.com/asiazhang/ledger");
        assert_eq!(ISSUES_URL, "https://github.com/asiazhang/ledger/issues");
    }
}
