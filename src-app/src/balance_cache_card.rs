//! 余额缓存审计修复卡（数据页签卡组末位；issue #2136 设置③·数据修复，
//! 旧壳 `BalanceCacheSettings.vue` 的重生，ADR-0067 决策 5「手动审计兑底」
//! 的界面入口，#2167 自 [`crate::settings_data`] 按职责拆出）：账户余额与
//! 净资产读持久化缓存（V017），缓存缺失按裁决报码化错误、不静默回退实时
//! 计算——修复动作不能藏在读路径里，只能由此处显式触发。修复语义全在域
//! 内单正文（[`ledger_accounts::audit_balance_cache`]：全账户实时重算 vs
//! 缓存逐户比对 → 整体重算回写 → 返回差异快照，幂等可重复），本卡只触发
//! 命令与呈现报告，不持第二份口径。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

// ---------------------------------------------------------------------------
// 余额缓存审计修复卡（issue #2136 设置③·数据修复；旧壳 BalanceCacheSettings.vue
// 的重生，ADR-0067 决策 5「手动审计兑底」的界面入口）：账户余额与净资产读
// 持久化缓存（V017），缓存缺失按裁决报码化错误、不静默回退实时计算——修复
// 动作不能藏在读路径里，只能由此处显式触发。修复语义全在域内单正文
//（[`ledger_accounts::audit_balance_cache`]：全账户实时重算 vs 缓存逐户比对
// → 整体重算回写 → 返回差异快照，幂等可重复），本卡只触发命令与呈现报告，
// 不持第二份口径。
// ---------------------------------------------------------------------------

use nsh_core::prelude::*;

use crate::design::font_size;
use crate::settings::{card, status_line};
use crate::shell_msgs::MSG_WRITE_TASK_FAILED;
use crate::write_load::session_missing_err;
use crate::{boot, views};

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
pub(crate) fn balance_cache_card(palette: views::ViewPalette) -> View {
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
// lib 单测：修复报告投影。headless 组件测试见 tests/app_headless.rs 设置③节。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
