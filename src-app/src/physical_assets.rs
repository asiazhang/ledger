// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
//（含 src/** 内 #[cfg(test)] 模块与外挂 tests.rs）经 crate 根 cfg(test) 整体
// 放行，生产构建零放宽。

//! 实物资产视图（issue #2049 NSH 迁移⑱；旧壳 `PhysicalAssetsView.vue` +
//! `src/physical-asset/` 组件树的重生）：实物资产清单（在持合计卡 + 在持/
//! 已处置状态筛选 + 名称/当前估值折本位币/估值日期/状态/处置信息）+ 新建
//! 与编辑共表单弹层（编辑态估值字段结构性排除）+ 更新估值弹层（追加历史
//! 行）+ 处置弹层（状态标记非删除，处置日期必填、处置价与币种成对可选纯
//! 记录）+ 软删轻量确认气泡。
//!
//! 职责边界（按旧壳核实平价）：合计卡（后端同源折算快照，本壳零二次聚合
//! ——「家底合计」恒算在持，回看已处置时段合计不随筛选变化，域读路径单点
//! 产出）+ 清单卡（装配产物渲染、零新增列）+ 表单弹层（建档估值必填即首
//! 条估值历史行；购买价与币种成对——清购买价即清币种，不产生只有币种的
//! 半挂状态）+ 处置与删除是界面上分离的两个动作（处置 = 状态标记非删除
//! 且仅限在持行、删除 = 软删，ADR-0064 决策；估值 / 处置日期拒绝未来、处
//! 置不早于购买等日期守卫全在域写入口单正文，本壳不自建语义）。写路径直
//! 调域公开写入口（不经 IPC，ADR-0145 决策 4），成功后广播交易失效信号
//!（实物资产估值进净资产第三腿——概览净资产卡订阅重读；旧壳 ledger:changed
//! 粗粒度语义的壳内对应物；广播接线删除即红钉在 headless 装载旅程）；失
//! 效信号订阅反方向：账本数据变化（含本壳写入口广播）重读清单与合计（旧
//! 壳 physicalAssetsStore `ledger:changed` 重拉平价）。
//!
//! 界面文案仅 zh-CN（旧壳 physicalAssets.* zh-CN 词条原样字面承载）；码化
//! 错误 zh 原样呈现；视图本地快照、离开再回 = 全新装载（旧壳 onMounted 同
//! 口径）；无 toast，动作反馈落状态条位（#2038 裁决）。来源跳转无实物资产
//! 落点（旧壳 KIND_TARGETS 闭集不含），本视图不消费 focus 意图、无高亮面
//!（占位壳的意图消费排除表随迁入更新，views.rs）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use ledger_infra::error::AppError;
use ledger_physical_asset::{
    PhysicalAsset, PhysicalAssetDisposeInput, PhysicalAssetInput, PhysicalAssetStatus,
    PhysicalAssetUpdateInput, PhysicalAssetValuationInput,
};
use nsh_base::{DataTableAlign, DataTableColumn, DataTableRows, button, data_table, input};
use nsh_core::element::AnyElement;
use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::adaptive;
use crate::app::db_slot;
use crate::assembly::{self, ColumnAlign, CurrencyTable, PHYSICAL_ASSET_COLUMNS};
use crate::boot;
use crate::date_select;
use crate::design::font_size;
use crate::modal;
use crate::nav_model::ViewName;
use crate::popconfirm;
use crate::select;
use crate::transactions::table_classes;
use crate::views::field_label;
use crate::{nav, signals};

// ---------------------------------------------------------------------------
// 词表（旧壳 physicalAssets.* zh-CN 词条原样；i18n 框架级方案不在本票，字面
// 承载）。
// ---------------------------------------------------------------------------
const NEW_BUTTON: &str = "新建资产";
const HOLDING_TOTAL: &str = "在持估值合计";
const LIST_CARD_TITLE: &str = "资产列表";
const EMPTY_GUIDE: &str = "还没有实物资产，点击「新建资产」为你的大件家当建立估值档案";
const STATUS_HOLDING: &str = "在持";
const STATUS_DISPOSED: &str = "已处置";
const ROW_UPDATE_VALUATION: &str = "更新估值";
const ROW_EDIT: &str = "编辑";
const ROW_DISPOSE: &str = "处置";
const ROW_DELETE: &str = "删除";
const ROW_CANCEL: &str = "取消";
const SAVE_LABEL: &str = "保存";
const DELETE_CONFIRM: &str = "删除后资产将退出列表与合计，数据与估值历史保留（软删除），确认删除？";
const MSG_CREATED: &str = "建档成功";
const MSG_SAVED: &str = "保存成功";
const MSG_VALUATION_UPDATED: &str = "估值已更新";
const MSG_DISPOSED: &str = "已处置";
const MSG_DELETED: &str = "已删除";
const MSG_SAVE_FAILED: &str = "保存失败";
const MSG_DELETE_FAILED: &str = "删除失败";
const FORM_TITLE_CREATE: &str = "新建实物资产";
const FORM_TITLE_EDIT: &str = "编辑实物资产";
const LABEL_NAME: &str = "名称";
const LABEL_VALUATION: &str = "当前估值";
const LABEL_PURCHASE_DATE: &str = "购买日期";
const LABEL_PURCHASE_PRICE: &str = "购买价";
const PLACEHOLDER_NAME: &str = "如：房子、车子、钢琴";
const PLACEHOLDER_VALUATION: &str = "如 800000";
const PLACEHOLDER_PURCHASE_PRICE: &str = "如 120000";
const PLACEHOLDER_CURRENCY: &str = "币种";
const MSG_NAME_REQUIRED: &str = "请输入资产名称";
const MSG_VALUATION_REQUIRED: &str = "请填写当前估值";
const MSG_VALUATION_INVALID: &str = "当前估值必须大于 0";
const MSG_VALUATION_CURRENCY_REQUIRED: &str = "请选择估值币种";
const MSG_PURCHASE_INVALID: &str = "购买价必须大于 0";
const MSG_PURCHASE_CURRENCY_REQUIRED: &str = "请选择购买币种";
const VALUATION_TITLE: &str = "更新估值";
const VALUATION_AMOUNT_LABEL: &str = "估值金额";
const VALUATION_AMOUNT_PLACEHOLDER: &str = "如 800000";
const VALUATION_DATE_LABEL: &str = "估值日期";
const VALUATION_DATE_HINT: &str = "留空 = 今天；每次更新追加一条历史，旧值保留";
const VALUATION_AMOUNT_REQUIRED: &str = "请填写估值金额";
const VALUATION_AMOUNT_INVALID: &str = "估值金额必须大于 0";
const VALUATION_CURRENCY_REQUIRED: &str = "请选择估值币种";
const DISPOSE_TITLE: &str = "处置资产";
const DISPOSE_DATE_LABEL: &str = "处置日期";
const DISPOSE_PRICE_LABEL: &str = "处置价（可选）";
const DISPOSE_PRICE_PLACEHOLDER: &str = "如 600000";
const DISPOSE_DATE_HINT: &str =
    "处置后退出在持列表与合计，档案保留可经「已处置」筛选回看；处置价是纯记录，不进任何金额口径";
const DISPOSE_PRICE_INVALID: &str = "处置价必须大于 0";
const DISPOSE_CURRENCY_REQUIRED: &str = "请选择处置币种";

/// 装载占位与任务缺省错误文案（旧壳 loading / 异常分支同口径）。
const LOADING_PLACEHOLDER: &str = "正在加载…";
const READ_TASK_FAILED: &str = "内部错误：读任务异常退出";
const WRITE_TASK_FAILED: &str = "内部错误：写入任务异常退出";

/// 语义令牌单取（success 非三件套成员，不经 ViewPalette）：视图层零本地
/// 色常量，令牌缺失统一落 `crate::theme` 的可读兜底（ADR-0147 决策 1）。
fn success_of() -> AdaptiveColor {
    crate::theme::color("success")
}

// ---------------------------------------------------------------------------
// 快照与流形状
// ---------------------------------------------------------------------------

/// 实物资产清单快照容器（视图信号持有用）：清单 + 在持合计 + 本位币基准的
/// 同源快照（旧壳 store 抓取三信号的合并，本壳单点）。等值面 = 行集逐行
/// id + updated_at + version + 合计与基准币种（呈现等值即反复重写不通知，
/// 保单快照容器同形）。
#[derive(Debug, Clone, Default)]
struct PhysicalAssetsSnap {
    assets: Vec<PhysicalAsset>,
    holding_total_native_cents: i64,
    native_currency: String,
}

impl PartialEq for PhysicalAssetsSnap {
    fn eq(&self, other: &Self) -> bool {
        self.holding_total_native_cents == other.holding_total_native_cents
            && self.native_currency == other.native_currency
            && self.assets.len() == other.assets.len()
            && self.assets.iter().zip(other.assets.iter()).all(|(a, b)| {
                a.id == b.id && a.updated_at == b.updated_at && a.version == b.version
            })
    }
}

impl PhysicalAssetsSnap {
    /// 域列表读返回 → 快照（旧壳 store 抓取面的具名单落点）。
    fn from_dom(list: ledger_physical_asset::PhysicalAssetList) -> Self {
        Self {
            assets: list.assets,
            holding_total_native_cents: list.holding_total_native_cents,
            native_currency: list.native_currency,
        }
    }
}

/// 币种字典快照容器（视图信号持有用）：`ledger_currencies::Currency` 无
/// `PartialEq`，等值面 = code + name + symbol + 小数位（下拉 / 格式化消费
/// 面同源，保单先例同形）。
#[derive(Debug, Clone, Default)]
struct CurrenciesSnap(Vec<ledger_currencies::Currency>);
impl PartialEq for CurrenciesSnap {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().zip(other.0.iter()).all(|(a, b)| {
                a.code == b.code
                    && a.name == b.name
                    && a.symbol == b.symbol
                    && a.decimal_places == b.decimal_places
            })
    }
}

// ---------------------------------------------------------------------------
// 静态弹窗小件（保单视图同款）
// ---------------------------------------------------------------------------

/// 弹窗内「标签 + 值」行为件（旧壳 NFormItem label-placement left 同布局）。
fn form_row(label: &'static str, muted: AdaptiveColor, value: impl IntoElement) -> View {
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .child(field_label(label, muted))
        .child(value)
}

/// 弹窗错误条位（拦截面线，下次尝试开始清空；码化错误 zh 原样呈现不加二级
/// 前缀）。
fn error_line(
    show: impl Fn() -> bool + 'static,
    text_of: impl Fn() -> String + 'static,
    color: AdaptiveColor,
    id: &'static str,
) -> impl IntoElement {
    when(show, move || {
        text(text_of())
            .text_size(px(font_size::CAPTION))
            .text_color(color)
            .test_id(id)
    })
    .otherwise(view)
}

/// 弹窗动作按钮行（取消在前、主按钮殿后——保单先例同款；id 拼出
/// `{id}-cancel` / `{id}-save` 锚点）。
fn modal_buttons(
    id: &'static str,
    fg: AdaptiveColor,
    on_cancel: impl Fn() + 'static + Copy,
    on_confirm: impl Fn() + 'static,
) -> View {
    let cancel_id = format!("{id}-cancel");
    let confirm_id = format!("{id}-save");
    view()
        .flex_row()
        .justify_end()
        .gap_2()
        .child(
            button()
                .app_class(
                    "px-3 py-1.5 rounded-(--radius) border border-border text-foreground hover:bg-accent",
                )
                .on_press(move |_: &()| on_cancel())
                .child(
                    text(ROW_CANCEL)
                        .text_size(px(font_size::SECONDARY))
                        .text_color(fg)
                        .test_id(cancel_id.as_str()),
                ),
        )
        .child(
            button()
                .app_class("px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90")
                .on_press(move |_: &()| on_confirm())
                .child(
                    text(SAVE_LABEL)
                        .text_size(px(font_size::SECONDARY))
                        .text_color(crate::theme::color("primary-foreground"))
                        .test_id(confirm_id.as_str()),
                ),
        )
}

/// 文本输入字段（弹窗标签行）。
fn text_field(
    id: &'static str,
    label: &'static str,
    placeholder: &'static str,
    value: Signal<String>,
    width: f32,
    fg: AdaptiveColor,
) -> impl IntoElement {
    form_row(
        label,
        crate::views::view_palette().muted,
        input()
            .test_id(id)
            .value(move || value.get())
            .placeholder(placeholder)
            .optional_label(label)
            .text_color(fg)
            .w(px(width))
            .app_class(
                "bg-background rounded-(--radius) border border-border px-2 py-1 \
                 placeholder:text-muted-foreground",
            )
            .on_change(move |text: &str| value.set(text.to_owned())),
    )
}

/// 金额 + 币种信号束（旧壳 NFormItem 内成对输入的同源状态；clippy
/// too_many_arguments 口径，成对信号一个结构体收齐——保单 AgreementDraft
/// 先例同形）。
struct AmountCurrencyDraft {
    /// 金额文本（元文本，保存面换算整数分）。
    amount: Signal<String>,
    /// 币种 code（可空 = 未选；成对守卫在保存面收口）。
    currency: Signal<Option<String>>,
}

/// 金额 + 币种组行（旧壳 NFormItem 内 NInput + AppSelect 同布局）：金额输入
/// 与币种下拉成对呈现，成对守卫在保存面收口（本壳 select 封装暂无 disabled
/// 槽，保单保额先例同注）。
fn amount_currency_row(
    id: &'static str,
    label: &'static str,
    draft: AmountCurrencyDraft,
    amount_placeholder: &'static str,
    currency_options: impl Fn() -> Vec<assembly::CategoryOption> + Copy + 'static,
    width: f32,
    fg: AdaptiveColor,
) -> View {
    let AmountCurrencyDraft {
        amount: amount_signal,
        currency: currency_signal,
    } = draft;
    let currency_id = format!("{id}-currency");
    form_row(
        label,
        crate::views::view_palette().muted,
        view()
            .flex_row()
            .items_center()
            .gap_1()
            .child(
                input()
                    .test_id(id)
                    .value(move || amount_signal.get())
                    .placeholder(amount_placeholder)
                    .optional_label(label)
                    .text_color(fg)
                    .w(px(width))
                    .app_class(
                        "bg-background rounded-(--radius) border border-border px-2 py-1 \
                         placeholder:text-muted-foreground",
                    )
                    .on_change(move |text: &str| amount_signal.set(text.to_owned())),
            )
            .child(select::app_select(
                std::borrow::Cow::Owned(currency_id),
                currency_options,
                currency_signal,
                PLACEHOLDER_CURRENCY,
                false,
            )),
    )
}

/// 行内小操作按钮（外层包裹视图带按压语义 + 内层文本要锚点；nsh-base
/// Button 自含按压语义会吞外层按压——纯视图触发件先例同注）。
fn action_button(
    id: &str,
    label: &'static str,
    fg: AdaptiveColor,
    on_press: impl Fn() + 'static,
) -> View {
    view()
        .app_class("px-2 py-0.5 rounded-sm border border-border text-foreground hover:bg-accent")
        .on_press(move |_: &PressEvent| on_press())
        .child(
            text(label)
                .text_size(px(font_size::CAPTION))
                .text_color(fg)
                .test_id(id),
        )
}

// ============================================================================
// 实物资产视图（导航骨架实物资产分支的真实组件，#2049；独立路由与资产
// 「更多」收纳页签共用同一组件整体装载——旧壳 /physical-assets 与容器页签
// 同组件同形态）。
// ============================================================================
#[component]
pub(crate) fn PhysicalAssetsView() -> impl IntoElement {
    let view_owner = Owner::current();

    // ============================ 快照态 ============================
    let snapshot = signal(PhysicalAssetsSnap::default());
    let currency_rows = signal(CurrenciesSnap::default());
    let loading = signal(true);
    let error = signal(None::<String>);
    let load_seq = signal(0u64);
    let popconfirm_open = signal(None::<String>);
    let status = signal(None::<String>);

    // 状态筛选（旧壳 NRadioGroup 在持/已处置同组语义）：默认在持。
    let filter = signal(PhysicalAssetStatus::Holding);

    /// 装载批产物（读接缝单点的具名元组）。
    type AssetLoads = ledger_infra::error::Result<(
        ledger_physical_asset::PhysicalAssetList,
        Vec<ledger_currencies::Currency>,
    )>;

    // 装载批：清单（折本位币 + 在持合计同源快照）+ 币种字典，同批读取（旧
    // 壳 onMounted 的 store self-init 同口径）。竞态 latest-wins（账户先例）。
    // 筛选随重入读取现行值（切筛选即重拉，旧壳 setStatusFilter 同语义）。
    let reload = move || {
        loading.set(true);
        error.set(None);
        load_seq.update(|seq| *seq += 1);
        let seq = load_seq.get();
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let Some(db) = db else {
                    error.set(Some("内部错误：账本库会话缺失".to_owned()));
                    loading.set(false);
                    return;
                };
                let filter_text = filter.get_untracked().as_str().to_owned();
                let outcome = blocking(move || -> AssetLoads {
                    let list = boot::physical_asset_rows(&db, Some(&filter_text))?;
                    let currency_list = boot::currency_rows(&db)?;
                    Ok((list, currency_list))
                })
                .await;
                if load_seq.get() != seq {
                    return;
                }
                match outcome {
                    Ok(Ok((list, currency_list))) => {
                        snapshot.set(PhysicalAssetsSnap::from_dom(list));
                        currency_rows.set(CurrenciesSnap(currency_list));
                    }
                    Ok(Err(e)) => error.set(Some(e.to_string())),
                    Err(_) => error.set(Some(READ_TASK_FAILED.to_owned())),
                }
                loading.set(false);
            });
        });
    };

    // ============================ 落位装载与失效信号 ============================
    effect(move || {
        let _landed = nav::store().current.get();
        reload();
    });
    // 失效信号订阅（旧壳 physicalAssetsStore `ledger:changed` 失效重拉的壳
    // 内对应物）：账本数据变化（含本壳各写入口广播）→ 清单与合计重读。订
    // 阅句柄随组件实例存亡（app.rs reload_subscription 同形：直放 let 会在
    // 组件体结束时 Drop 撤销）。
    let reload_subscription = signals::subscribe_transactions_changed(reload);
    effect(move || {
        let _held_for_view_lifetime: &signals::TransactionsChangedSubscription =
            &reload_subscription;
    });

    // ============================ 主题令牌 ============================
    let crate::views::ViewPalette {
        fg,
        muted,
        danger_fg,
    } = crate::views::view_palette();

    // ============================ 行装配（渲染期零格式化） ============================
    let currencies = computed(move || CurrencyTable::from_currencies(currency_rows.get().0.iter()));
    let row_cells = computed(move || {
        let table = currencies.get();
        let snap = snapshot.get();
        assembly::assemble_physical_asset_rows(&snap.assets, &table)
    });

    // 行快照反查（编辑 / 估值 / 处置的目标）：域行快照内单点；行缺席（并
    // 发软删）动作静默退场（弹窗不呈现半态）。
    let row_of = move |asset_id: &str| -> Option<PhysicalAsset> {
        snapshot
            .get_untracked()
            .assets
            .iter()
            .find(|row| row.id == asset_id)
            .cloned()
    };

    // ============================ 写弹层状态束（同屏至多一层） ============================
    // 旧壳三弹窗（建档/编辑表单、更新估值、处置）各自 intent 工厂单开——
    // 意图闭集单弹窗语义在这里落成「三组开关信号同显至多一层」。
    let form_open = signal(false);
    let form_editing_id: Signal<Option<String>> = signal(None);
    let form_name = signal(String::new());
    let form_valuation_text = signal(String::new());
    let form_valuation_currency: Signal<Option<String>> = signal(None);
    let form_purchase_date = signal(None::<chrono::NaiveDate>);
    let form_purchase_text = signal(String::new());
    let form_purchase_currency: Signal<Option<String>> = signal(None);
    let form_error = signal(None::<String>);
    let is_edit = move || form_editing_id.get_untracked().is_some();

    let valuation_open = signal(false);
    let valuation_target: Signal<Option<String>> = signal(None);
    let valuation_amount = signal(String::new());
    let valuation_currency: Signal<Option<String>> = signal(None);
    let valuation_date = signal(chrono::Local::now().date_naive());
    let valuation_error = signal(None::<String>);

    let dispose_open = signal(false);
    let dispose_target: Signal<Option<String>> = signal(None);
    let dispose_date = signal(chrono::Local::now().date_naive());
    let dispose_price_text = signal(String::new());
    let dispose_currency: Signal<Option<String>> = signal(None);
    let dispose_error = signal(None::<String>);

    let close_form = move || form_open.set(false);
    let close_valuation = move || valuation_open.set(false);
    let close_dispose = move || dispose_open.set(false);

    // ============================ 弹窗开关（回填 / 复位） ============================
    let open_create = move || {
        form_name.set(String::new());
        form_valuation_text.set(String::new());
        form_valuation_currency.set(None);
        form_purchase_date.set(None);
        form_purchase_text.set(String::new());
        form_purchase_currency.set(None);
        form_error.set(None);
        form_editing_id.set(None);
        form_open.set(true);
    };
    let open_edit = move |asset_id: String| {
        let Some(existing) = row_of(&asset_id) else {
            return;
        };
        form_name.set(existing.name.clone());
        form_purchase_date.set(
            existing
                .purchase_date
                .as_deref()
                .and_then(date_select::parse_date_text),
        );
        form_purchase_text.set(
            existing
                .purchase_price_cents
                .map(|cents| {
                    let dp = currencies
                        .get_untracked()
                        .format_of(&existing.purchase_currency_code.clone().unwrap_or_default())
                        .decimal_places;
                    assembly::cents_to_amount_text(cents, dp)
                })
                .unwrap_or_default(),
        );
        form_purchase_currency.set(existing.purchase_currency_code.clone());
        form_error.set(None);
        form_valuation_text.set(String::new());
        form_valuation_currency.set(None);
        form_editing_id.set(Some(asset_id));
        form_open.set(true);
    };
    let open_valuation = move |asset_id: String| {
        let Some(existing) = row_of(&asset_id) else {
            return;
        };
        valuation_amount.set(String::new());
        // 币种预选当前估值币种（旧壳 currency 应回缺省币种同口径）。
        valuation_currency.set(
            (!existing.current_valuation_currency_code.is_empty())
                .then(|| existing.current_valuation_currency_code.clone()),
        );
        // 日期缺省今天（旧壳留空 = 后端同语义缺省今天；NSH date_select 受控
        // 件恒有值，物品处置先例同注）。
        valuation_date.set(chrono::Local::now().date_naive());
        valuation_error.set(None);
        valuation_target.set(Some(asset_id));
        valuation_open.set(true);
    };
    let open_dispose = move |asset_id: String| {
        let Some(existing) = row_of(&asset_id) else {
            return;
        };
        // 处置日期缺省今天（旧壳留空必填面在受控件不可达，域同守卫兜底——
        // 物品处置弹窗先例同口径；差异入交付报告对表）。
        dispose_date.set(chrono::Local::now().date_naive());
        dispose_price_text.set(String::new());
        // 币种预选当前估值币种（纯记录，预选减少手选成本，旧壳同口径）。
        dispose_currency.set(
            (!existing.current_valuation_currency_code.is_empty())
                .then(|| existing.current_valuation_currency_code.clone()),
        );
        dispose_error.set(None);
        dispose_target.set(Some(asset_id));
        dispose_open.set(true);
    };

    // ============================ 保存执行（建档 / 编辑） ============================
    // 客户端必填校验（消息与后端错误码文案同源，双保险防呆），购买价与币
    // 种成对先于写入拦截（旧壳 save 编排平价）；建档估值必填（首条估值历
    // 史行），编辑模式估值结构性排除（ADR-0064 只追加口径）。
    let perform_save = move || {
        form_error.set(None);
        status.set(None);
        let name = form_name.get_untracked().trim().to_owned();
        if name.is_empty() {
            form_error.set(Some(MSG_NAME_REQUIRED.to_owned()));
            return;
        }
        // 购买价与币种成对（填写购买价时币种必填；清购买价即清币种——旧壳
        // watch(purchaseFilled) 的保存态等价面）。
        let purchase_text = form_purchase_text.get_untracked();
        let (purchase_cents, purchase_currency_code) = if purchase_text.trim().is_empty() {
            (None, None)
        } else {
            let Some(cents) =
                assembly::budget_yuan_to_cents(&purchase_text).filter(|cents| *cents > 0)
            else {
                form_error.set(Some(MSG_PURCHASE_INVALID.to_owned()));
                return;
            };
            match form_purchase_currency.get_untracked() {
                Some(code) => (Some(cents), Some(code)),
                None => {
                    form_error.set(Some(MSG_PURCHASE_CURRENCY_REQUIRED.to_owned()));
                    return;
                }
            }
        };
        let editing = form_editing_id.get_untracked();
        let is_update = editing.is_some();
        // 建档估值（首条历史行）：金额与币种成对必填；估值日期缺省今天（旧
        // 壳同口径：估值日期不出建档表单）。
        let (valuation_cents, valuation_currency_code) = if is_update {
            (None, None)
        } else {
            let raw = form_valuation_text.get_untracked();
            if raw.trim().is_empty() {
                form_error.set(Some(MSG_VALUATION_REQUIRED.to_owned()));
                return;
            }
            let Some(cents) = assembly::budget_yuan_to_cents(&raw).filter(|cents| *cents > 0)
            else {
                form_error.set(Some(MSG_VALUATION_INVALID.to_owned()));
                return;
            };
            let Some(code) = form_valuation_currency.get_untracked() else {
                form_error.set(Some(MSG_VALUATION_CURRENCY_REQUIRED.to_owned()));
                return;
            };
            (Some(cents), Some(code))
        };
        let purchase_date = form_purchase_date.get_untracked().map(|d| d.to_string());
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => match editing.as_deref() {
                        Some(id) => boot::update_physical_asset(
                            db,
                            id,
                            PhysicalAssetUpdateInput {
                                name: name.clone(),
                                purchase_date: purchase_date.clone(),
                                purchase_price_cents: purchase_cents,
                                purchase_currency_code: purchase_currency_code.clone(),
                            },
                        ),
                        None => boot::create_physical_asset(
                            db,
                            PhysicalAssetInput {
                                name: name.clone(),
                                purchase_date: purchase_date.clone(),
                                purchase_price_cents: purchase_cents,
                                purchase_currency_code: purchase_currency_code.clone(),
                                initial_valuation_cents: valuation_cents,
                                initial_valuation_currency_code: valuation_currency_code.clone(),
                                initial_valuation_date: None,
                            },
                        )
                        .map(|_: String| ()),
                    },
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(_)) => {
                        close_form();
                        status.set(Some(if is_update {
                            MSG_SAVED.to_owned()
                        } else {
                            MSG_CREATED.to_owned()
                        }));
                        // 实物资产估值进净资产第三腿（净资产合计三腿之一）
                        // ——按旧壳 ledger:changed 粗粒度语义广播交易失效
                        // （概览净资产卡订阅重读；广播接线删除即红）。
                        signals::notify_transactions_invalidated();
                        reload();
                    }
                    Ok(Err(e)) => form_error.set(Some(format!("{MSG_SAVE_FAILED}: {e}"))),
                    Err(_) => form_error.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    // ============================ 更新估值执行（追加历史行） ============================
    let perform_valuation = move || {
        valuation_error.set(None);
        status.set(None);
        let Some(asset_id) = valuation_target.get_untracked() else {
            return;
        };
        let raw = valuation_amount.get_untracked();
        let Some(cents) = assembly::amount_text_to_cents(&raw).filter(|cents| *cents > 0) else {
            if raw.trim().is_empty() {
                valuation_error.set(Some(VALUATION_AMOUNT_REQUIRED.to_owned()));
            } else {
                valuation_error.set(Some(VALUATION_AMOUNT_INVALID.to_owned()));
            }
            return;
        };
        let Some(currency_code) = valuation_currency.get_untracked() else {
            valuation_error.set(Some(VALUATION_CURRENCY_REQUIRED.to_owned()));
            return;
        };
        let input = PhysicalAssetValuationInput {
            amount_cents: Some(cents),
            currency_code: Some(currency_code),
            // 受控件缺省今天（旧壳留空 = 后端同语义缺省；可补过去、未来由域
            // 拒绝——码化错误原样呈现）。
            valuation_date: Some(valuation_date.get_untracked().to_string()),
        };
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::update_physical_asset_valuation(db, &asset_id, input),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        close_valuation();
                        status.set(Some(MSG_VALUATION_UPDATED.to_owned()));
                        // 当前值变化进合计与净资产第三腿——广播交易失效 +
                        // 清单重读（重拉接线删除即红）。
                        signals::notify_transactions_invalidated();
                        reload();
                    }
                    Ok(Err(e)) => valuation_error.set(Some(format!("{MSG_SAVE_FAILED}: {e}"))),
                    Err(_) => valuation_error.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    // ============================ 处置执行（状态标记非删除） ============================
    let perform_dispose = move || {
        dispose_error.set(None);
        status.set(None);
        let Some(asset_id) = dispose_target.get_untracked() else {
            return;
        };
        // 处置日期必填：受控件恒有值（缺省今天）——旧壳「为空拦截」分支在
        // 受控形态不可达，录入错误由域码化错误显式拒绝（物品处置先例同注）。
        let date_text = dispose_date.get_untracked().to_string();
        // 处置价与币种成对（旧壳 priceFilled 同语义；处置价是纯记录，不进
        // 任何金额口径）。
        let price_text = dispose_price_text.get_untracked();
        let (price_cents, price_currency_code) = if price_text.trim().is_empty() {
            (None, None)
        } else {
            let Some(cents) =
                assembly::budget_yuan_to_cents(&price_text).filter(|cents| *cents > 0)
            else {
                dispose_error.set(Some(DISPOSE_PRICE_INVALID.to_owned()));
                return;
            };
            match dispose_currency.get_untracked() {
                Some(code) => (Some(cents), Some(code)),
                None => {
                    dispose_error.set(Some(DISPOSE_CURRENCY_REQUIRED.to_owned()));
                    return;
                }
            }
        };
        let input = PhysicalAssetDisposeInput {
            disposal_date: Some(date_text),
            disposal_price_cents: price_cents,
            disposal_currency_code: price_currency_code,
        };
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::dispose_physical_asset(db, &asset_id, input),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        close_dispose();
                        status.set(Some(MSG_DISPOSED.to_owned()));
                        // 已处置退出在持合计与净资产——广播交易失效 + 重读。
                        signals::notify_transactions_invalidated();
                        reload();
                    }
                    Ok(Err(e)) => dispose_error.set(Some(format!("{MSG_SAVE_FAILED}: {e}"))),
                    Err(_) => dispose_error.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    // ============================ 软删（轻量确认气泡锚定行内删除按钮） ============================
    let perform_delete = move |id: String| {
        status.set(None);
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::delete_physical_asset(db, &id),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        status.set(Some(MSG_DELETED.to_owned()));
                        signals::notify_transactions_invalidated();
                    }
                    Ok(Err(e)) => status.set(Some(format!("{MSG_DELETE_FAILED}: {e}"))),
                    Err(_) => status.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    // ============================ 表格装配 ============================
    let columns = computed(move || {
        let width = adaptive::content_width();
        physical_asset_columns(width)
    });
    let data_of = move || {
        let cells = row_cells.get();
        DataTableRows::new(cells.len())
            .texts("name", cells.iter().map(|r| r.name.as_str()))
            .texts("valuation", cells.iter().map(|r| r.valuation.as_str()))
            .texts(
                "valuation_date",
                cells.iter().map(|r| r.valuation_date.as_str()),
            )
            .texts("status", cells.iter().map(|r| r.status_label.as_str()))
            .texts("disposal", cells.iter().map(|r| r.disposal.as_str()))
            .texts("actions", cells.iter().map(|_| ""))
    };

    // 自绘格：名称列、估值列、估值日期列、处置信息列（文本原样）与状态列
    //（双态语义色——旧壳 NTag success/default 的文本面对应形态）、操作列
    //（更新估值 / 编辑 /(在持行才有的) 处置 / 删除气泡）。
    let cell_of = move |cell: &nsh_base::DataTableCell| -> AnyElement {
        let index = cell.row as usize;
        match cell.key.as_ref() {
            "status" => {
                let disposed = row_cells
                    .get_untracked()
                    .get(index)
                    .is_some_and(|row| row.status_label == STATUS_DISPOSED);
                text(cell.text.clone())
                    .text_size(px(font_size::SECONDARY))
                    .text_color(if disposed {
                        crate::views::view_palette().muted
                    } else {
                        success_of()
                    })
                    .into_element()
            }
            "actions" => {
                let rows = row_cells.get_untracked();
                let Some(cells) = rows.get(index) else {
                    return view().into_element();
                };
                let id = cells.id.clone();
                let holding = cells.status_label == STATUS_HOLDING;
                // 处置按钮锚点先于 when 闭包成形（format_args! 在嵌套 move
                // 闭包内按 move 捕获，闭包内再 clone 即冲突——先格式化后捕
                // 获借用即安）。
                let dispose_trigger_id = format!("physical-asset-dispose-{id}");
                // when 闭包是 move 闭包：体内所需的 id 先行独立副本（闭包
                // 把 cell_of 体作用域的 id move 走，后续 popconfirm 部分仍
                // 要用），闭包体内只用这份副本。
                view()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(action_button(
                        format!("physical-asset-valuation-{id}").as_str(),
                        ROW_UPDATE_VALUATION,
                        fg,
                        {
                            let id = id.clone();
                            move || open_valuation(id.clone())
                        },
                    ))
                    .child(action_button(
                        format!("physical-asset-edit-{id}").as_str(),
                        ROW_EDIT,
                        fg,
                        {
                            let id = id.clone();
                            move || open_edit(id.clone())
                        },
                    ))
                    .child(
                        // 处置入口仅限在持行（已处置已退出在持口径；旧壳
                        // row.status === "holding" 分支同语义）。处置与删除
                        // 是界面上分离的两个动作（ADR-0064）。
                        when(
                            move || holding,
                            {
                                let dispose_id = id.clone();
                                move || {
                                    action_button(
                                        dispose_trigger_id.as_str(),
                                        ROW_DISPOSE,
                                        fg,
                                        {
                                            let id = dispose_id.clone();
                                            move || open_dispose(id.clone())
                                        },
                                    )
                                }
                            },
                        )
                        .otherwise(view),
                    )
                    .child(
                        // 轻量确认气泡（ADR-0078 轻量档：旧壳 AppPopconfirm 同
                        // 形态——软删无恢复入口，确认文案把去向说清楚）。删除
                        // 即红条目：删本 popconfirm 开合接线 → headless「确认
                        // 文案永不呈现」变红。
                        popconfirm::popconfirm(
                            "physical-asset-delete",
                            {
                                let id = id.clone();
                                move || popconfirm_open.get().as_deref() == Some(id.as_str())
                            },
                            {
                                let id = id.clone();
                                move |shown: bool| {
                                    popconfirm_open.set(shown.then(|| id.clone()))
                                }
                            },
                            popconfirm::PopconfirmSpec {
                                message: DELETE_CONFIRM.to_owned(),
                                confirm_label: ROW_DELETE.to_owned(),
                                cancel_label: ROW_CANCEL.to_owned(),
                            },
                            // 触发件用纯视图（nsh-base Button 自含按压语义会
                            // 吞掉外层按压——按压源归外层包裹视图，先例同注）。
                            view()
                                .app_class(
                                    "px-2 py-0.5 rounded-sm border border-border text-foreground hover:bg-accent",
                                )
                                .child(
                                    text(ROW_DELETE)
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(fg)
                                        .test_id(format!("physical-asset-delete-{id}").as_str()),
                                ),
                            {
                                let id = id.clone();
                                move || perform_delete(id.clone())
                            },
                        ),
                    )
                    .into_element()
            }
            _ => text(cell.text.clone())
                .text_size(px(font_size::SECONDARY))
                .text_color(fg)
                .number_of_lines(1)
                .into_element(),
        }
    };

    // 状态筛选双钮（旧壳 NRadioGroup 在持/已处置同组语义；nsh-base 无
    // NRadioButton 胶囊形态，双钮 + 选中态 accent 底的双态自适应，#2038 形
    // 态对表入交付报告）。
    let filter_button = |status: PhysicalAssetStatus, id: &'static str| {
        let label = match status {
            PhysicalAssetStatus::Holding => STATUS_HOLDING,
            PhysicalAssetStatus::Disposed => STATUS_DISPOSED,
        };
        view().child(
            button()
                .selected(move || filter.get() == status)
                .app_class("px-2 py-0.5 rounded-sm selected:bg-accent hover:bg-accent")
                .on_press(move |_: &()| {
                    if filter.get_untracked() != status {
                        filter.set(status);
                        reload();
                    }
                })
                .child(
                    text(label)
                        .text_size(px(font_size::CAPTION))
                        .text_color(fg)
                        .test_id(id),
                ),
        )
    };

    // 在持合计文本（后端同源快照；金额格式化走统一 formatAmount 家族——旧
    // 壳 holdingTotalText 同口径，基准币种 = 快照 native_currency）。tracked
    // 读（get）建立依赖：快照/币种字典到达即重求值——get_untracked 不建立
    // 依赖，文本将恒停默认态（状态条位先例同形）。
    let holding_total = move || {
        let snap = snapshot.get();
        let fmt = currencies.get().format_of(&snap.native_currency);
        format!(
            "{HOLDING_TOTAL}：{}",
            assembly::format_amount_basic(snap.holding_total_native_cents, &fmt)
        )
    };

    // ============================ 表单弹层内容（静态字段 + 模式分支） ============================
    let form_content = move || {
        let editing = is_edit();
        let mut body = view().flex_col().gap_2().child(text_field(
            "physical-asset-name",
            LABEL_NAME,
            PLACEHOLDER_NAME,
            form_name,
            220.0,
            fg,
        ));
        // 编辑模式无估值字段：估值只能经「更新估值」变更（ADR-0064 只追
        // 加；旧壳 v-if 结构性排除同样式）。
        if !editing {
            body = body.child(amount_currency_row(
                "physical-asset-valuation",
                LABEL_VALUATION,
                AmountCurrencyDraft {
                    amount: form_valuation_text,
                    currency: form_valuation_currency,
                },
                PLACEHOLDER_VALUATION,
                move || currencies.get_untracked().currency_options(),
                140.0,
                fg,
            ));
        }
        body = body
            .child(form_row(
                LABEL_PURCHASE_DATE,
                muted,
                // 可选日期（旧壳 AppDatePicker clearable 同形态——可清空日
                // 期封装件随 #2056 首建就位，直接消费）。
                date_select::date_option_select("physical-asset-purchase-date", form_purchase_date),
            ))
            .child(amount_currency_row(
                "physical-asset-purchase-price",
                LABEL_PURCHASE_PRICE,
                AmountCurrencyDraft {
                    amount: form_purchase_text,
                    currency: form_purchase_currency,
                },
                PLACEHOLDER_PURCHASE_PRICE,
                move || currencies.get_untracked().currency_options(),
                140.0,
                fg,
            ))
            .child(error_line(
                move || form_error.get().is_some(),
                move || {
                    format!(
                        "{MSG_SAVE_FAILED}: {}",
                        form_error.get().unwrap_or_default()
                    )
                },
                danger_fg,
                "physical-asset-form-error",
            ))
            .child(modal_buttons(
                "physical-asset-form",
                fg,
                close_form,
                perform_save,
            ));
        body
    };

    // ============================ 更新估值弹层内容 ============================
    let valuation_content = move || {
        view()
            .flex_col()
            .gap_2()
            .child(amount_currency_row(
                "physical-asset-valuation-amount",
                VALUATION_AMOUNT_LABEL,
                AmountCurrencyDraft {
                    amount: valuation_amount,
                    currency: valuation_currency,
                },
                VALUATION_AMOUNT_PLACEHOLDER,
                move || currencies.get_untracked().currency_options(),
                140.0,
                fg,
            ))
            .child(form_row(
                VALUATION_DATE_LABEL,
                muted,
                date_select::date_select("physical-asset-valuation-date", valuation_date),
            ))
            .child(
                text(VALUATION_DATE_HINT)
                    .text_size(px(font_size::CAPTION))
                    .text_color(muted),
            )
            .child(error_line(
                move || valuation_error.get().is_some(),
                move || {
                    format!(
                        "{MSG_SAVE_FAILED}: {}",
                        valuation_error.get().unwrap_or_default()
                    )
                },
                danger_fg,
                "physical-asset-valuation-error",
            ))
            .child(modal_buttons(
                "physical-asset-valuation",
                fg,
                close_valuation,
                perform_valuation,
            ))
    };

    // ============================ 处置弹层内容 ============================
    let dispose_content = move || {
        view()
            .flex_col()
            .gap_2()
            .child(form_row(
                DISPOSE_DATE_LABEL,
                muted,
                date_select::date_select("physical-asset-dispose-date", dispose_date),
            ))
            .child(amount_currency_row(
                "physical-asset-dispose-price",
                DISPOSE_PRICE_LABEL,
                AmountCurrencyDraft {
                    amount: dispose_price_text,
                    currency: dispose_currency,
                },
                DISPOSE_PRICE_PLACEHOLDER,
                move || currencies.get_untracked().currency_options(),
                140.0,
                fg,
            ))
            .child(
                text(DISPOSE_DATE_HINT)
                    .text_size(px(font_size::CAPTION))
                    .text_color(muted),
            )
            .child(error_line(
                move || dispose_error.get().is_some(),
                move || {
                    format!(
                        "{MSG_SAVE_FAILED}: {}",
                        dispose_error.get().unwrap_or_default()
                    )
                },
                danger_fg,
                "physical-asset-dispose-error",
            ))
            .child(modal_buttons(
                "physical-asset-dispose",
                fg,
                close_dispose,
                perform_dispose,
            ))
    };

    // ============================ 视图树 ============================
    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(
            // 屏名（旧壳 page-title 同位；容器页签态与独立路由共用）。
            view().px_4().py_2().child(
                text(ViewName::PhysicalAssets.label())
                    .text_size(px(font_size::TITLE))
                    .text_color(fg)
                    .test_id("view-title"),
            ),
        )
        .child(error_line(
            move || error.get().is_some(),
            move || format!("加载失败：{}", error.get().unwrap_or_default()),
            danger_fg,
            "physical-asset-error",
        ))
        .child(
            // 合计卡（旧壳顶部 NCard 同位：合计行 + 建档入口右对齐）。
            view().px_4().child(
                view()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        text(holding_total)
                            .text_size(px(font_size::SECONDARY))
                            .text_color(fg)
                            .test_id("physical-asset-holding-total"),
                    )
                    .child(
                        button()
                            .app_class("px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90")
                            .on_press(move |_: &()| open_create())
                            .child(
                                text(NEW_BUTTON)
                                    .text_size(px(font_size::SECONDARY))
                                    .text_color(crate::theme::color("primary-foreground"))
                                    .test_id("physical-asset-new"),
                            ),
                    ),
            ),
        )
        .child(
            // 内容区（保单视图同形态的字段卡片列；表格虚拟化按自身高度裁行
            // ——根内容列必须 flex_1，卡片弹性高才有确定高来源，保单先例）。
            view()
                .flex_1()
                .flex_col()
                .gap_3()
                .p_4()
                .pt_0()
                .child(
                    // 清单卡片（卡片自身 flex_1——虚拟化表格取弹性链高度）。
                    view()
                        .flex_1()
                        .flex_col()
                        .gap_2()
                        .app_class("rounded-(--radius-card) border border-border p-3")
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(
                                    text(LIST_CARD_TITLE)
                                        .text_size(px(font_size::SECONDARY))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(fg),
                                )
                                .child(filter_button(
                                    PhysicalAssetStatus::Holding,
                                    "physical-asset-filter-holding",
                                ))
                                .child(filter_button(
                                    PhysicalAssetStatus::Disposed,
                                    "physical-asset-filter-disposed",
                                )),
                        )
                        .child(
                            view().test_id("physical-asset-table").flex_1().child(
                                data_table()
                                    .columns(columns)
                                    .data(data_of)
                                    .render_cell(cell_of)
                                    .loading(move || loading.get())
                                    .loading_view(text(LOADING_PLACEHOLDER).text_color(muted))
                                    .empty(
                                        text(EMPTY_GUIDE)
                                            .text_color(muted)
                                            .test_id("physical-asset-empty-guide"),
                                    )
                                    .classes(table_classes())
                                    .flex_1(),
                            ),
                        ),
                )
                .child(
                    // 操作反馈条位（成功/失败共用一条；新动作开始清回空）。
                    when(
                        move || status.get().is_some(),
                        move || {
                            text(move || status.get().unwrap_or_default())
                                .text_size(px(font_size::CAPTION))
                                .text_color(fg)
                                .test_id("physical-asset-status")
                        },
                    )
                    .otherwise(view),
                )
                .child(modal::app_modal(
                    move || form_open.get(),
                    move || {
                        if is_edit() {
                            FORM_TITLE_EDIT.to_owned()
                        } else {
                            FORM_TITLE_CREATE.to_owned()
                        }
                    },
                    close_form,
                    form_content,
                ))
                .child(modal::app_modal(
                    move || valuation_open.get(),
                    move || VALUATION_TITLE.to_owned(),
                    close_valuation,
                    valuation_content,
                ))
                .child(modal::app_modal(
                    move || dispose_open.get(),
                    move || DISPOSE_TITLE.to_owned(),
                    close_dispose,
                    dispose_content,
                )),
        )
}

/// 列定义 → 宿主列（UI 层只做映射，不产列；保单先例同形——装配建议值转
/// 述，富余宽度全给名称列，列宽计算不掺装配）。
fn physical_asset_columns(width: f32) -> Vec<DataTableColumn> {
    let mut columns: Vec<DataTableColumn> = PHYSICAL_ASSET_COLUMNS
        .iter()
        .map(|def| {
            let mut column = DataTableColumn::default();
            column.key = def.key.into();
            column.title = Some(def.title.into());
            column.width = Some(Points(def.width));
            column.align = match def.align {
                ColumnAlign::Start => DataTableAlign::Start,
                ColumnAlign::End => DataTableAlign::End,
            };
            column
        })
        .collect();
    let suggested: f32 = PHYSICAL_ASSET_COLUMNS.iter().map(|def| def.width).sum();
    let extra = (width - TABLE_WIDTH_PAD - suggested).max(0.0);
    if extra > 0.0
        && let Some(name) = columns.iter_mut().find(|col| col.key == "name")
        && let Some(w) = name.width
    {
        name.width = Some(Points(w.0 + extra));
    }
    columns
}

/// 表格外围定宽扣减（列宽计算输入）：内容列 p-4（两侧 16pt）+ 清单卡 p-3
///（两侧 12pt）+ 清单卡描边两侧 1pt——保单 / 储蓄目标视图同形。
const TABLE_WIDTH_PAD: f32 = 16.0 * 2.0 + 12.0 * 2.0 + 2.0;
