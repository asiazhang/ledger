//! 币种管理卡（issue #2136 设置③；旧壳 `BaseCurrencySettings.vue` /
//! `DisplayCurrencySettings.vue` / `ExchangeRateSyncSettings.vue` 的重生，
//! ADR-0022 币种页签，issue #1664 币种域归口）。
//!
//! **三卡**（旧壳同构，各带作用域提示）：
//! - **本位币基准**（账本级设置）：币种字典下拉（label = 「{code} - {name}」，
//!   旧壳同形），读直调域权威读单点（缺 key 回默认 CNY）、写直调域编排入口
//!   （成员校验在域内，未知代码码化拒绝 `currency.base-invalid`）——显示值
//!   一律来自读返回（AppSettings 权威），不走本地存储（旧壳同口径）。修改后
//!   随多端同步在所有设备一致生效，是全壳折算口径的用户入口。
//! - **展示币种**（设备级轻量偏好）：本机表单预选与金额符号提示，不随同步、
//!   不参与账本折算。NSH 平台存储 [`Storage`] 键 `default_currency`（键值形
//!   态同旧壳 localStorage JSON 字符串，不与旧壳共享、不进 SQLite、不动
//!   schema——外观卡同款契约）；消费面 = 记账表单无账户缺省币种预选（
//!   [`crate::txn_form::selected_currency`] 同点消费，缺 key 回默认 CNY）。
//! - **同步汇率**（手动入口）：一键触发一次汇率同步（域编排入口 [`boot::
//!   sync_exchange_rates`]——持门编排，手动与每日自动共用在途互斥门，撞车
//!   码化错误 `fx.sync-in-progress` 就地呈现）；进行中不可重复触发（按钮禁
//!   用），阶段文字随真实进度流转（正在读取 → 携带天数的正在写入，旧壳
//!   `useFxSync` 阶段事件同口径），结果就地呈现（覆盖区间 / 条数）。
//!
//! 界面文案仅 zh-CN（旧壳 settings.appearance.currency 段 / settings.fxSync.*
//! zh-CN 词条原样承载）；码化错误 zh 原样呈现；动作反馈落卡内状态条位
//!（#2038 裁决）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_core::prelude::*;
use nsh_core::storage::Storage;

use crate::app::db_slot;
use crate::assembly::CategoryOption;
use crate::boot;
use crate::design::font_size;
use crate::select;
use crate::settings::{card, status_line};
use crate::views;

// ---------------------------------------------------------------------------
// 词表（旧壳 settings.* zh-CN 词条原样；i18n 框架级方案不在本票，字面承载）。
// ---------------------------------------------------------------------------
const CARD_BASE: &str = "本位币基准";
const BASE_HINT: &str = "账本级设置：修改后随多端同步，在所有设备一致生效，折算口径保持唯一。";
const BASE_PLACEHOLDER: &str = "选择本位币基准";
const BASE_LOAD_PREFIX: &str = "读取本位币基准失败：";
const BASE_SAVE_PREFIX: &str = "保存本位币基准失败：";
const CARD_DISPLAY: &str = "展示币种";
const DISPLAY_HINT: &str =
    "仅本设备生效：新表单的币种预选与金额符号提示；不随同步，不参与账本折算。";
const DISPLAY_PLACEHOLDER: &str = "选择展示币种";
const CARD_FX: &str = "同步汇率";
const FX_HINT: &str =
    "从欧洲央行参考汇率同步当期与历史汇率；除每日自动同步外，可在此手动触发一次。";
const FX_BUTTON: &str = "立即同步";
const FX_SYNCING: &str = "正在同步汇率…";
const FX_STAGE_FETCHING: &str = "正在读取汇率文件…";
const FX_STAGE_PERSISTING_PREFIX: &str = "已解析 ";
const FX_STAGE_PERSISTING_SUFFIX: &str = " 天，正在写入…";
const FX_OK_PREFIX: &str = "同步完成：";
const FX_OK_MIDDLE: &str = " 条汇率数据（覆盖 ";
const FX_OK_MIDDLE2: &str = " 至 ";
const FX_OK_SUFFIX: &str = "）";
const FX_FAILED_PREFIX: &str = "同步汇率失败: ";
const FX_INTERVAL_DASH: &str = "-";
const SESSION_MISSING: &str = "内部错误：账本库会话缺失";
const READ_TASK_FAILED: &str = "内部错误：读任务异常退出";

/// 展示币种存储键（旧壳 store `default_currency` 同键字面；NSH 自建平台存储
/// ——外观卡同款契约）。
const DISPLAY_CURRENCY_KEY: &str = "default_currency";
/// 展示币种出厂值（旧壳 store 出厂值同值）。
pub(crate) const DEFAULT_DISPLAY_CURRENCY: &str = "CNY";

// ---------------------------------------------------------------------------
// 设备偏好读写（NSH 自建平台存储；键值形态同旧壳 localStorage JSON 字符串）。
// ---------------------------------------------------------------------------

/// 读展示币种偏好（缺失 / 损坏 → 出厂值 CNY）——本卡初值与记账表单缺省预选
/// 的同一取值单点。
pub(crate) fn display_currency_pref() -> String {
    Storage::get_item(DISPLAY_CURRENCY_KEY)
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| DEFAULT_DISPLAY_CURRENCY.to_owned())
}

/// 写展示币种偏好（写失败不静默：告警 + 会话内仍生效，下次启动回出厂——
/// 外观卡同取舍）。
fn store_display_currency(code: &str) {
    let value = serde_json::json!(code);
    if let Err(e) = Storage::set_item(DISPLAY_CURRENCY_KEY, value.to_string()) {
        tracing::warn!(key = DISPLAY_CURRENCY_KEY, error = %e, "展示币种偏好写入存储失败");
    }
}

/// 币种条目（域行的视图侧投影：信号集合要求项可比较，域实体无 PartialEq——
/// 装配层投影既定形态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CurrencyItem {
    pub code: String,
    pub name: String,
}

/// 币种下拉选项（旧壳卡片 label = 「{code} - {name}」同形；条目序保持域读序）。
pub(crate) fn currency_card_options(rows: &[CurrencyItem]) -> Vec<CategoryOption> {
    rows.iter()
        .map(|c| CategoryOption {
            id: c.code.clone(),
            label: format!("{} - {}", c.code, c.name),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 视图（设置页「币种」页签消费面）。
// ---------------------------------------------------------------------------

/// 币种页签内容（三卡纵列，旧壳 SETTINGS_CARD_STACK 同构）。
pub(crate) fn currencies_pane(palette: views::ViewPalette) -> View {
    let owner = Owner::current();

    // 币种字典行集（三卡共用一份；装载失败诚实呈现）。
    let rows = signal(Vec::<CurrencyItem>::new());
    let load_error = signal(None::<String>);
    {
        let (rows, load_error) = (rows, load_error);
        owner.run(move || {
            spawn(async move {
                let db = db_slot().clone();
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::currency_rows(db),
                    None => Err(ledger_infra::error::AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        SESSION_MISSING,
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(data)) => {
                        rows.set(
                            data.into_iter()
                                .map(|c| CurrencyItem {
                                    code: c.code,
                                    name: c.name,
                                })
                                .collect(),
                        );
                        load_error.set(None);
                    }
                    Ok(Err(e)) => load_error.set(Some(format!("{BASE_LOAD_PREFIX}{e}"))),
                    Err(_) => load_error.set(Some(READ_TASK_FAILED.to_owned())),
                }
            });
        });
    }

    // 本位币基准（读返回权威值；变更 → 域编排写，失败回显保持原值——旧壳
    // setBaseCurrency catch 同语义）。
    let base_status = signal(None::<String>);
    let base_selected = signal(None::<String>);
    let base_persisted = signal(None::<String>);
    {
        let (base_selected, base_persisted, base_status) =
            (base_selected, base_persisted, base_status);
        owner.run(move || {
            spawn(async move {
                let db = db_slot().clone();
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::base_currency(db),
                    None => Err(ledger_infra::error::AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        SESSION_MISSING,
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(code)) => {
                        base_persisted.set(Some(code.clone()));
                        base_selected.set(Some(code));
                    }
                    Ok(Err(e)) => base_status.set(Some(format!("{BASE_LOAD_PREFIX}{e}"))),
                    Err(_) => base_status.set(Some(READ_TASK_FAILED.to_owned())),
                }
            });
        });
    }
    // 写链（与日志档位卡同形：初读同源双写不触发，选点变化才写；失败回滚
    // 显示回原值）。
    effect(move || {
        let current = base_selected.get();
        let base = base_persisted.get_untracked();
        if current.is_some() && base.is_some() && current != base {
            let target = current.clone().unwrap_or_default();
            let written_target = target.clone();
            let reset = base.clone();
            let (base_persisted, base_selected, base_status) =
                (base_persisted, base_selected, base_status);
            owner.run(move || {
                spawn(async move {
                    let db = db_slot().clone();
                    let outcome = blocking(move || match db.as_ref() {
                        Some(db) => boot::set_base_currency(db, &target),
                        None => Err(ledger_infra::error::AppError::coded(
                            boot::CODE_SESSION_MISSING,
                            SESSION_MISSING,
                        )),
                    })
                    .await;
                    match outcome {
                        Ok(Ok(())) => {
                            base_persisted.set(Some(written_target.clone()));
                            base_selected.set(Some(written_target));
                            base_status.set(None);
                        }
                        Ok(Err(e)) => {
                            base_selected.set(reset);
                            base_status.set(Some(format!("{BASE_SAVE_PREFIX}{e}")));
                        }
                        Err(_) => {
                            base_selected.set(reset);
                            base_status.set(Some(format!("{BASE_SAVE_PREFIX}{READ_TASK_FAILED}")));
                        }
                    }
                });
            });
        }
    });

    // 展示币种（设备偏好：初读同源双写不触发、选点变化才写盘——与本位币
    // 卡同形（code-review：原实现初值 Some(pref) 使 effect 初挂即写一次存
    // 储，进页即写盘与本位币卡口径不一致）；变更即落盘——旧壳
    // setDefaultCurrency 同语义，无后端往返；写失败不静默（告警 + 会话内
    // 仍生效，下次启动回出厂——外观卡同取舍）。
    let display_persisted = signal(display_currency_pref());
    let display_selected = signal(Some(display_currency_pref()));
    effect(move || {
        let current = display_selected.get();
        let persisted = display_persisted.get_untracked();
        if let Some(code) = current
            && persisted != code
        {
            store_display_currency(&code);
            display_persisted.set(code);
        }
    });

    // 同步汇率（在途单飞：进行中按钮禁用；阶段文字随真实进度流转——进度回
    // 调在引擎线程 spawn 任务内直达信号；终态收起清空，结果/错误接棒）。
    let fx_syncing = signal(false);
    let fx_stage = signal(None::<String>);
    let fx_status = signal(None::<String>);
    let run_sync = move || {
        if fx_syncing.get_untracked() {
            return; // 单飞：进行中不重入（按钮禁用第一道，本判定兜底）
        }
        fx_syncing.set(true);
        fx_status.set(Some(FX_SYNCING.to_owned()));
        fx_stage.set(None);
        let (fx_syncing, fx_stage, fx_status) = (fx_syncing, fx_stage, fx_status);
        owner.run(move || {
            spawn(async move {
                let db = db_slot().clone();
                // 阶段回调（旧壳 FX_SYNC_PROGRESS 事件面的接缝信号直投同位）。
                let stage = fx_stage;
                let outcome = match db.as_ref() {
                    Some(db) => {
                        boot::sync_exchange_rates(
                            db,
                            move |p: ledger_market_sync::FxSyncProgress| match p.stage {
                                ledger_market_sync::FxSyncStage::Fetching => {
                                    stage.set(Some(FX_STAGE_FETCHING.to_owned()));
                                }
                                ledger_market_sync::FxSyncStage::Persisting => {
                                    stage.set(Some(format!(
                                    "{FX_STAGE_PERSISTING_PREFIX}{}{FX_STAGE_PERSISTING_SUFFIX}",
                                    p.days_parsed.unwrap_or_default()
                                )));
                                }
                            },
                        )
                        .await
                    }
                    None => Err(ledger_infra::error::AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        SESSION_MISSING,
                    )),
                };
                fx_syncing.set(false);
                fx_stage.set(None);
                match outcome {
                    Ok(report) => {
                        fx_status.set(Some(format!(
                            "{FX_OK_PREFIX}{}{FX_OK_MIDDLE}{}{FX_OK_MIDDLE2}{}{FX_OK_SUFFIX}",
                            report.persist.points,
                            report
                                .persist
                                .earliest
                                .as_deref()
                                .unwrap_or(FX_INTERVAL_DASH),
                            report.persist.latest.as_deref().unwrap_or(FX_INTERVAL_DASH),
                        )));
                    }
                    Err(e) => fx_status.set(Some(format!("{FX_FAILED_PREFIX}{e}"))),
                }
            });
        });
    };

    let options = computed(move || currency_card_options(&rows.get()));

    let base_card = card(
        CARD_BASE,
        palette,
        vec![
            view()
                .flex_row()
                .app_class("items-center gap-2")
                .child(select::app_select(
                    "base-currency",
                    move || options.get(),
                    base_selected,
                    BASE_PLACEHOLDER,
                    false,
                )),
            view().child(
                text(BASE_HINT)
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.muted),
            ),
            view().child(status_line("base-currency-status", base_status, palette)),
        ],
    );

    let display_card = card(
        CARD_DISPLAY,
        palette,
        vec![
            view()
                .flex_row()
                .app_class("items-center gap-2")
                .child(select::app_select(
                    "display-currency",
                    move || options.get(),
                    display_selected,
                    DISPLAY_PLACEHOLDER,
                    false,
                )),
            view().child(
                text(DISPLAY_HINT)
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.muted),
            ),
        ],
    );

    let fx_card = card(
        CARD_FX,
        palette,
        vec![
            view().child(
                text(FX_HINT)
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.muted),
            ),
            view().flex_row().app_class("items-center gap-2").child(
                nsh_base::button()
                    .app_class(
                        "px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90 disabled:opacity-40",
                    )
                    .disabled(move || fx_syncing.get())
                    .on_press(move |_: &()| run_sync())
                    .child(
                        text(FX_BUTTON)
                            .text_size(px(font_size::SECONDARY))
                            .test_id("fx-sync-now"),
                    ),
            ),
            view().child(
                when(
                    move || fx_stage.get().is_some(),
                    move || {
                        text(move || fx_stage.get().unwrap_or_default())
                            .text_size(px(font_size::CAPTION))
                            .text_color(palette.muted)
                            .test_id("fx-sync-stage")
                    },
                )
                .otherwise(view),
            ),
            view().child(status_line("fx-sync-status", fx_status, palette)),
        ],
    );

    view()
        .flex_col()
        .app_class("gap-3")
        .child(base_card)
        .child(display_card)
        .child(fx_card)
        .child(
            when(
                move || load_error.get().is_some(),
                move || {
                    text(move || load_error.get().unwrap_or_default())
                        .text_size(px(font_size::CAPTION))
                        .text_color(palette.danger_fg)
                        .test_id("currency-load-error")
                },
            )
            .otherwise(view),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 币种下拉选项（旧壳卡片 label = 「{code} - {name}」同形；条目序保持
    /// 域读序）。
    #[test]
    fn currency_card_options_use_code_dash_name_labels_in_domain_order() {
        let rows = vec![
            CurrencyItem {
                code: "CNY".to_owned(),
                name: "人民币".to_owned(),
            },
            CurrencyItem {
                code: "USD".to_owned(),
                name: "美元".to_owned(),
            },
        ];
        let options = currency_card_options(&rows);
        assert_eq!(
            options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(),
            ["CNY - 人民币", "USD - 美元"]
        );
        assert_eq!(options[0].id, "CNY");
    }

    /// 词表锁定（旧壳 settings.* zh-CN 词条原样承载）。
    #[test]
    fn word_list_mirrors_old_shell_zh_cn() {
        assert_eq!(CARD_BASE, "本位币基准");
        assert_eq!(CARD_DISPLAY, "展示币种");
        assert_eq!(CARD_FX, "同步汇率");
        assert_eq!(FX_BUTTON, "立即同步");
        assert_eq!(
            DEFAULT_DISPLAY_CURRENCY, "CNY",
            "展示币种出厂值同旧壳 store"
        );
        assert_eq!(
            DEFAULT_DISPLAY_CURRENCY,
            ledger_currencies::DEFAULT_BASE_CURRENCY,
            "出厂展示币种与本位币基准默认同值"
        );
    }
}
