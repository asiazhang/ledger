//! 主题令牌装配（issue #2100，ADR-0147 决策 1）：清单挂载解析 + 令牌消费
//! 单点。
//!
//! **挂载链**：`nsh.json`（应用清单，`"theme"` 键是主题挂载点）→ `theme.json`
//! （主题令牌表，shadcn `cssVars` 格式，亮暗双态齐全、覆盖式只写差异）→
//! 引擎令牌（[`tokens`]，随根组件挂载，[`crate::app::root`] 单点）→ 视图经
//! [`color`] 与令牌类消费。两份文件都经编译期内嵌（`include_str!`）：文件
//! 缺失或挪位直接编译失败，强于 CLI 构建期校验；运行时解析失败不 panic，
//! 告警后落出厂 neutral（等待成功优于即刻失败，AGENTS.md 体验取舍）。
//!
//! **挂载语义**（负向判据的实体）：清单无 `"theme"` 键、键值不是本仓主题
//! 文件名或可解析主题 → 令牌整体落 NSH 出厂默认 shadcn `neutral`（出厂基线
//! 接线证明——移除清单中的主题挂载，应用即回退黑白灰默认观感）。
//!
//! **分层**（ADR-0147 决策 1）：本表答「出厂观感是什么」；设备级亮暗强指归
//! 外观设置接缝（[`crate::settings`]，`set_color_scheme` 单点），清单不设
//! `appearance` 键——两层正交，不立第二源。
//!
//! **令牌闭集**：shadcn 标准 UI 语义名（`background`/`primary`/`muted`/
//! `destructive` 等，未写明的沿用出厂默认）+ 本仓自有令牌——金额语义色九档
//! （`expense`…`dividend`）、警告与成功（`warning` 亮 `#f0a020` / 暗 `#f2c97d`、`success` 亮 `#18a058` / 暗 `#63e2b7`）、走势主色（`line`）、图表系列
//! 十二槽（`chart-series-1…12`）与未分类灰（`chart-uncategorized`）、圆角阶
//! 四档（`radius` 8 / `radius-card` 12 / `radius-panel` 16 / `radius-inset`
//! 8 点，同心规则见 docs/DESIGN.md）。散落模块的本地色常量已按此表收编
//! （视图层散落兜底随 #2104 与 review 收编清零）；全仓散落色常量的唯一合法
//! 住所是本模块的缺失兜底（见 [`color`]）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::Theme;
use nsh_core::element::{AdaptiveColor, Tokens};

/// 应用清单（`nsh.json`；主题挂载点唯一来源，编译期内嵌）。
const NSH_MANIFEST: &str = include_str!("../nsh.json");

/// 主题令牌表（`theme.json`；shadcn `cssVars`，编译期内嵌）。
const THEME_FILE: &str = include_str!("../theme.json");

/// 清单 `"theme"` 键指向的主题文件名（与 [`THEME_FILE`] 内嵌副本对应；指向
/// 其他路径视为未挂载——本仓只随二进制内嵌这一份主题）。
const THEME_FILE_NAME: &str = "theme.json";

/// 令牌缺失的可读兜底（中灰，亮暗底都可读）：散落色常量收编后，全仓唯一的
/// 本地色常量住所——消费方（图表、危险态、金额语义色）零本地色常量，令牌
/// 缺失统一落此兜底而非各自私藏色值。
const MISSING_TOKEN_FALLBACK: AdaptiveColor = AdaptiveColor::rgb(0x808080, 0x808080);

/// 清单挂载 → 令牌（纯函数，离线可测）：清单 JSON 里 `"theme"` 是
/// - 指向 [`THEME_FILE_NAME`] 的路径 → 用主题文件内容；
/// - 内联主题对象（`{"cssVars": …}` 同形）→ 直接用；
/// - 缺键 / 其他形态 / 解析失败 → 出厂 neutral（负向判据：移除挂载即回退）。
pub(crate) fn tokens_for(manifest: &str, theme_file: &str) -> Tokens {
    let mount = serde_json::from_str::<serde_json::Value>(manifest)
        .ok()
        .and_then(|value| value.get("theme").cloned());
    let theme_json = match mount {
        Some(serde_json::Value::String(path)) if path == THEME_FILE_NAME => {
            Some(theme_file.to_owned())
        }
        Some(inline @ serde_json::Value::Object(_)) => Some(inline.to_string()),
        other => {
            if other.is_some() {
                tracing::warn!("nsh.json: theme: 未指向 {THEME_FILE_NAME}，落出厂 neutral");
            }
            None
        }
    };
    let theme = theme_json
        .and_then(|json| match Theme::from_json(&json) {
            Ok(theme) => Some(theme),
            Err(e) => {
                tracing::warn!("nsh.json: theme: 解析失败（{e}），落出厂 neutral");
                None
            }
        })
        .unwrap_or_else(Theme::default);
    theme.into_tokens()
}

/// 启动装配的令牌（随根组件挂载，[`crate::app::root`] 消费）：清单挂载解析
/// 的唯一入口。
pub(crate) fn tokens() -> Tokens {
    tokens_for(NSH_MANIFEST, THEME_FILE)
}

/// 令牌色读取单点（引擎线程；读取即订阅，亮暗切换原生跟随）：令牌缺失落
/// [`MISSING_TOKEN_FALLBACK`] 可读兜底，不 panic 不静默透明。图表、危险态与
/// 金额语义色一律经此消费——消费方零本地色常量。
pub(crate) fn color(name: &str) -> AdaptiveColor {
    nsh_core::element::tokens()
        .color(name)
        .unwrap_or(MISSING_TOKEN_FALLBACK)
}

/// 内嵌主题表是否在册令牌 `name`（离线可测的收编对表面：图表/语义色收编的
/// 单测据此钉住令牌表闭集，不起引擎）。
#[cfg(test)]
pub(crate) fn bundled_theme_has_token(name: &str) -> bool {
    tokens().get(name).is_some()
}

/// 内嵌主题表的圆角阶四档（radius / card / panel / inset；离线对表面：
/// design 常量模块与主题令牌同值核对，漂移即两侧任一测试红）。
#[cfg(test)]
pub(crate) fn bundled_theme_radius_tiers() -> (f32, f32, f32, f32) {
    let tokens = tokens();
    (
        tokens.number("radius").unwrap_or_default(),
        tokens.number("radius-card").unwrap_or_default(),
        tokens.number("radius-panel").unwrap_or_default(),
        tokens.number("radius-inset").unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内嵌清单与主题文件是合法 JSON、清单挂载恰好指向内嵌主题文件名——
    /// 三者任一漂移（文件挪位 / 键名笔误）即红。
    #[test]
    fn bundled_manifest_mounts_the_bundled_theme() {
        let manifest: serde_json::Value = serde_json::from_str(NSH_MANIFEST).unwrap();
        assert_eq!(
            manifest.get("theme").and_then(|v| v.as_str()),
            Some(THEME_FILE_NAME),
            "清单 theme 键指向 theme.json"
        );
        serde_json::from_str::<serde_json::Value>(THEME_FILE)
            .unwrap_or_else(|e| panic!("theme.json 是合法 JSON：{e}"));
    }

    /// 主题令牌生效：挂载在册时自有令牌取到表内值（支出红 = 旧壳 expense
    /// 亮态同值），标准名取到定制底色（非 neutral 纯白）。
    #[test]
    fn mount_resolves_own_tokens_and_tinted_base() {
        let tokens = tokens_for(NSH_MANIFEST, THEME_FILE);
        let expense = tokens.color("expense").expect("自有令牌 expense 在册");
        assert_eq!(expense.light, nsh_core::color::rgb(0xd03050).to_hsla());
        assert_eq!(expense.dark, nsh_core::color::rgb(0xe88080).to_hsla());
        let background = tokens.color("background").expect("标准名 background 在册");
        assert_ne!(
            background.light,
            nsh_core::color::rgb(0xffffff).to_hsla(),
            "出厂观感脱离 neutral 纯白（定制账本配色）"
        );
    }

    /// 负向条目（出厂基线接线证明）：移除清单中的主题挂载 → 令牌整体等于
    /// 出厂 neutral（Theme::default）。挂载解析硬编码（无视清单）即红。
    #[test]
    fn removing_the_mount_falls_back_to_default_neutral() {
        let unmounted = r#"{"name":"Ledger","rust":".","root":"app::App"}"#;
        let tokens = tokens_for(unmounted, THEME_FILE);
        let neutral = Theme::default().into_tokens();
        assert_eq!(
            tokens.color("background").map(|c| c.light),
            neutral.color("background").map(|c| c.light),
            "无挂载 → neutral 底色"
        );
        assert!(tokens.color("expense").is_none());
        assert!(
            tokens.color("warning").is_none(),
            "neutral 出厂集无 warning/涨跌档"
        );
    }

    /// 内联主题对象同效（清单 `"theme"` 允许内联主题内容，官方文档同形）。
    #[test]
    fn inline_theme_object_mounts_alike() {
        let inline = r##"{"name":"Ledger","theme":{"cssVars":{"light":{"expense":"#d03050"}}}}"##;
        let tokens = tokens_for(inline, THEME_FILE);
        let expense = tokens.color("expense").expect("内联令牌在册");
        assert_eq!(expense.light, nsh_core::color::rgb(0xd03050).to_hsla());
    }

    /// 自有令牌闭集在册（收编对表的单一位置）：金额语义九档、警告、走势
    /// 主色、图表系列十二槽与未分类灰、确认按钮前景两档、圆角阶四档——
    /// 收编漏档（表删档或消费方键名漂移不一致）即红。
    #[test]
    fn bundled_theme_carries_the_own_token_closure() {
        let tokens = tokens_for(NSH_MANIFEST, THEME_FILE);
        let amount_tokens = [
            "expense", "income", "refund", "transfer", "buy", "sell", "convert", "split",
            "dividend",
        ];
        for name in amount_tokens {
            let color = tokens
                .color(name)
                .unwrap_or_else(|| panic!("自有令牌 {name} 在册"));
            assert_ne!(color.light, color.dark, "{name} 亮暗双态齐全且分层");
        }
        for name in [
            "warning",
            "success",
            "destructive-foreground",
            "warning-foreground",
            "chart-uncategorized",
        ] {
            assert!(tokens.color(name).is_some(), "自有令牌 {name} 在册");
        }
        let line = tokens.color("line").expect("走势主色 line 在册");
        assert_eq!(line.light, line.dark, "line 亮暗同值（单值档）");
        for slot in 1..=12 {
            assert!(
                tokens.color(&format!("chart-series-{slot}")).is_some(),
                "图表系列槽 chart-series-{slot} 在册"
            );
        }
        assert_eq!(tokens.number("radius"), Some(8.0), "圆角基档 8 点");
        assert_eq!(tokens.number("radius-card"), Some(12.0));
        assert_eq!(tokens.number("radius-panel"), Some(16.0));
        assert_eq!(tokens.number("radius-inset"), Some(8.0));
    }

    /// 指向非本仓主题文件名 = 未挂载（内嵌副本只有一份），落 neutral 不 panic。
    #[test]
    fn unknown_theme_path_falls_back_to_neutral() {
        let tokens = tokens_for(r#"{"theme":"brand.json"}"#, THEME_FILE);
        assert!(tokens.color("expense").is_none());
        assert!(tokens.color("background").is_some(), "neutral 标准名仍在");
    }

    /// 清单不是 JSON → neutral（运行时解析失败不 panic 的防御半边）。
    #[test]
    fn broken_manifest_falls_back_to_neutral() {
        let tokens = tokens_for("not json", THEME_FILE);
        assert!(tokens.color("expense").is_none());
    }
}
