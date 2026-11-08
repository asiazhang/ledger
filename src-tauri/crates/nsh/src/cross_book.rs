//! 跨账本投资汇总（NSH 迁移㉔ #2055，旧壳 `CrossBookSummaryView.vue` 的重生；
//! ADR-0114）：多账本投资口径（持仓市值 / 持仓收益 / 累计收益 / 可投资资产）
//! 的只读合计视图——折算、逐本状态与口径标注全部在本模块壳层编排产出，视图
//! 只做装配渲染，无任何编辑入口（ADR-0114 决策 6）。
//!
//! 编排是旧壳 `src-tauri/src/cross_book_summary.rs`（根包壳层编排，ADR-0114
//! 决策权威）的**重生**：跨本聚合不属投资域（域是单本语义，ADR-0112），编排
//! 住壳层；本壳与旧壳各自壳内承接同一段编排，聚合表达式的口径义务（消费域
//! 单点读函数、折不另立）由消费面满足——`read_book_investment` 只调
//! `ledger-investment` 域读函数，折算只调核心交易域 `convert_to_native_current`
//! 单点。逐本状态闭集、部分合计警示与折算口径标注逐条随 ADR-0114 决策 3/5。
//!
//! 只读承诺：本模块不产生任何写入；活动本复用进程内只读连接，逐本连接取数
//! 后即弃（作用域 drop）。
//!
//! **读快照一致性**（#1699 同款纪律）：活动本读数与汇率折算同读事务
//! （`ensure_transaction` 收口）；逐本读各收本内快照，跨库天然逐本时点。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ledger_infra::db::book_registry::{Book, RegistryRead, read_registry};
use ledger_infra::db::data_location::DB_FILE_NAME;
use ledger_infra::db::encryption::DbFileKind;
use ledger_infra::db::encryption::probe_file_kind;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{DbState, open_connection_readonly_in, schema_version};
use ledger_infra::error::{AppError, Result};
use ledger_transaction::amount;
use rusqlite::Connection;

use nsh_core::prelude::*;

use crate::app::db_slot;
use crate::popover::popover_tip;
use crate::{assembly, boot, nav};

/// 注册表不可用的稳定错误码与文案（旧壳 cross_book_summary 命令同码同文
/// 案——注册表是排除盘期锚，排除盘期唯一写入面是旧壳生产入口）。
const CODE_REGISTRY_UNAVAILABLE: &str = "book.registry-unavailable";
const MSG_REGISTRY_UNAVAILABLE: &str = "账本注册表尚未就绪，请重启应用后再试";

/// 旧壳产品标识（注册表数据根的拼接面，#2055）：账本注册表（引导层）由旧
/// 壳（唯一发布产品）掌管，本壳消费同一根——本壳 AppId 不同，数据根拼接式
/// 与旧壳 tauri `app_data_dir()` 同式（XDG_DATA_HOME 或 `~/.local/share` 拼
/// identifier，Linux 平台面）。
pub const LEDGER_APP_IDENTIFIER: &str = "com.zhangheng.ledger";

/// 数据根解析（Linux 平台语义）：生产 bin 启动时调用一次并落槽位（env 不
/// 进读线程的时间窗口内现读）；None = 数据根不可解析（注册表随之不可用，
/// 码化拒绝同码）。
pub fn default_data_dir_from_env() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_DATA_HOME") {
        Some(v) if !v.is_empty() && Path::new(&v).is_absolute() => PathBuf::from(v),
        _ => {
            let home = std::env::var_os("HOME")?;
            let mut dir = PathBuf::from(home);
            dir.push(".local");
            dir.push("share");
            dir
        }
    };
    Some(base.join(LEDGER_APP_IDENTIFIER))
}

/// 数据根槽位（bin 启动注入；headless 测试按场景注入——app.rs 复位点同批）。
static DATA_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

fn data_dir_slot() -> std::sync::MutexGuard<'static, Option<PathBuf>> {
    DATA_DIR.lock().unwrap_or_else(|p| p.into_inner())
}

/// 数据根槽位读取（#2054）：恢复安全备份（RestoreSafetyBackup）等「应用
/// 数据根」面的消费点——与旧壳 `app_data_dir()` 同一语义的单点（汇总读取
/// 同根）。未注入回 `None`（调用方码化拒绝，不静默落错位）。
pub fn default_data_dir() -> Option<PathBuf> {
    data_dir_slot().clone()
}

/// 注入注册表数据根（幂等：先注入者优先，与账本目录接缝同形）。生产 bin
/// 启动时调用一次；未注入时汇总读取码化拒绝（注册表不可用同码）。
pub fn set_default_data_dir(dir: PathBuf) {
    let mut slot = data_dir_slot();
    if slot.is_none() {
        *slot = Some(dir);
    }
}

/// 复位数据根槽位（headless 测试接缝，app.rs 复位点同批）。
#[doc(hidden)]
pub fn reset_default_data_dir() {
    *data_dir_slot() = None;
}

/// 汇总读入口（视图 blocking 闭包消费面，旧壳命令编排的第①–④步同序）：
/// 数据根 → 注册表读（校验通过；损坏/缺失/数据根缺失同码化拒绝）→ 活动本
/// schema 版本（活动本建连必经迁移，其版本即当前应用 schema 版本）→ 非活
/// 动本逐本探测与只读取数 → 活动本读 + 当期汇率折算合并。全程只读。
pub(crate) fn summary(db: &DbState) -> Result<CrossBookInvestmentSummary> {
    let Some(default_dir) = data_dir_slot().clone() else {
        return Err(AppError::coded(
            CODE_REGISTRY_UNAVAILABLE,
            MSG_REGISTRY_UNAVAILABLE,
        ));
    };
    // ① 注册表（引导配置，打开任何库之前必须可读）：损坏/未配置 → 码化拒绝。
    let registry = match read_registry(&default_dir) {
        RegistryRead::Resolved(registry) => registry,
        _ => {
            tracing::warn!(default_dir = %default_dir.display(), "注册表不可用，跨账本汇总拒绝出数");
            return Err(AppError::coded(
                CODE_REGISTRY_UNAVAILABLE,
                MSG_REGISTRY_UNAVAILABLE,
            ));
        }
    };
    let books = registry.books;
    let active_id = registry.active_id;

    // ② 活动本 schema 版本（活动本建连必经迁移）。
    let active_schema_version = {
        let conn = db
            .read_conn
            .lock()
            .map_err(|e| AppError::Db(e.to_string()))?;
        schema_version(&conn)?
    };

    // ③ 非活动本逐本探测与只读取数（失败逐本降级为不可读态，不阻塞其他本）。
    let (rows, readings) = collect_other_books(&books, &active_id, active_schema_version);

    // ④ 活动本读数 + 当期汇率折算合并（同读事务收口在合并器内）。
    let conn = db
        .read_conn
        .lock()
        .map_err(|e| AppError::Db(e.to_string()))?;
    read_active_book_and_merge(readings, rows, &conn)
}

/// 纯函数合并器（旧壳 merge_readings 重生）：币种＝目标币种直接相加，否则折
/// 算并置 converted；任一计入本的本位币≠目标币同样置 converted（位币不一致
/// 是口径事实）；空值组跳过、不以零计入。
pub(crate) fn merge_readings(
    readings: &[BookInvestmentReading],
    target_currency: &str,
    convert: &mut dyn FnMut(i64, &str) -> ledger_infra::error::Result<i64>,
) -> ledger_infra::error::Result<MergedTotals> {
    let mut totals = MergedTotals {
        market_value_cents: 0,
        unrealized_pnl_cents: 0,
        cumulative_pnl_cents: 0,
        investable_assets_cents: 0,
        converted: readings
            .iter()
            .any(|r| r.native_currency != target_currency),
    };
    let mut add = |sum: &mut i64, cents: i64, currency: &str| -> ledger_infra::error::Result<()> {
        if currency == target_currency {
            *sum += cents;
        } else {
            totals.converted = true;
            *sum += convert(cents, currency)?;
        }
        Ok(())
    };
    for reading in readings {
        for group in &reading.holding_totals {
            if let Some(cents) = group.market_value_cents {
                add(&mut totals.market_value_cents, cents, &group.currency_code)?;
            }
            if let Some(cents) = group.unrealized_pnl_cents {
                add(
                    &mut totals.unrealized_pnl_cents,
                    cents,
                    &group.currency_code,
                )?;
            }
        }
        for group in &reading.cumulative_pnl {
            add(
                &mut totals.cumulative_pnl_cents,
                group.cumulative_pnl_cents,
                &group.currency_code,
            )?;
        }
        if let Some((currency, cents)) = &reading.investable_assets {
            add(&mut totals.investable_assets_cents, *cents, currency)?;
        }
    }
    Ok(totals)
}

/// 合并结果（折算后，目标币种口径）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MergedTotals {
    pub market_value_cents: i64,
    pub unrealized_pnl_cents: i64,
    pub cumulative_pnl_cents: i64,
    pub investable_assets_cents: i64,
    pub converted: bool,
}

/// 单本投资口径原始读数（金额带币种，折算前）。
#[derive(Debug, Clone, Default)]
pub(crate) struct BookInvestmentReading {
    pub native_currency: String,
    pub holding_totals: Vec<ledger_investment::CurrencyHoldingTotals>,
    pub cumulative_pnl: Vec<ledger_investment::CurrencyCumulativePnl>,
    pub investable_assets: Option<(String, i64)>,
}

/// 汇总载荷（折算全部在编排侧完成，视图零算术——仪表盘同款纪律）。
#[derive(Debug, Clone)]
pub(crate) struct CrossBookInvestmentSummary {
    /// 折算目标＝活动账本（主账本）本位币。
    pub target_currency: String,
    /// 是否发生过当期汇率折算（界面据此标注口径）。
    pub converted: bool,
    pub market_value_cents: i64,
    pub unrealized_pnl_cents: i64,
    pub cumulative_pnl_cents: i64,
    pub investable_assets_cents: i64,
    /// 逐本状态行（注册表序，含活动本。
    pub books: Vec<CrossBookBookRow>,
}

/// 逐本状态闭集（ADR-0114 决策 5，2026-09-14 修订：四态排除 + 活动本计入，
/// 逐本可见、不静默少算）。zh 词表归装配层 `cross_book_status_label`
///（旧壳 crossBook.status.* 同值表），本枚举只承载判别面。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CrossBookBookStatus {
    /// 计入。
    Included,
    /// 加密库且进程内未解锁（非活动密文本恒此态）。
    Locked,
    /// 只登记未建库（库文件缺失或空文件）。
    NotInitialized,
    /// 明文库 schema 版本与活动本不一致（旧版待升级或库来自更新版本）。
    SchemaMismatch,
    /// 只读打开或读取失败（文件损坏等）。
    Unreadable,
}

/// 逐本行：注册表序呈现，含活动本；`is_active` 是「当前」标签的判定面
///（旧壳载荷无此字段、非活动计入本被误标「当前」，本壳重生时正位——
/// 旧壳缺陷另立 issue，见交付报告）。
#[derive(Debug, Clone)]
pub(crate) struct CrossBookBookRow {
    pub id: String,
    pub name: String,
    pub status: CrossBookBookStatus,
    pub is_active: bool,
}

/// 单本读数（投资域 `&Connection` 纯读组合）：持仓合计 + 累计收益 + 可投资
/// 资产。任一读失败原样上抛（逐本分派降级为不可读态）。
///
/// **读快照一致性（#1699 纪律重生）**：四段读数整体收进同一读事务（嵌套
/// 感知）——写提交落在语句之间会让同屏合计与可投资资产不同时点。覆盖
/// 活动本与非活动本两条调用路径；同接线的探针级证据住根包编排单测
///（`cross_book_summary` 测试权威，本重生同调用序、不再重镏）。
pub(crate) fn read_book_investment(conn: &Connection) -> Result<BookInvestmentReading> {
    ensure_transaction(conn, || {
        let holding_totals = ledger_investment::query_holdings_summary_by_currency(conn)?;
        let cumulative_pnl = ledger_investment::query_cumulative_pnl_summary(conn)?;
        let investable_assets_cents = ledger_investment::query_investable_assets_cents(conn)?;
        let native_currency = amount::default_currency_code(conn)?;
        Ok(BookInvestmentReading {
            native_currency: native_currency.clone(),
            holding_totals,
            cumulative_pnl,
            investable_assets: Some((native_currency, investable_assets_cents)),
        })
    })
}

/// 活动本读数与合并的单点（ADR-0114 决策 3 第④步）：活动本读数、目标本位
/// 币、当期汇率折算与逐本合并一次给出。
///
/// **读快照一致性（#1699）**：整段收进同一读事务（嵌套感知）——本读闭包、
/// 目标本位币与 merge 的逐笔当期汇率取数同快照（同上：探针级证据住根包
/// 编排单测，本重生同调用序）。非活动本读数各自已收本内快照；跨库合天然
/// 逐本时点，不假装跨本一致。
pub(crate) fn read_active_book_and_merge(
    mut readings: Vec<BookInvestmentReading>,
    rows: Vec<CrossBookBookRow>,
    conn: &Connection,
) -> Result<CrossBookInvestmentSummary> {
    ensure_transaction(conn, || {
        readings.push(read_book_investment(conn)?);
        let target_currency = amount::default_currency_code(conn)?;
        let totals = merge_readings(&readings, &target_currency, &mut |cents, currency| {
            amount::convert_to_native_current(conn, cents, currency)
        })?;
        Ok(CrossBookInvestmentSummary {
            target_currency,
            converted: totals.converted,
            market_value_cents: totals.market_value_cents,
            unrealized_pnl_cents: totals.unrealized_pnl_cents,
            cumulative_pnl_cents: totals.cumulative_pnl_cents,
            investable_assets_cents: totals.investable_assets_cents,
            books: rows,
        })
    })
}

/// 逐本探测与读取（非活动本）：返回注册表序的全量状态行与非活动本的可计
/// 入读数。分派与降级逐条随 ADR-0114 决策 5（见 [`read_other_book`]）。
pub(crate) fn collect_other_books(
    books: &[Book],
    active_id: &str,
    active_schema_version: i64,
) -> (Vec<CrossBookBookRow>, Vec<BookInvestmentReading>) {
    let mut rows = Vec::with_capacity(books.len());
    let mut readings = Vec::new();
    for book in books {
        if book.id == active_id {
            rows.push(CrossBookBookRow {
                id: book.id.clone(),
                name: book.name.clone(),
                status: CrossBookBookStatus::Included,
                is_active: true,
            });
            continue;
        }
        let (status, reading) = read_other_book(book, active_schema_version);
        rows.push(CrossBookBookRow {
            id: book.id.clone(),
            name: book.name.clone(),
            status,
            is_active: false,
        });
        if let Some(reading) = reading {
            readings.push(reading);
        }
    }
    (rows, readings)
}

/// 单本（非活动）分派：探测文件形态 → 空库＝未初始化、密文＝未解锁、明文
/// ＝比版本后读取；打开/读取失败逐本降级为 Unreadable，不上抛、不阻塞其
/// 他本（ADR-0114 决策 5 修订版四态闭集）。
fn read_other_book(
    book: &Book,
    active_schema_version: i64,
) -> (CrossBookBookStatus, Option<BookInvestmentReading>) {
    let db_path = book.dir.join(DB_FILE_NAME);
    let kind = match probe_file_kind(&db_path) {
        Ok(kind) => kind,
        Err(_) => return (CrossBookBookStatus::Unreadable, None),
    };
    match kind {
        DbFileKind::Empty => (CrossBookBookStatus::NotInitialized, None),
        // 进程内没有逐本解锁路径（ADR-0114 决策 5 / ADR-0117 决策 2）。
        DbFileKind::Encrypted => (CrossBookBookStatus::Locked, None),
        DbFileKind::Plaintext => {
            // 只读打开、不迁移（迁移是写；旧版本等先切换进入再升级）。
            let conn = match open_connection_readonly_in(&book.dir) {
                Ok(conn) => conn,
                Err(e) => {
                    tracing::warn!(error = %e, "非活动本只读打开失败，逐本降级为不可读态");
                    return (CrossBookBookStatus::Unreadable, None);
                }
            };
            let version = match schema_version(&conn) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "非活动本 schema 版本读取失败，逐本降级为不可读态");
                    return (CrossBookBookStatus::Unreadable, None);
                }
            };
            if version != active_schema_version {
                return (CrossBookBookStatus::SchemaMismatch, None);
            }
            match read_book_investment(&conn) {
                Ok(reading) => (CrossBookBookStatus::Included, Some(reading)),
                Err(e) => {
                    tracing::warn!(error = %e, "非活动明文本读取失败，逐本降级为不可读态");
                    (CrossBookBookStatus::Unreadable, None)
                }
            }
        }
    }
}

/// 汇总视图卡面快照（读路径一次装配的全部屏内文本，渲染期零格式化）：四
/// 口径卡格 + 口径标注 + 逐本行。信号容器要求的全量派生（PartialEq 供响应
/// 等值、Default 供缺席快照兜底）。
#[derive(Debug, Clone, PartialEq, Default)]
struct CrossBookViewCells {
    /// 口径标注（partialNote 或 currencyNote，随未计入态切换）。
    note: String,
    /// 逐本计入卡片尾部的币种标注小字（旧壳 header-extra 同位置）。
    footer_note: String,
    /// 存在未计入本（合计警示的呈现面）。
    has_excluded: bool,
    cards: Vec<assembly::CrossBookTotalCells>,
    books: Vec<CrossBookBookCell>,
}

/// 逐本行格：名称 + 状态词 + 「当前」标签与计入色面。
#[derive(Debug, Clone, PartialEq)]
struct CrossBookBookCell {
    id: String,
    name: String,
    status: &'static str,
    included: bool,
    is_active: bool,
}

/// 跨账本投资汇总视图（导航接缝可达，#2055）：只读合计——无编辑入口
///（ADR-0114 决策 6），入口外的全部动作只有读失败的重试。守卫随投资开
/// 关（[nav_model::guard_feature] 映射），视图本体不做开关判定。视图本地
/// 快照、离开再回 = 全新装载（旧壳 onMounted 同口径）；无定位形态（落位
/// 意图读一次即弃，汇总路由不读 focus 同）。
#[component]
pub(crate) fn CrossBookSummaryView() -> impl IntoElement {
    let view_owner = Owner::current();

    // ============================ 快照态 ============================
    let cells = signal(None::<CrossBookViewCells>);
    let error = signal(None::<String>);
    let loading = signal(true);
    let load_seq = signal(0u64);

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
                let outcome = blocking(move || {
                    crate::cross_book::summary(&db).map(|loaded| {
                        let fmt_table = boot::currency_rows(&db)
                            .map(|rows| assembly::CurrencyTable::from_currencies(&rows))
                            .unwrap_or_default();
                        assemble_view_cells(&loaded, &fmt_table)
                    })
                })
                .await;
                if load_seq.get() != seq {
                    return;
                }
                match outcome {
                    Ok(Ok(transformed)) => cells.set(Some(transformed)),
                    Ok(Err(e)) => error.set(Some(e.to_string())),
                    Err(_) => error.set(Some("内部错误：读任务异常退出".to_owned())),
                }
                loading.set(false);
            });
        });
    };

    // 落位：首次装载 + 意图消费（读一次封闸：汇总路由无定位形态）。
    effect(move || {
        let _landed = nav::store().current.get();
        let _ = nav::take_intent();
        reload();
    });

    // 主题令牌（双态自适应；令牌缺失回退亮灰/中灰可读兜底，先例同形）。
    let tokens = nsh_core::element::tokens();
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    let muted = tokens
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into());
    let danger_fg = tokens
        .color("destructive")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    // 警告色（未计入本状态标签；危险确认 warning 级同色值）。
    let warn_fg = AdaptiveColor::new(rgb(0xf0a020), rgb(0xf2c97d));
    // 计入色（账本语义绿，与 #2033 起的正常态语义色同源）。
    let ok_fg = AdaptiveColor::new(rgb(0x18a058), rgb(0x63e2b7));

    // 错误态（码化错误 zh 原样）：汇总读取失败 + 重试（同条读路径重发，
    // 零写入）。
    let error_body = when(
        move || error.get().is_some(),
        move || {
            view()
                .flex_col()
                .gap_2()
                .app_class("rounded-md border border-border bg-background p-3")
                .child(
                    view()
                        .flex_row()
                        .gap_2()
                        .items_center()
                        .child(
                            text("汇总读取失败")
                                .text_size(px(13.0))
                                .text_color(danger_fg)
                                .test_id("cross-book-error"),
                        )
                        .child(
                            view().flex_row().items_center().gap_2().child(
                                text(move || error.get().unwrap_or_default())
                                    .text_size(px(12.0))
                                    .text_color(muted),
                            ),
                        ),
                )
                .child(
                    view()
                        .on_press(move |_: &()| reload())
                        .rounded_md()
                        .px_2()
                        .py_1()
                        .app_class("bg-primary hover:opacity-90")
                        .child(
                            text("重试")
                                .text_size(px(13.0))
                                .text_color(rgb(0xffffff))
                                .test_id("cross-book-retry"),
                        ),
                )
        },
    )
    .otherwise(move || {
        when(
            move || loading.get(),
            move || {
                text("加载中…")
                    .text_size(px(13.0))
                    .text_color(muted)
                    .test_id("cross-book-loading")
            },
        )
        .otherwise(move || {
            // 口径标注（警示优先：部分合计警示替换币种句；全计入时直接相加句）。
            view()
                .flex_col()
                .gap_3()
                .child(
                    text(move || {
                        let snapshot = cells.get();
                        if snapshot.as_ref().is_some_and(|c| c.has_excluded) {
                            "部分账本未计入，以下合计为部分合计".to_owned()
                        } else {
                            snapshot.map(|c| c.note).unwrap_or_default()
                        }
                    })
                    .text_size(px(13.0))
                    .text_color(move || {
                        if cells.get().as_ref().is_some_and(|c| c.has_excluded) {
                            warn_fg
                        } else {
                            muted
                        }
                    })
                    .test_id("cross-book-note"),
                )
                .child(
                    // 四口径合计卡（同一行栅格；说明气泡随卡，presto headless 锚点。
                    view().flex_row().gap_4().child(each(
                        move || cells.get().map(|c| c.cards).unwrap_or_default(),
                        |card| card.key,
                        move |card, _index| {
                            let key = card.get_untracked().key;
                            let test_id = format!("summary-{key}");
                            let info_id = format!("cross-book-{key}-info");
                            view()
                                .flex_col()
                                .gap_1()
                                .flex_1()
                                .app_class("rounded-lg border border-border bg-background p-4")
                                .child(
                                    view()
                                        .flex_row()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            text(move || card.get().label)
                                                .text_size(px(12.0))
                                                .text_color(muted),
                                        )
                                        .child(popover_tip(
                                            info_id,
                                            vec![card.get_untracked().tip],
                                            text("ⓘ").text_size(px(12.0)).text_color(muted),
                                        )),
                                )
                                .child(
                                    text(move || card.get().amount)
                                        .text_size(px(22.0))
                                        .text_color(fg)
                                        .test_id(test_id.as_str()),
                                )
                        },
                    )),
                )
                .child(
                    // 账本计入状态卡（名称 + 「当前」标签 + 状态标签）。
                    view()
                        .flex_col()
                        .gap_2()
                        .app_class("rounded-lg border border-border bg-background p-4")
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(text("账本计入状态").text_size(px(13.0)).text_color(fg))
                                .child(
                                    text(move || {
                                        cells.get().map(|c| c.footer_note).unwrap_or_default()
                                    })
                                    .text_size(px(12.0))
                                    .text_color(muted),
                                ),
                        )
                        .child(each(
                            move || cells.get().map(|c| c.books).unwrap_or_default(),
                            |book| book.id.clone(),
                            move |book, _index| {
                                let id = book.get_untracked().id;
                                let row_test_id = format!("summary-book-{id}");
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .test_id(row_test_id.as_str())
                                    .child(
                                        text(move || book.get().name.clone())
                                            .text_size(px(13.0))
                                            .text_color(fg)
                                            .test_id(format!("summary-book-name-{id}").as_str()),
                                    )
                                    .child(
                                        // 「当前」标签只挂活动本行。
                                        when(
                                            move || book.get().is_active,
                                            move || {
                                                text("当前")
                                                    .text_size(px(11.0))
                                                    .text_color(fg)
                                                    .test_id(
                                                        format!("summary-book-active-{id}")
                                                            .as_str(),
                                                    )
                                            },
                                        )
                                        .otherwise(view),
                                    )
                                    .child(
                                        text(move || book.get().status)
                                            .text_size(px(11.0))
                                            .text_color(move || {
                                                if book.get().included { ok_fg } else { warn_fg }
                                            }),
                                    )
                            },
                        )),
                )
        })
    });

    view()
        .flex_1()
        .flex_col()
        .gap_3()
        .p_4()
        .app_class("bg-background overflow-y-auto")
        .test_id("cross-book-summary")
        .child(error_body)
}

/// 域编排出数 → 卡面快照（渲染期零格式化的装配单点，数据全在本次调用内）。
fn assemble_view_cells(
    summary: &crate::cross_book::CrossBookInvestmentSummary,
    currencies: &assembly::CurrencyTable,
) -> CrossBookViewCells {
    CrossBookViewCells {
        footer_note: assembly::cross_book_currency_note(
            summary.converted,
            &summary.target_currency,
        ),
        note: assembly::cross_book_currency_note(summary.converted, &summary.target_currency),
        has_excluded: assembly::cross_book_has_excluded(summary),
        cards: assembly::assemble_cross_book_totals(summary, currencies),
        books: summary
            .books
            .iter()
            .map(|book| CrossBookBookCell {
                id: book.id.clone(),
                name: book.name.clone(),
                status: assembly::cross_book_status_label(book.status.clone()),
                included: book.status == crate::cross_book::CrossBookBookStatus::Included,
                is_active: book.is_active,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ledger_infra::error::AppError;
    use ledger_investment::CurrencyCumulativePnl;
    use ledger_investment::CurrencyHoldingTotals;

    fn totals(
        currency: &str,
        market: Option<i64>,
        unrealized: Option<i64>,
    ) -> CurrencyHoldingTotals {
        CurrencyHoldingTotals {
            currency_code: currency.into(),
            market_value_cents: market,
            unrealized_pnl_cents: unrealized,
        }
    }

    fn cumulative(currency: &str, cents: i64) -> CurrencyCumulativePnl {
        CurrencyCumulativePnl {
            currency_code: currency.into(),
            cumulative_pnl_cents: cents,
        }
    }

    /// 同币种直加：目标币种金额直接求和，`converted` 恒 false（旧壳合并器
    /// 同名测试重生）。
    #[test]
    fn same_currency_adds_directly_without_conversion() {
        let readings = vec![
            BookInvestmentReading {
                native_currency: "CNY".into(),
                holding_totals: vec![totals("CNY", Some(120_000), Some(20_000))],
                cumulative_pnl: vec![cumulative("CNY", 45_000)],
                investable_assets: Some(("CNY".into(), 170_000)),
            },
            BookInvestmentReading {
                native_currency: "CNY".into(),
                holding_totals: vec![totals("CNY", Some(1_000), Some(-500))],
                cumulative_pnl: vec![cumulative("CNY", 500)],
                investable_assets: Some(("CNY".into(), 1_500)),
            },
        ];
        let merged = merge_readings(&readings, "CNY", &mut |_cents, cur| {
            panic!("同币种不应触发折算，实际折算 {cur}")
        })
        .unwrap();
        assert_eq!(
            merged,
            MergedTotals {
                market_value_cents: 121_000,
                unrealized_pnl_cents: 19_500,
                cumulative_pnl_cents: 45_500,
                investable_assets_cents: 171_500,
                converted: false,
            }
        );
    }

    /// 异币种折算：经注入闭包折算到目标币种并置 `converted`；空值组跳过。
    #[test]
    fn mixed_currencies_convert_and_flag() {
        let readings = vec![
            BookInvestmentReading {
                native_currency: "CNY".into(),
                holding_totals: vec![
                    totals("CNY", Some(100_000), Some(10_000)),
                    totals("USD", None, Some(2_000)),
                ],
                cumulative_pnl: vec![cumulative("USD", 3_000)],
                investable_assets: Some(("CNY".into(), 110_000)),
            },
            BookInvestmentReading {
                native_currency: "USD".into(),
                holding_totals: vec![totals("USD", Some(12_000), Some(2_000))],
                cumulative_pnl: vec![cumulative("HKD", 700)],
                investable_assets: Some(("USD".into(), 14_000)),
            },
        ];
        let merged = merge_readings(&readings, "CNY", &mut |cents, _cur| Ok(cents * 7)).unwrap();
        assert!(merged.converted);
        assert_eq!(merged.market_value_cents, 100_000 + 12_000 * 7);
        assert_eq!(merged.unrealized_pnl_cents, 10_000 + (2_000 + 2_000) * 7);
        assert_eq!(merged.cumulative_pnl_cents, (3_000 + 700) * 7);
        assert_eq!(merged.investable_assets_cents, 110_000 + 14_000 * 7);
    }

    /// 空读数（全部排除的现场）：全零合计、未折算，不报错。
    #[test]
    fn empty_readings_yield_zero_totals() {
        let merged = merge_readings(&[], "CNY", &mut |cents, _cur| Ok(cents * 7)).unwrap();
        assert_eq!(
            merged,
            MergedTotals {
                market_value_cents: 0,
                unrealized_pnl_cents: 0,
                cumulative_pnl_cents: 0,
                investable_assets_cents: 0,
                converted: false,
            }
        );
    }

    /// 折算闭包的错误原样上抛（缺汇率现场由当期入口 `convert_to_native_current`
    /// 命中 `fx.rate-missing`），合并不吞错。
    #[test]
    fn conversion_error_propagates() {
        let readings = vec![BookInvestmentReading {
            native_currency: "USD".into(),
            holding_totals: vec![totals("USD", Some(12_000), None)],
            cumulative_pnl: vec![],
            investable_assets: None,
        }];
        let err = merge_readings(&readings, "CNY", &mut |_cents, _cur| {
            Err(AppError::Invalid("boom".into()))
        })
        .unwrap_err();
        assert!(matches!(err, AppError::Invalid(_)));
    }

    /// 位币不一致本身就是折算口径事实：计入本的本位币≠目标币时，即使本次金
    /// 额全部同币种、零折算发生，也置 `converted`（ADR-0114 决策 3 标注义务）。
    #[test]
    fn base_currency_divergence_flags_converted_even_without_amount_conversion() {
        let readings = vec![BookInvestmentReading {
            native_currency: "USD".into(),
            holding_totals: vec![totals("CNY", Some(100_000), Some(10_000))],
            cumulative_pnl: vec![],
            investable_assets: Some(("CNY".into(), 110_000)),
        }];
        let merged = merge_readings(&readings, "CNY", &mut |_cents, _cur| {
            panic!("金额全部同币种，不应触发折算闭包")
        })
        .unwrap();
        assert!(merged.converted, "位币不一致应置折算口径标注");
        assert_eq!(merged.investable_assets_cents, 110_000);
    }

    // -------------------------------------------------------------------
    // 单本读数与逐本分派（文件现场测试；旧壳 probes/dispatch 集成场景的重生
    // 子集——四态分派逐本可见、不阻塞其他本出数，造数经产品入口/域公开写入。
    // -------------------------------------------------------------------

    use ledger_accounts::AccountType;

    /// 唯一暂存库目录（同 tests.rs 纪律：身份工厂发 UUID，进程退出由 OS 回收）。
    fn scratch_book_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nsh-xbook-{tag}-{}", ledger_infra::ids::new_uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 建一个库作为可读明文本（0 字节文件 + 产品开库入口；返回连接）。
    fn open_preminted_book(tag: &str) -> (std::path::PathBuf, ledger_infra::db::DbState) {
        let dir = scratch_book_dir(tag);
        std::fs::write(dir.join(ledger_infra::db::data_location::DB_FILE_NAME), b"").unwrap();
        let db = crate::boot::boot_ledger(&dir).unwrap();
        (dir, db)
    }

    /// 逐本分派（旧壳集成场景的重生子集）：明文本计入并出数、空库/目录登记
    /// 未建库 = 尚未初始化、密文本 = 未解锁、schema 版本不一致 = 版本不一致
    /// ——四态逐本可见、不阻塞其他本。
    #[test]
    fn collect_other_books_dispatches_four_visible_states() {
        // 活动本（无需造数；其 schema 版本即参照）。
        let (_active_dir, active_db) = open_preminted_book("active");

        // 明文本：投资账户出数（现金腿 = 可投资资产读数）。
        let (readable_dir, readable_db) = open_preminted_book("readable");
        {
            let conn = readable_db.conn.lock().unwrap();
            ledger_accounts::create_account(
                &conn,
                ledger_accounts::AccountInput {
                    name: "投入".into(),
                    kind: AccountType::Investment,
                    currency_code: "CNY".into(),
                    initial_balance_cents: Some(150_000),
                    credit_limit_cents: None,
                    statement_day: None,
                    due_day: None,
                },
            )
            .unwrap();
        }
        drop(readable_db);

        // 空库本（0 字节 → 尚未初始化）。
        let empty_dir = scratch_book_dir("empty");
        std::fs::write(
            empty_dir.join(ledger_infra::db::data_location::DB_FILE_NAME),
            b"",
        )
        .unwrap();

        // 密文本（先建明文库再转加密 → 未解锁、未计入；ADR-0117 决策 2）。
        let (locked_dir, _locked_db) = open_preminted_book("locked");
        ledger_infra::db::encryption::enable_encryption_for_file(
            &locked_dir.join(ledger_infra::db::data_location::DB_FILE_NAME),
            "测试口令",
        )
        .unwrap();
        drop(_locked_db);

        // 版本不一致本：明文迁移完成后降写 user_version（仓库外版本位移的
        // 现场模拟）。
        let (mismatch_dir, mismatch_db) = open_preminted_book("mismatch");
        mismatch_db
            .conn
            .lock()
            .unwrap()
            .execute_batch("PRAGMA user_version = 1")
            .unwrap();

        // 未建库本：只登记目录、库里无文件（→ 尚未初始化）。
        let missing_dir = scratch_book_dir("missing");

        let active_id = "active-id".to_owned();
        let book = |id: &str, name: &str, dir: std::path::PathBuf| Book {
            id: id.to_owned(),
            name: name.to_owned(),
            dir,
        };
        let books = vec![
            book("active-id", "活动本", _active_dir),
            book("b-readable", "明文本", readable_dir),
            book("b-empty", "空库本", empty_dir),
            book("b-locked", "密文本", locked_dir),
            book("b-mismatch", "旧版本本", mismatch_dir),
            book("b-missing", "未建库本", missing_dir),
        ];
        let schema =
            ledger_infra::db::schema_version(&active_db.read_conn.lock().unwrap()).unwrap();
        let (rows, readings) = collect_other_books(&books, &active_id, schema);

        let status_of = |id: &str| {
            rows.iter()
                .find(|row| row.id == id)
                .map(|row| row.status.clone())
                .unwrap()
        };
        assert_eq!(status_of("active-id"), CrossBookBookStatus::Included); // 活动本恒计入
        assert_eq!(status_of("b-readable"), CrossBookBookStatus::Included); // 明文本计入
        assert_eq!(status_of("b-locked"), CrossBookBookStatus::Locked);
        assert_eq!(status_of("b-mismatch"), CrossBookBookStatus::SchemaMismatch);
        assert_eq!(status_of("b-empty"), CrossBookBookStatus::NotInitialized);
        assert_eq!(status_of("b-missing"), CrossBookBookStatus::NotInitialized);
        // 「当前」标签只挂活动本（非活动计入本不误标，旧壳载荷缺陷的本壳正位）。
        let active_of = |id: &str| {
            rows.iter()
                .find(|row| row.id == id)
                .map(|row| row.is_active)
                .unwrap()
        };
        assert!(active_of("active-id"));
        assert!(!active_of("b-readable"));
        // 只计入本有读数（明细语境不受分派脚手架牵连）。
        assert_eq!(readings.len(), 1);
        assert_eq!(
            readings[0]
                .investable_assets
                .as_ref()
                .map(|(c, _)| c.as_str()),
            Some("CNY")
        );
    }

    /// 活动本读数与合并（ADR-0114 决策 3 第④步单点）：他本读数 + 活动本
    /// 读数在同一目标币种口径下合并；缺折算汇率的码化错误原样上抶。
    #[test]
    fn active_book_merge_consumes_same_currency_and_missing_fx_raises() {
        let (_dir, db) = open_preminted_book("merge");
        // 活动本：投资账户 CNY 现金 2 000 元。
        {
            let conn = db.conn.lock().unwrap();
            ledger_accounts::create_account(
                &conn,
                ledger_accounts::AccountInput {
                    name: "投入".into(),
                    kind: AccountType::Investment,
                    currency_code: "CNY".into(),
                    initial_balance_cents: Some(200_000),
                    credit_limit_cents: None,
                    statement_day: None,
                    due_day: None,
                },
            )
            .unwrap();
        }
        // 他本读数（参数面）：USD 现金 1 000 元 → 活动本无数价折算 → 缺汇率上扶。
        let other = BookInvestmentReading {
            native_currency: "USD".into(),
            holding_totals: vec![],
            cumulative_pnl: vec![],
            investable_assets: Some(("USD".into(), 100_000)),
        };
        let conn = db.read_conn.lock().unwrap();
        let err = read_active_book_and_merge(vec![other], vec![], &conn).unwrap_err();
        assert_eq!(err.code(), Some("fx.rate-missing"));
    }
}
