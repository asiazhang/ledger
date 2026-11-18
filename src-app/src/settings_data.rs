//! 数据安全页签装配与卡间共用件（issue #2054 NSH 迁移㉓卡宿主；ADR-0022
//! 数据页签）。
//!
//! #2167 拆分（行为零变化）：本模块原为备份/恢复、加密、同步、账本切换、
//! 余额缓存多职责聚合（Divergent Change 存量债），按消费面分簇归位——
//! - 备份管理五卡（`BackupSettings` 全量面）→ [`crate::backup_cards`]；
//! - 加密卡（`EncryptionSettings` 全量面）→ [`crate::encryption_card`]；
//! - 多端同步卡（`useSyncCard`，含 S3 厂商预设）→ [`crate::sync_card`]；
//! - 账本切换卡（`useBookSwitcher`）→ [`crate::books_card`]；
//! - 余额缓存审计修复卡（issue #2136）→ [`crate::balance_cache_card`]。
//!
//! 剩余职责单一：数据页签卡宿主装配（[`DataSafetyCards`] 入口组件，挂
//! [`crate::settings`] 数据页签的卡片宿主与状态条位，容器面单点不变）与
//! 跨卡共用件两族——呈现件（密文输入行 [`pass_input`] / 表单字段行
//! [`field_input`]）与装配纯函数（备份产物大小 / 时刻格式化
//! [`format_backup_size`] / [`format_backup_time`]，备份卡与同步卡共用）。
//! 动作反馈落各卡内状态条位（#2038 裁决，NSH 无 toast）；阻塞 IO 一律
//! `blocking` 后台执行，信号读写都在引擎 UI 线程；界面文案仅 zh-CN（旧壳
//! settings.* / books.* zh-CN 词条原样承载），码化错误的中文原样呈现。
//! 弹窗条件段机制选型（#2146 定案，行为零变化）：布尔有无 / 分支翻转 →
//! `when`，整段重造 → `dynamic`（各卡模块头有同款注记；恢复确认弹窗的机
//! 制注记随备份卡住 [`crate::backup_cards`]）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use crate::backup_cards;

use nsh_base::input;
use nsh_core::prelude::*;

use crate::balance_cache_card;
use crate::books_card;
use crate::design::font_size;
use crate::encryption_card;
use crate::sync_card;
use crate::views;

// ---------------------------------------------------------------------------
// 装配层（纯函数与纯投影；渲染期零格式化——行文本已是装配产物）。
// ---------------------------------------------------------------------------

/// 备份产物大小展示（旧壳 formatSize 同口径：B / ×1 小数 KB / MB）。
pub(crate) fn format_backup_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// 备份时刻展示（旧壳 formatIsoMinute 同口径：ISO 剥到分钟 `YYYY-MM-DD
/// HH:mm`；形状不符原样透传，不做第二份口径的格式化猜测）。
pub(crate) fn format_backup_time(iso: &str) -> String {
    let bytes: Vec<char> = iso.chars().collect();
    match (bytes.get(10), bytes.get(16)) {
        (Some('T'), _) if bytes.len() >= 16 => {
            let date: String = bytes[..10].iter().collect();
            let clock: String = bytes[11..16].iter().collect();
            format!("{date} {clock}")
        }
        _ => iso.to_owned(),
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// 共用呈现件（密文输入行 / 表单字段行）；破坏色便捷面收
// [`crate::views::ViewPalette`] 的 `danger_fg` 成员（通面五件套唯一住所）。
// ---------------------------------------------------------------------------

/// 密文输入行（标签 + masked 输入框；旧壳 NInput type=password 对应件）。
pub(crate) fn pass_input(
    target: Signal<String>,
    palette: views::ViewPalette,
    id: &'static str,
    label: &'static str,
    placeholder: &'static str,
) -> View {
    view()
        .flex_col()
        .app_class("gap-1")
        .child(
            text(label)
                .text_size(px(font_size::CAPTION))
                .text_color(palette.fg),
        )
        .child(
            input()
                .test_id(id)
                .value(move || target.get())
                .masked(true)
                .placeholder(placeholder)
                .optional_label(label)
                .text_color(palette.fg)
                .w(px(260.0))
                .app_class(
                    "bg-background rounded-(--radius) border border-border px-2 py-1 placeholder:text-muted-foreground",
                )
                .on_change(move |value: &str| target.set(value.to_owned())),
        )
}

/// 通道表单普通字段行（占位即标签；无障碍名 = 占位原文）。
pub(crate) fn field_input(
    palette: views::ViewPalette,
    id: &'static str,
    placeholder: &'static str,
    target: Signal<String>,
    masked: bool,
    dynamic_placeholder: Option<impl Fn() -> &'static str + Copy + 'static>,
) -> View {
    let width = if masked { px(260.0) } else { px(420.0) };
    let base = input()
        .test_id(id)
        .value(move || target.get())
        .optional_label(placeholder)
        .text_color(palette.fg)
        .w(width)
        .app_class(
            "bg-background rounded-(--radius) border border-border px-2 py-1 placeholder:text-muted-foreground",
        )
        .on_change(move |value: &str| target.set(value.to_owned()));
    let base = match dynamic_placeholder {
        Some(provider) => base.placeholder(provider),
        None => base.placeholder(placeholder),
    };
    let base = if masked { base.masked(true) } else { base };
    view().child(base)
}

/// 数据安全卡组入口（数据页签消费面；五卡装配）。
#[component]
pub fn DataSafetyCards() -> impl IntoElement {
    let palette = views::view_palette();
    view()
        .flex_col()
        .app_class("gap-3")
        .child(backup_cards::backup_cards(palette))
        .child(encryption_card::encryption_card(palette))
        .child(sync_card::sync_card(palette))
        .child(books_card::books_card(palette))
        .child(balance_cache_card::balance_cache_card(palette))
}

// ---------------------------------------------------------------------------
// lib 单测：装配层纯面（大小 / 时刻格式化）。headless 组件测试见
// tests/app_headless.rs 设置②节。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 大小格式化闭集：B / KB（一位小数）/ MB（一位小数）三段。
    #[test]
    fn backup_size_covers_unit_closure() {
        assert_eq!(format_backup_size(512), "512 B");
        assert_eq!(format_backup_size(2048), "2.0 KB");
        assert_eq!(format_backup_size(1536 * 1024), "1.5 MB");
    }

    /// 时刻格式化：ISO 剥到分钟（YYYY-MM-DD HH:mm）；形状不符原样透传。
    #[test]
    fn backup_time_projects_iso_minute() {
        assert_eq!(
            format_backup_time("2026-01-01T10:20:30Z"),
            "2026-01-01 10:20"
        );
        assert_eq!(format_backup_time("非ISO形状"), "非ISO形状");
    }
}
