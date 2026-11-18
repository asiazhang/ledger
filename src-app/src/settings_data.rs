//! 数据安全与连接卡宿主（issue #2054 NSH 迁移㉓；旧壳 `EncryptionSettings` /
//! `SyncSettings` + `useSyncCard` / `useBookSwitcher` 的重生，ADR-0022 数据
//! 页签 / ADR-0078 危险确认 / ADR-0091 多端同步 / ADR-0114 跨账本）。
//!
//! #2167 拆分：备份管理五卡（`BackupSettings` 全量面，超越 #2038 最小入
//! 口——动作 / 目录 / 自动 / 列表 / 恢复卡）已按职责归位
//! [`crate::backup_cards`]；本模块收敛为数据页签装配与卡间共用件。
//!
//! **卡片组**（挂 [`crate::settings`] 数据页签的卡片宿主与状态条位，容器面
//! 单点不变， [`DataSafetyCards`] 为入口组件）：
//! - **加密**（`EncryptionSettings` 全量面，#2088 写面平价）：明文库形态 =
//!   warning 提示 + 主口令两框 + ≥8 位 / 一致守卫 + error 级无后门确认；密
//!   文会话形态（已开启 + 修改主口令 / 关闭加密 / 自动解锁注记）：修改主
//!   口令 error 级确认（旧壳同分级）、关闭加密 warning 级确认（破坏性但
//!   有兑底，旧壳同分级）、自动解锁按 Linux 平台不支持在屏注记（旧壳非
//!   macOS 隐藏选项的同款平价）。三形态转换完成后原位换连本会话继续可用
//!   （旧壳「应用自动重启」的本壳替代口径，无重启丢写窗口）；口令强度实
//!   时条豁免留痕（zxcvbn Rust 依赖的字典体积对单二进制的增量，守卫闭集
//!   ≥8 位 / 两次一致保留，CHANGELOG 偏差条目）。
//! - **多端同步**（`useSyncCard`）：明文警示 / 状态回显（上次同步 / 挂起
//!   数量与明细）、立即同步（成功 → 轮次报告反馈 + 失效信号广播）、通道配
//!   置表单（S3 七字段 + 同步空间 + 厂商预设预填 / 端点反查回显 + 密钥不回
//!   显口径）、保存前「测试连接」与检查点发布 / 新端引导向导。同步写语义
//!   不经本壳（轮次 / 发布 / 引导全在域内单点编排，本壳只解包口令与触发，
//!   票面实施决策）；不依赖同步协议 crate（壳只依赖域，ADR-0112）。
//! - **账本切换**（`useBookSwitcher`）：清单卡（跨账本汇总**置顶入口** +
//!   行按压切换〔warning 级轻量确认〕+ 行级改名弹窗 / 移除气泡确认 + 新建
//!   弹窗），清单读注册表最新落盘态；切换写活动指针，本壳无原位重引导
//!   （旧壳 restart_app 自动重载的偏差不在屏披露，反馈句与侧栏 #2129
//!   干净变体同文）。
//!
//! 动作反馈落卡内状态条位（#2038 裁决，NSH 无 toast）；阻塞 IO（加密转
//! 换、同步网络段、注册表读写）一律 `blocking` 后台执行；信号读写都在引
//! 擎 UI 线程。界面文案仅 zh-CN（旧壳 settings.* / books.* zh-CN 词条原样
//! 承载）；码化错误的中文原样呈现。跨卡共用件两族住本模块：呈现件（密文
//! 输入行 [`pass_input`] / 表单字段行 [`field_input`]）与装配纯函数（备份
//! 产物大小 / 时刻格式化 [`format_backup_size`] / [`format_backup_time`]，
//! [`crate::backup_cards`] 与同步卡共用）。
//!
//! 弹窗条件段机制选型（#2146 定案，行为零变化）：**布尔有无 / 分支翻转 →
//! `when`**（nsh-core `flow.rs` `Region::If`：条件翻转入支重造、离支销毁，
//! 分支闭包 untracked 求值，内容活性靠 reactive 绑定逐次求值）；**多态内容
//! 匹配或整段重造 → `dynamic`**（`Region::Dynamic`：读什么变什么重造）。
//! 全部弹窗条件段按此归位（各卡弹窗各族段均为布尔有无 → `when` 同款；
//! 恢复确认弹窗的机制注记随备份卡住 [`crate::backup_cards`]。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use crate::backup_cards;

use nsh_base::input;
use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::design::font_size;
use crate::books_card;
use crate::encryption_card;
use crate::settings::{card, status_line};
use crate::sync_card;
use crate::shell_msgs::MSG_WRITE_TASK_FAILED;
use crate::write_load::session_missing_err;
use crate::{boot, views};




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



/// 数据安全四卡入口（数据页签消费面）。
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
        .child(balance_cache_card(palette))
}

// ---------------------------------------------------------------------------
// 余额缓存审计修复卡（issue #2136 设置③·数据修复；旧壳 BalanceCacheSettings.vue
// 的重生，ADR-0067 决策 5「手动审计兑底」的界面入口）：账户余额与净资产读
// 持久化缓存（V017），缓存缺失按裁决报码化错误、不静默回退实时计算——修复
// 动作不能藏在读路径里，只能由此处显式触发。修复语义全在域内单正文
//（[`ledger_accounts::audit_balance_cache`]：全账户实时重算 vs 缓存逐户比对
// → 整体重算回写 → 返回差异快照，幂等可重复），本卡只触发命令与呈现报告，
// 不持第二份口径。
// ---------------------------------------------------------------------------

const CARD_BALANCE_CACHE: &str = "账户余额缓存";
const BALANCE_HINT: &str = "账户余额与净资产读的是持久化缓存。缓存行缺失或与实时重算不一致时，相关页面会直接报错而不回退实时计算。一键修复会逐户比对并整体重算回写，返回修复前的差异快照，可重复执行。";
const BALANCE_REPAIR: &str = "一键修复";
const BALANCE_REPORT_DONE_TITLE_PREFIX: &str = "修复完成：已校准 ";
const BALANCE_REPORT_DONE_TITLE_SUFFIX: &str = " 个账户";
const BALANCE_REPORT_DONE_BODY: &str = "差异账户已按实时口径整体重算回写，余额与净资产恢复一致。";
const BALANCE_REPORT_NOOP_TITLE: &str = "无需修复";
const BALANCE_REPORT_NOOP_BODY_PREFIX: &str = "全部 ";
const BALANCE_REPORT_NOOP_BODY_SUFFIX: &str = " 个账户的缓存与实时重算一致，未做改动。";
const BALANCE_MISSING_TAG: &str = "（缓存行缺失）";
const BALANCE_DRIFT_COLUMNS: [&str; 3] = ["账户", "修复前缓存", "实时重算"];
const BALANCE_REPAIR_FAILED_PREFIX: &str = "修复失败: ";

/// 修复报告的呈现投影（旧壳 reportView computed 同口径：有漂移 → 已修复
///（列出逐户差异）；零漂移 → 无需修复。命令返回的报告恒为「修复已完成后的
/// 差异快照」，不存在「未修复」态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BalanceCacheReportView {
    /// 横幅标题。
    pub title: String,
    /// 横幅正文。
    pub body: String,
    /// 逐户差异行（None = 零漂移，表格不呈现）。
    pub drifts: Option<Vec<(String, String, String)>>,
}

/// 域审计报告 → 呈现投影（差异行的三列文本在装配期定格：账户名 / 缓存值
///（缺失独立文案，不显示成 0）/ 实时重算值〔按账户币种格式化，币种反查
/// 自同批读的账户行集，未收录退化为无币种格式化——旧壳 reference 未收录
/// 同口径〕）。渲染期零格式化。
pub(crate) fn balance_cache_report_view(
    report: &ledger_accounts::BalanceCacheAudit,
    currencies: &crate::assembly::CurrencyTable,
    currency_of: &std::collections::BTreeMap<String, String>,
) -> BalanceCacheReportView {
    if !report.drifts.is_empty() {
        let drifts = report
            .drifts
            .iter()
            .map(|drift| {
                let code = currency_of
                    .get(&drift.account_id)
                    .cloned()
                    .unwrap_or_default();
                let fmt = currencies.format_of(&code);
                let cached = drift
                    .cached_cents
                    .map(|cents| crate::assembly::format_amount_basic(cents, &fmt))
                    .unwrap_or_else(|| BALANCE_MISSING_TAG.to_owned());
                (
                    drift.account_name.clone(),
                    cached,
                    crate::assembly::format_amount_basic(drift.actual_cents, &fmt),
                )
            })
            .collect();
        BalanceCacheReportView {
            title: format!(
                "{BALANCE_REPORT_DONE_TITLE_PREFIX}{}{BALANCE_REPORT_DONE_TITLE_SUFFIX}",
                report.drifts.len()
            ),
            body: BALANCE_REPORT_DONE_BODY.to_owned(),
            drifts: Some(drifts),
        }
    } else {
        BalanceCacheReportView {
            title: BALANCE_REPORT_NOOP_TITLE.to_owned(),
            body: format!(
                "{BALANCE_REPORT_NOOP_BODY_PREFIX}{}{BALANCE_REPORT_NOOP_BODY_SUFFIX}",
                report.accounts_checked
            ),
            drifts: None,
        }
    }
}

/// 余额缓存审计修复卡（数据页签卡组末位）。
fn balance_cache_card(palette: views::ViewPalette) -> View {
    let owner = Owner::current();
    let report = signal(None::<BalanceCacheReportView>);
    let status = signal(None::<String>);
    let repairing = signal(false);

    // 一键修复（幂等可重复；报告就地覆盖呈现）。删触发接线（boot 审计调用
    // 或卡内任务）→ 报告永不出现 → headless 旅程红。
    let run_repair = move || {
        if repairing.get_untracked() {
            return; // 单飞兜底
        }
        repairing.set(true);
        status.set(None);
        owner.run(move || {
            spawn(async move {
                let db = crate::app::db_slot().clone();
                // 参考币种行集与审计同批读（差异行金额格式化的币种反查源；
                // 读失败退化为无币种格式化——呈现面降级，修复本身不受影响）。
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => {
                        let currencies = crate::assembly::CurrencyTable::from_currencies(
                            crate::boot::currency_rows(db).unwrap_or_default().iter(),
                        );
                        // 差异行金额格式化的账户币种反查源（读失败退化为无
                        // 币种格式化——呈现面降级，修复本身不受影响）。
                        let currency_of = crate::boot::account_rows(db)
                            .unwrap_or_default()
                            .into_iter()
                            .map(|a| (a.id, a.currency_code))
                            .collect::<std::collections::BTreeMap<String, String>>();
                        boot::audit_balance_cache(db).map(|audit| {
                            balance_cache_report_view(&audit, &currencies, &currency_of)
                        })
                    }
                    None => Err(session_missing_err()),
                })
                .await;
                repairing.set(false);
                match outcome {
                    Ok(Ok(view)) => {
                        let done = view.title.clone();
                        report.set(Some(view));
                        status.set(Some(done));
                    }
                    Ok(Err(e)) => status.set(Some(format!("{BALANCE_REPAIR_FAILED_PREFIX}{e}"))),
                    Err(_) => status.set(Some(format!(
                        "{BALANCE_REPAIR_FAILED_PREFIX}{MSG_WRITE_TASK_FAILED}"
                    ))),
                }
            });
        });
    };

    // 报告横幅（有报告时在屏；错误优先呈现于状态条位，报告保持上次成功面）。
    let report_row = view().child(
        when(
            move || report.get().is_some(),
            move || {
                let snapshot = report
                    .get_untracked()
                    .unwrap_or_else(|| BalanceCacheReportView {
                        title: String::new(),
                        body: String::new(),
                        drifts: None,
                    });
                let mut block = view()
                    .flex_col()
                    .app_class("rounded-sm border border-border bg-accent p-2 gap-1")
                    .child(
                        text(snapshot.title)
                            .text_size(px(font_size::CAPTION))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.fg)
                            .test_id("balance-repair-report"),
                    )
                    .child(
                        text(snapshot.body)
                            .text_size(px(font_size::CAPTION))
                            .text_color(palette.muted)
                            .test_id("balance-repair-body"),
                    );
                // 逐户差异表（零漂移不呈现）。
                if let Some(drifts) = snapshot.drifts
                    && !drifts.is_empty()
                {
                    let mut table = view()
                        .flex_col()
                        .app_class("gap-0.5")
                        .test_id("balance-cache-drifts")
                        .child(view().flex_row().app_class("gap-3").children(
                            BALANCE_DRIFT_COLUMNS.map(|title| {
                                text(title)
                                    .text_size(px(font_size::CAPTION))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette.fg)
                            }),
                        ));
                    for (account, cached, actual) in drifts {
                        table = table.child(
                            view()
                                .flex_row()
                                .app_class("gap-3")
                                .child(
                                    text(account)
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(palette.fg),
                                )
                                .child(
                                    text(cached)
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(palette.muted),
                                )
                                .child(
                                    text(actual)
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(palette.fg),
                                ),
                        );
                    }
                    block = block.child(table);
                }
                block
            },
        )
        .otherwise(view),
    );

    card(
        CARD_BALANCE_CACHE,
        palette,
        vec![
            view().child(
                text(BALANCE_HINT)
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.muted),
            ),
            report_row,
            view().flex_row().app_class("items-center gap-2").child(
                nsh_base::button()
                    .app_class(
                        "px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90 disabled:opacity-40",
                    )
                    .disabled(move || repairing.get())
                    .on_press(move |_: &()| run_repair())
                    .child(
                        text(BALANCE_REPAIR)
                            .text_size(px(font_size::SECONDARY))
                            .test_id("balance-repair"),
                    ),
            ),
            view().child(status_line("balance-repair-status", status, palette)),
        ],
    )
}

// ---------------------------------------------------------------------------
// lib 单测：装配层纯面（大小 / 时刻格式化、来源 / 设备词表、厂商预设闭集
// 与反查）。headless 组件测试见 tests/app_headless.rs 设置②节。
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

    /// 修复报告投影：有漂移 → 「已修复」横幅 + 逐户差异行（金额按账户币种
    /// 格式化）；零漂移 → 「无需修复」横幅、无差异表。
    #[test]
    fn balance_cache_report_view_projects_done_and_noop() {
        let currencies = crate::assembly::CurrencyTable::from_currencies(
            [ledger_currencies::Currency {
                code: "CNY".to_owned(),
                name: "人民币".to_owned(),
                symbol: "¥".to_owned(),
                decimal_places: 2,
            }]
            .iter(),
        );
        let mut currency_of = std::collections::BTreeMap::new();
        currency_of.insert("acc-1".to_owned(), "CNY".to_owned());

        let drift_report = ledger_accounts::BalanceCacheAudit {
            accounts_checked: 3,
            repaired: true,
            drifts: vec![ledger_accounts::BalanceCacheDrift {
                account_id: "acc-1".to_owned(),
                account_name: "现金".to_owned(),
                cached_cents: Some(5_000),
                actual_cents: 8_000,
            }],
        };
        let view = balance_cache_report_view(&drift_report, &currencies, &currency_of);
        assert_eq!(view.title, "修复完成：已校准 1 个账户");
        let drifts = view.drifts.unwrap();
        assert_eq!(
            drifts[0],
            ("现金".to_owned(), "¥50".to_owned(), "¥80".to_owned())
        );

        let noop_report = ledger_accounts::BalanceCacheAudit {
            accounts_checked: 2,
            repaired: false,
            drifts: Vec::new(),
        };
        let view = balance_cache_report_view(&noop_report, &currencies, &currency_of);
        assert_eq!(view.title, BALANCE_REPORT_NOOP_TITLE);
        assert!(view.drifts.is_none());
        assert!(view.body.contains("2 个账户"));
    }



    /// 差异行的金额列：缓存缺失（None）独立文案呈现，不显示成 0（旧壳
    /// cachedText 同口径）；账户币种未收录退化为无币种格式化。
    #[test]
    fn drift_rows_project_missing_cache_and_unlisted_currency() {
        let currencies = crate::assembly::CurrencyTable::default();
        let currency_of = std::collections::BTreeMap::new();
        let report = ledger_accounts::BalanceCacheAudit {
            accounts_checked: 1,
            repaired: true,
            drifts: vec![ledger_accounts::BalanceCacheDrift {
                account_id: "acc-x".to_owned(),
                account_name: "孤儿账户".to_owned(),
                cached_cents: None,
                actual_cents: 1_234,
            }],
        };
        let view = balance_cache_report_view(&report, &currencies, &currency_of);
        let drifts = view.drifts.unwrap();
        assert_eq!(
            drifts[0].1, BALANCE_MISSING_TAG,
            "缺失列独立文案，不显示成 0"
        );
        assert_eq!(drifts[0].2, "12.34", "未收录币种退化为基础格式化（无币符）");
    }
}
