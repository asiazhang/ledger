//! 装配层（spec #2015「装配层」/ issue #2018 立桩首版 / issue #2019 格文本格式
//! 化全家桶）：行集进入表格前的纯 Rust view-model 装配——每行每格的最终展示
//! 文本与列定义在数据到达时一次性算就，渲染期零格式化（单元格预计算的 NSH
//! 形态，词汇表见界面域「单元格预计算」）。
//!
//! 纪律：
//! - **纯 Rust、无 UI 依赖**（spec #2015：「装配层是纯 Rust、无 UI 依赖，以便
//!   行/格预计算逻辑像域代码一样可单测」）：本模块只 import 域类型（核心交易域
//!   行模型、币种 / 分类参考数据）与 std，不 import `nsh_core` / `nsh_base`——
//!   评审与单测共同守这条边界，UI 宿主映射住 [`crate::app`]；
//! - **口径单源不漂移**：金额展示锚点（转换行转出金额 / 份额调整无金额 / 其余
//!   行本位币锚点）镜像旧壳交易域展示口径；金额换算以核心交易域为唯一真源
//!   （整数分），本层只消费不另立；格文本与语义色的**文案与色值口径以旧壳为
//!   对拍基准**（`formatAmount` / `categoryPath` / `kindSemanticColor` 及 i18n
//!   zh-CN 词表），新旧壳并排打开同一账本库时同口径交易行格文本一致（#2019 AC）；
//! - **范围（#2019 全家桶 + #2020 派生行形态）**：数字分组、金额语义色、隐私
//!   模式遮蔽（掩码消费设备轻量设置项，同一域来源）、分类路径、备注列、通用
//!   kind 标签；借贷派生 transfer 专属文案与派生行形态（购买项逐项行 / 双账户 /
//!   出资分解 / 来源名与状态 / 商户名）在 #2020 接线；
//! - **界面文案仅 zh-CN**（spec #2015 实施决策：i18n 框架级方案不在立桩票），
//!   数字分组固定 zh-CN 四位一组（分组位数由界面语言隐含——NSH 无界面语言
//!   切换，恒 zh；en-US 三位一组随 i18n 方案票再议）。

use std::collections::{BTreeMap, BTreeSet};

use chrono::Datelike;

use ledger_accounts::AccountType;
use ledger_currencies::Currency;
use ledger_transaction::{
    Transaction, TransactionKind, TransactionSourceKind, TransactionSourceStatus,
};

/// 每页行数（issue #2018）：100 行/页服务端 offset 分页，走域读路径
/// （ADR-0008 分页语义）。旧壳 50 行/页是 #2010 翻页迟滞触发点，立桩翻倍
/// 验证原生余量——立桩验收以「无可感知迟滞」对表（ADR-0145 硬伤清单）。
pub const PAGE_SIZE: usize = 100;

/// 列内对齐（UI 无关闭集：UI 层映射为宿主表格的对齐枚举）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnAlign {
    /// 起始对齐（文本列）。
    Start,
    /// 末端对齐（金额列）。
    End,
}

/// 列定义：表格列的装配产物（键、zh-CN 列名、对齐、建议宽度 pt），UI 层只做
/// 宿主映射、不产列。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColumnDef {
    /// 列键（行格的稳定标识，与列一一对应）。
    pub key: &'static str,
    /// zh-CN 列名。
    pub title: &'static str,
    /// 对齐。
    pub align: ColumnAlign,
    /// 建议宽度（pt）。
    pub width: f32,
}

/// 列定义，列名与旧壳同文案（i18n zh-CN transactions.columns 词表）、列序与
/// 旧壳桌面表格一致（旧壳列全集 = 日期/类型/分类/商户/账户/来源/商品·备注/金额；
/// 操作列随 #2022 行菜单接线）。备注列在购买项展开视图一列两用，列名取旧壳
/// 展开形态词 itemNote「商品 / 备注」（ADR-0138 决策 13，主列表恒展开形态）。
/// 金额列末端对齐（数字惯例）。备注列旧壳不设固定宽（弹性列），此处给建议宽度
/// 供 UI 映射。
pub const COLUMNS: &[ColumnDef] = &[
    ColumnDef {
        key: "date",
        title: "日期",
        align: ColumnAlign::Start,
        width: 110.0,
    },
    ColumnDef {
        key: "kind",
        title: "类型",
        align: ColumnAlign::Start,
        width: 90.0,
    },
    ColumnDef {
        key: "category",
        title: "分类",
        align: ColumnAlign::Start,
        width: 140.0,
    },
    ColumnDef {
        key: "merchant",
        title: "商户",
        align: ColumnAlign::Start,
        width: 120.0,
    },
    ColumnDef {
        key: "account",
        title: "账户",
        align: ColumnAlign::Start,
        width: 180.0,
    },
    ColumnDef {
        key: "source",
        title: "来源",
        align: ColumnAlign::Start,
        width: 140.0,
    },
    ColumnDef {
        key: "note",
        title: "商品 / 备注",
        align: ColumnAlign::Start,
        width: 200.0,
    },
    ColumnDef {
        key: "amount",
        title: "金额",
        align: ColumnAlign::End,
        width: 110.0,
    },
];

/// 一行的装配产物：每列的最终展示文本（字段与 [`COLUMNS`] 逐列对应），`id`
/// 供行级操作定位（行右键删除随 #2022 消费）。购买项订单的一单多行共用同一
/// `id`（派生行的操作目标恒是交易本体）；订单级列（日期/类型/商户/账户/来源/
/// 金额）只在订单块首行带值、续行留空（nsh-base DataTable 无纵向合并能力，
/// 旧壳合并格「内容只出现一次」的语义以首行带值镜像，ADR-0138 决策 13）。
#[derive(Debug, Clone, PartialEq)]
pub struct RowCells {
    /// 交易 id（购买项续行同值——操作目标是交易本体）。
    pub id: String,
    /// 日期列（ISO 原样，与旧壳同口径；订单级）。
    pub date: String,
    /// 类型列（kind 直译标签；transfer 借贷派生专属文案见 [`kind_label`]；订单级）。
    pub kind: String,
    /// 分类列（`父 > 子` 路径；购买项行取该购买项自己的分类，逐行不合并；
    /// 未解析渲染 `-`）。
    pub category: String,
    /// 商户列（商户名，含软删商户历史引用；无/未解析渲染 `-`；订单级）。
    pub merchant: String,
    /// 账户列（单账户 / 双账户「转出 → 转入」/ 出资分解「首条 + 标注」三形态；
    /// 订单级）。
    pub account: String,
    /// 来源列（来源名 + 状态标注；无来源渲染 `-`；订单级）。
    pub source: String,
    /// 备注列（购买项行 = 商品名 + 「共 N 件」，其余行 = 备注原文；无备注渲染
    /// `-`；逐行不合并——一列两用，ADR-0138 决策 13）。
    pub note: String,
    /// 金额列（含隐私掩码与数字分组；无金额行渲染 `-`；订单级——订单实付
    /// 只在块首行出现一次）。
    pub amount: String,
    /// 金额列语义色型（UI 层映射为宿主颜色，色值住 [`crate::app`]）。
    pub amount_semantic: AmountSemantic,
}

/// 金额语义色型（旧壳 `kindSemanticColor` 的输入闭集——九种交易类型各一专属
/// 色，色值公式住旧壳 `packages/theme`，NSH 侧只持色型、不持色值）：每个 kind
/// 恒有语义（表驱动穷尽，无色回退不存在，与旧壳同构）。借贷派生行（#2020）
/// 复用 [`AmountSemantic::Transfer`]——旧壳借贷金额与普通转账同为紫色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmountSemantic {
    /// 支出（红）。
    Expense,
    /// 收入（绿）。
    Income,
    /// 退款（蓝）。
    Refund,
    /// 转账（紫；借贷派生同型）。
    Transfer,
    /// 买入（粉）。
    Buy,
    /// 卖出（青）。
    Sell,
    /// 基金转换（琥珀）。
    Convert,
    /// 份额调整（靛蓝）。
    Split,
    /// 现金分红（青绿）。
    Dividend,
}

/// kind → 金额语义色型（旧壳金额列着色口径：色是 kind 的纯函数，无金额行与
/// 隐私掩码态照常着色——掩码隐藏数字，不隐藏方向，旧壳隐私保留项同口径）。
pub fn amount_semantic(kind: TransactionKind) -> AmountSemantic {
    match kind {
        TransactionKind::Income => AmountSemantic::Income,
        TransactionKind::Expense => AmountSemantic::Expense,
        TransactionKind::Transfer => AmountSemantic::Transfer,
        TransactionKind::Refund => AmountSemantic::Refund,
        TransactionKind::Buy => AmountSemantic::Buy,
        TransactionKind::Sell => AmountSemantic::Sell,
        TransactionKind::Convert => AmountSemantic::Convert,
        TransactionKind::Split => AmountSemantic::Split,
        TransactionKind::Dividend => AmountSemantic::Dividend,
    }
}

/// 类型标签（文案与旧壳 zh-CN 词表一致）：借贷是 transfer 的派生视角——方向由
/// 两端账户类型派生（[`derive_lending_direction`]，旧壳 `kindLabel` 同一判定单
/// 点），借出/收回/借入/还款专属文案与借贷表单方向标签（[`lending_word`]）
/// 同词表单点；普通转账恒「转账」；非 transfer kind 按自身 kind 标签。
pub fn kind_label(kind: TransactionKind, lending: LendingDirection) -> &'static str {
    if kind == TransactionKind::Transfer {
        return match lending {
            LendingDirection::Lend => lending_word(LendingFormDirection::Lend),
            LendingDirection::Collect => lending_word(LendingFormDirection::Collect),
            LendingDirection::Borrow => lending_word(LendingFormDirection::Borrow),
            LendingDirection::Repay => lending_word(LendingFormDirection::Repay),
            LendingDirection::None => "转账",
        };
    }
    match kind {
        TransactionKind::Income => "收入",
        TransactionKind::Expense => "支出",
        TransactionKind::Transfer => "转账",
        TransactionKind::Refund => "退款",
        TransactionKind::Buy => "买入",
        TransactionKind::Sell => "卖出",
        TransactionKind::Convert => "转换",
        TransactionKind::Split => "份额调整",
        TransactionKind::Dividend => "分红",
    }
}

/// 金额展示锚点（旧壳金额列展示口径）：基金转换行展示**转出金额**（确认单权威，
/// ADR-0099——行金额锚点是服务端按 FIFO 消耗算出的结转成本，不是用户看到的
/// 金额）；扩展缺失（旧数据）回退行金额锚点；份额调整行无现金腿、无金额
/// （ADR-0106，`None` 渲染 `-`，不以 0 伪装「已知为零」）；其余行展示行金额
/// 锚点（本位币口径 `amount_native_cents`）。
pub fn amount_display_cents(row: &Transaction) -> Option<i64> {
    if row.kind == TransactionKind::Convert
        && let Some(convert) = row.convert.as_ref()
    {
        return Some(convert.out_amount_cents);
    }
    if row.kind == TransactionKind::Split {
        return None;
    }
    Some(row.amount_native_cents)
}

/// 币种展示参数（金额格式化的消费面：符号 + 小数位）。
#[derive(Debug, Clone, PartialEq)]
pub struct CurrencyFormat {
    /// 币符（紧跟在负号后）。
    pub symbol: String,
    /// 小数位。
    pub decimal_places: u32,
}

impl Default for CurrencyFormat {
    /// 未知币种的缺省（旧壳 `decimal_places ?? 2` / `symbol ?? ""` 同口径）。
    fn default() -> Self {
        Self {
            symbol: String::new(),
            decimal_places: 2,
        }
    }
}

/// 币种展示参数表（账本币种字典的只读投影，code → 符号/小数位）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CurrencyTable {
    pub(crate) by_code: BTreeMap<String, CurrencyFormat>,
    /// 币种字典读序条目（code + name）：表单下拉选项消费面——选项序保持域
    /// 读序（不按 code 重排，旧壳 reference.currencies 序同口径）。
    pub(crate) entries: Vec<(String, String)>,
}

impl CurrencyTable {
    /// 从币种参考数据行集投影。
    pub fn from_currencies<'a>(rows: impl IntoIterator<Item = &'a Currency>) -> Self {
        let rows: Vec<&'a Currency> = rows.into_iter().collect();
        let by_code = rows
            .iter()
            .map(|&c| {
                (
                    c.code.clone(),
                    CurrencyFormat {
                        symbol: c.symbol.clone(),
                        decimal_places: u32::try_from(c.decimal_places.max(0)).unwrap_or(2),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        // 读序条目自同行集投影（BTreeMap 上面的 by_code 已按 code 重排，表单
        // 选项序不能从它拿）；code → 名与表同源。
        let entries = rows
            .iter()
            .map(|c| (c.code.clone(), c.name.clone()))
            .collect();
        Self { by_code, entries }
    }

    /// 币种 `code` 的展示参数；未登记（币种被删等）走缺省，不静默换符号。
    pub fn format_of(&self, code: &str) -> CurrencyFormat {
        self.by_code.get(code).cloned().unwrap_or_default()
    }

    /// 币种选项表（账户表单币种下拉；label 镜像旧壳 `currencyOptions`
    /// 「{name} ({code})」同形，条目序保持域读序、不按 code 重排）。
    pub fn currency_options(&self) -> Vec<CategoryOption> {
        self.entries
            .iter()
            .map(|(code, name)| CategoryOption {
                id: code.clone(),
                label: format!("{} ({})", name, code),
            })
            .collect()
    }
}

/// 分类路径条目（路径解析的消费面：名称 + 父引用）。
#[derive(Debug, Clone, PartialEq)]
struct CategoryEntry {
    name: String,
    parent_id: Option<String>,
}

/// 分类路径表（分类参考数据行集的只读投影，id → 路径文本）：路径口径镜像旧
/// 壳 `categoryPath`（packages/utils）——顶级为自身名，子分类为「父 > 子」路径
/// 名（单跳拼接，更深层截断为两级——旧壳实现即两级，分类实体亦两级为设计
/// 形态）；父引用解析不到（孤儿引用）回退自身名；id 解析不到（软删分类的
/// 历史引用等）返回 `None`，调用方渲染 `-`（旧壳 `categoryPath(id) || "-"`
/// 同口径）。
///
/// 输入行集只收**在用分类**（软删不进路径表）：旧壳路径函数消费在用分类行集
/// （reference store 口径），软删分类的历史交易引用照旧显示 `-`。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CategoryTable {
    by_id: BTreeMap<String, CategoryEntry>,
}

impl CategoryTable {
    /// 从分类参考数据行集投影。
    pub fn from_categories<'a>(
        rows: impl IntoIterator<Item = &'a ledger_categories::Category>,
    ) -> Self {
        Self {
            by_id: rows
                .into_iter()
                .map(|c| {
                    (
                        c.id.clone(),
                        CategoryEntry {
                            name: c.name.clone(),
                            parent_id: c.parent_id.clone(),
                        },
                    )
                })
                .collect(),
        }
    }

    /// 分类 `id` 的路径文本；解析不到（无分类 / 软删分类）返回 `None`。
    pub fn path_of(&self, id: Option<&str>) -> Option<String> {
        let cat = self.by_id.get(id?)?;
        let Some(parent_id) = cat.parent_id.as_deref() else {
            return Some(cat.name.clone());
        };
        match self.by_id.get(parent_id) {
            Some(parent) => Some(format!("{} > {}", parent.name, cat.name)),
            None => Some(cat.name.clone()),
        }
    }
}

/// 金额基础格式化（`formatAmount` zh-CN 口径，issue #2019 全家桶的分组核）：
/// 分 → 元按币种小数位换算、去尾零（98.00→98、98.50→98.5，无损去零不涉舍入）、
/// 负号在最前、币符紧跟其后、整数部分 zh-CN 每四位一组（词汇表「数字分组」，
/// 小数部分不分组）。隐私掩码在装配入口消费 [`RowContext::privacy_enabled`]
/// 后收口（[`AMOUNT_PRIVACY_MASK`]，[`assemble_rows`]）；NSH 无界面语言切换，
/// 分组恒 zh 四位一组（en-US 三位一组随 i18n 方案票再议）。
///
/// 换算口径刻意镜像旧壳 JS（f64 除法 + 定点格式化，已对拍）：整数分在
/// |分| < 2⁵³（约 ¥9×10¹³）内 f64 精确，定点格式化与 `toFixed` 在该量级内
/// 逐位同形（见 tests 基本例）；个人账本量级远在其下，不引入整数串算的第二
/// 套舍入口径（口径单源：旧壳展示行为即对拍基准）。
pub fn format_amount_basic(cents: i64, fmt: &CurrencyFormat) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = (cents as i128).unsigned_abs() as f64;
    let value = abs / 10f64.powi(fmt.decimal_places as i32);
    let fixed = format!("{:.*}", fmt.decimal_places as usize, value);
    // 只去零、不做舍入；小数部分全空时连小数点一起去掉（与旧壳 replace 同序）。
    let trimmed = if fmt.decimal_places > 0 {
        fixed.trim_end_matches('0').trim_end_matches('.')
    } else {
        fixed.as_str()
    };
    format!("{sign}{}{}", fmt.symbol, group_number_string(trimmed, 4))
}

/// 纯数字字符串按四位一组切组：从右向左每 `size` 位一组、半角逗号分隔；
/// 负号保留在最前，小数部分连续输出不插分隔符（词汇表「数字分组」）。
fn group_number_string(num_str: &str, size: usize) -> String {
    let (sign, body) = match num_str.strip_prefix('-') {
        Some(b) => ("-", b),
        None => ("", num_str),
    };
    let grouped = match body.split_once('.') {
        Some((int_part, frac)) => format!("{}.{}", join_groups(int_part, size), frac),
        None => join_groups(body, size),
    };
    format!("{sign}{grouped}")
}

/// 纯数字串从右向左每 `size` 位一组（输入不含符号与小数点，ASCII 数字安全切片）。
fn join_groups(digits: &str, size: usize) -> String {
    let mut groups: Vec<&str> = Vec::new();
    let mut end = digits.len();
    while end > 0 {
        let start = end.saturating_sub(size);
        groups.push(&digits[start..end]);
        end = start;
    }
    groups.reverse();
    groups.join(",")
}

/// 账户展示条目（名称 + 类型）：账户格三形态与借贷派生的消费面。
#[derive(Debug, Clone, PartialEq)]
struct AccountEntry {
    name: String,
    kind: AccountType,
}

/// 账户表（账户参考数据行集的只读投影，id → 名称/类型）：**只收用户可见账户**
/// （在用且未隐藏——旧壳 reference store accountMap 口径，`list_accounts` 排除
/// 隐藏与软删）。黑洞/隐藏/软删账户的引用不在表中：账户格渲染占位「-」（旧壳
/// AccountLink 同口径），借贷派生按缺失归一资金侧参与判定（旧壳
/// `directionalSide` 同口径）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccountTable {
    by_id: BTreeMap<String, AccountEntry>,
}

impl AccountTable {
    /// 从账户参考数据行集投影（行集由调用方按可见口径给出）。
    pub fn from_accounts<'a>(rows: impl IntoIterator<Item = &'a ledger_accounts::Account>) -> Self {
        Self {
            by_id: rows
                .into_iter()
                .map(|a| {
                    (
                        a.id.clone(),
                        AccountEntry {
                            name: a.name.clone(),
                            kind: a.kind,
                        },
                    )
                })
                .collect(),
        }
    }

    /// 账户 `id` 的名称；不在表中（隐藏/软删/缺失引用）返回 `None`（渲染 `-`）。
    fn name_of(&self, id: Option<&str>) -> Option<&str> {
        self.by_id.get(id?).map(|e| e.name.as_str())
    }

    /// 账户 `id` 的类型；不在表中返回 `None`（借贷派生按缺失归一资金侧）。
    fn type_of(&self, id: Option<&str>) -> Option<AccountType> {
        self.by_id.get(id?).map(|e| e.kind)
    }
}

/// 商户表（商户参考数据行集的只读投影，id → 名称）：**含软删商户**——旧壳
/// merchantMap 口径（在用 + 软删全量，历史交易显示共用，issue #191）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MerchantTable {
    by_id: BTreeMap<String, String>,
}

impl MerchantTable {
    /// 从商户参考数据行集投影（行集由调用方按含软删口径给出）。
    pub fn from_merchants<'a>(
        rows: impl IntoIterator<Item = &'a ledger_merchants::Merchant>,
    ) -> Self {
        Self {
            by_id: rows
                .into_iter()
                .map(|m| (m.id.clone(), m.name.clone()))
                .collect(),
        }
    }

    /// 商户 `id` 的名称；不在表中返回 `None`（渲染 `-`）。
    fn name_of(&self, id: Option<&str>) -> Option<&str> {
        self.by_id.get(id?).map(String::as_str)
    }
}

/// 借贷方向五态（旧壳 `lending.ts` 同闭集）：借出 / 收回 / 借入 / 还款 / 普通
/// 转账。借贷是 transfer + receivable/debt 账户的**派生视角**，不新增交易 kind；
/// 方向识别唯一依据是两端账户类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LendingDirection {
    /// 借出（资金 → 应收）。
    Lend,
    /// 收回（应收 → 资金）。
    Collect,
    /// 借入（负债 → 资金）。
    Borrow,
    /// 还款（资金 → 负债）。
    Repay,
    /// 普通转账（非借贷两端互转）。
    None,
}

/// 账户在借贷语境下的侧别（旧壳 `directionalSide` 归一口径）：receivable / debt
/// 之外（含类型缺失——黑洞/隐藏/软删账户不在账户表）归一资金侧参与判定；借贷
/// 语义由借贷侧唯一决定，不因对端缺失退回普通转账。
fn lending_side(kind: Option<AccountType>) -> LendingDirectionSide {
    match kind {
        Some(AccountType::Receivable) => LendingDirectionSide::Receivable,
        Some(AccountType::Debt) => LendingDirectionSide::Debt,
        _ => LendingDirectionSide::Fund,
    }
}

/// 借贷侧别闭集（判定内部用；资金侧含归一的缺失类型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LendingDirectionSide {
    Fund,
    Receivable,
    Debt,
}

/// 借贷方向派生（transfer + 两端账户类型 → 五态，旧壳 `deriveLendingDirection`
/// 的镜像）：非 transfer 恒普通转账；资金⇄应收 = 借出/收回、负债⇄资金 = 借入/
/// 还款；借贷账户互转、资金互转、两端均缺失 = 普通转账。账户类型经 [`AccountTable`]
/// 注入（与旧壳 `resolveLendingDirection` 的 accountType 解析器同形）。
pub fn derive_lending_direction(tx: &Transaction, accounts: &AccountTable) -> LendingDirection {
    if tx.kind != TransactionKind::Transfer {
        return LendingDirection::None;
    }
    match (
        lending_side(accounts.type_of(tx.account_id.as_deref())),
        lending_side(accounts.type_of(tx.to_account_id.as_deref())),
    ) {
        (LendingDirectionSide::Fund, LendingDirectionSide::Receivable) => LendingDirection::Lend,
        (LendingDirectionSide::Receivable, LendingDirectionSide::Fund) => LendingDirection::Collect,
        (LendingDirectionSide::Debt, LendingDirectionSide::Fund) => LendingDirection::Borrow,
        (LendingDirectionSide::Fund, LendingDirectionSide::Debt) => LendingDirection::Repay,
        _ => LendingDirection::None,
    }
}

/// 来源类型名（i18n zh-CN transactions.source.kind 词表同文案）：展示名空串时
/// 的兜底名（旧壳 `sourceCellView` 同口径）。
fn source_kind_label(kind: TransactionSourceKind) -> &'static str {
    match kind {
        TransactionSourceKind::InstallmentPlan => "分期计划",
        TransactionSourceKind::Subscription => "订阅计划",
        TransactionSourceKind::ScheduledTransfer => "定时转账计划",
        TransactionSourceKind::Policy => "保单",
        TransactionSourceKind::Item => "物品",
        TransactionSourceKind::Instrument => "标的",
    }
}

/// 来源状态标注（i18n zh-CN transactions.source.status 词表同文案）。
fn source_status_label(status: TransactionSourceStatus) -> &'static str {
    match status {
        TransactionSourceStatus::Cancelled => "已取消",
        TransactionSourceStatus::Disposed => "已处置",
        TransactionSourceStatus::Deleted => "已删除",
    }
}

/// 购买项件数标注（旧壳 `purchaseQtyText` 单点同文案）。
fn purchase_qty_text(quantity: i64) -> String {
    format!("共 {quantity} 件")
}

/// 账户格原子（旧壳 `accountAtom` 文本镜像）：可见账户显名；黑洞/隐藏/软删
/// 账户（不在账户表）占位「-」。
fn account_atom(accounts: &AccountTable, id: Option<&str>) -> String {
    accounts.name_of(id).unwrap_or("-").to_owned()
}

/// 账户格文本（旧壳 `accountCellView` 三形态的文本镜像）：出资分解行（出资子行
/// 在场，词汇表「出资项」读侧口径）= 首条出资账户 + 标注（多账户「等 N 账户」/
/// 同账户多条「同账户 N 笔」/ 单条无标注），refund 派生分解另带「按比例自动分解」
/// （旧壳第二行标注的平铺镜像）；转账 = 「转出 → 转入」；buy/sell = 双账户
/// （资金流出方在前，ADR-0096 决策 6）；其余 = 单账户。
fn account_cell_text(tx: &Transaction, accounts: &AccountTable) -> String {
    if let Some(first) = tx.fundings.first() {
        let distinct = tx
            .fundings
            .iter()
            .map(|f| f.account_id.as_str())
            .collect::<BTreeSet<_>>()
            .len();
        let mut text = account_atom(accounts, Some(&first.account_id));
        if distinct > 1 {
            text.push_str(&format!(" 等 {distinct} 账户"));
        } else if tx.fundings.len() > 1 {
            text.push_str(&format!(" 同账户 {} 笔", tx.fundings.len()));
        }
        if tx.fundings.iter().any(|f| f.derived) {
            text.push_str(" 按比例自动分解");
        }
        return text;
    }
    let single = account_atom(accounts, tx.account_id.as_deref());
    if tx.kind == TransactionKind::Transfer
        && let Some(to) = tx.to_account_id.as_deref()
    {
        return format!("{single} → {}", account_atom(accounts, Some(to)));
    }
    if tx.kind == TransactionKind::Buy
        && let Some(funding) = tx.funding_account_id.as_deref()
    {
        return format!("{} → {single}", account_atom(accounts, Some(funding)));
    }
    if tx.kind == TransactionKind::Sell
        && let Some(funding) = tx.funding_account_id.as_deref()
    {
        return format!("{single} → {}", account_atom(accounts, Some(funding)));
    }
    single
}

/// 商户格文本（旧壳商户格口径）：商户映射命中（含软删商户历史引用）显名；
/// 无引用或解析不到渲染「-」。
fn merchant_cell_text(tx: &Transaction, merchants: &MerchantTable) -> String {
    merchants
        .name_of(tx.merchant_id.as_deref())
        .unwrap_or("-")
        .to_owned()
}

/// 来源格文本（旧壳 `sourceCellView` 的文本镜像，spec #704 词汇表「来源列」）：
/// 展示名空串按来源类型名兜底；状态标注随名空格拼接（旧壳状态徽章的平铺镜像）；
/// 无来源行渲染「-」（旧壳来源列同口径）。
fn source_cell_text(tx: &Transaction) -> String {
    let Some(source) = tx.source.as_ref() else {
        return "-".to_owned();
    };
    let name = if source.display_name.is_empty() {
        source_kind_label(source.kind)
    } else {
        source.display_name.as_str()
    };
    match source.status {
        Some(status) => format!("{name} {}", source_status_label(status)),
        None => name.to_owned(),
    }
}

/// 装配消费面（数据到达时随行集一并备齐的参考数据与开关）：装配函数的
/// 唯一入参形态——参考数据表与设备开关随票增长收在此处，不散成位置参数。
pub struct RowContext<'a> {
    /// 币种展示参数表。
    pub currencies: &'a CurrencyTable,
    /// 分类路径表。
    pub categories: &'a CategoryTable,
    /// 账户表（在用可见口径；账户格与借贷派生）。
    pub accounts: &'a AccountTable,
    /// 商户表（含软删口径）。
    pub merchants: &'a MerchantTable,
    /// 金额隐私模式（设备轻量设置项；真源与水合见 [`crate::app`]）。
    pub privacy_enabled: bool,
}

/// 金额隐私掩码（spec #564 掩码恒等性 / issue #566）：固定四点、无负号、无
/// 币符、不随语言与数量级变化——与旧壳 `AMOUNT_PRIVACY_MASK` 同字面。
pub const AMOUNT_PRIVACY_MASK: &str = "••••";

/// 装配行集（数据到达时一次性产出全部格文本）：读路径行 → 表格行的唯一入口。
///
/// 一笔交易展开 1..N 行（ADR-0138 决策 13）：带购买项的交易**每个购买项各占
/// 一行**（行序 = 交易序、同单按顺序位相邻），无购买项原样一行。派生行不是
/// 独立排序/分页单元——分页、`total` 与「共 N 条」仍按交易计（决策 14，total
/// 由域读路径返回、不经本层，购买项行不虚增计数）。
pub fn assemble_rows(items: &[Transaction], ctx: &RowContext) -> Vec<RowCells> {
    items
        .iter()
        .flat_map(|tx| expand_transaction_rows(tx, ctx))
        .collect()
}

/// 一笔交易 → 1..N 行：带购买项逐项一行，其余原样一行（[`assemble_rows`] 的
/// 展开单点）。
fn expand_transaction_rows(tx: &Transaction, ctx: &RowContext) -> Vec<RowCells> {
    if tx.purchases.is_empty() {
        vec![derived_row(tx, None, ctx)]
    } else {
        (0..tx.purchases.len())
            .map(|index| derived_row(tx, Some(index), ctx))
            .collect()
    }
}

/// 单个显示行的装配（购买项序位 `Some(i)` = 订单块内第 i 个购买项行；`None` =
/// 无购买项行）：订单级列只在块首行带值、续行留空（旧壳 rowSpan 合并格的内容
/// 只出现一次）；分类与备注列逐行。
fn derived_row(tx: &Transaction, purchase_index: Option<usize>, ctx: &RowContext) -> RowCells {
    let purchase = purchase_index.map(|index| &tx.purchases[index]);
    let order_level = purchase_index.is_none() || purchase_index == Some(0);
    // 订单级格：块首行带值，续行留空（DataTable 无纵向合并，见 RowCells 注）。
    let order = |value: String| if order_level { value } else { String::new() };
    let amount = match amount_display_cents(tx) {
        None => "-".to_owned(),
        // 隐私模式收口在金额格式化层（旧壳 formatAmount 同位）：调用点
        // 零判断；`-`（份额调整无金额）是空值口径、不是金额，照旧渲染。
        Some(_) if ctx.privacy_enabled => AMOUNT_PRIVACY_MASK.to_owned(),
        Some(cents) => format_amount_basic(cents, &ctx.currencies.format_of(&tx.currency_code)),
    };
    let category = match purchase {
        Some(p) => ctx.categories.path_of(p.category_id.as_deref()),
        None => ctx.categories.path_of(tx.category_id.as_deref()),
    }
    .unwrap_or_else(|| "-".to_owned());
    let note = match purchase {
        // 备注列一列两用：购买项行逐行显示商品名 + 件数（价格存而不显示）。
        Some(p) => format!("{} {}", p.name, purchase_qty_text(p.quantity)),
        None => tx.note.clone().unwrap_or_else(|| "-".to_owned()),
    };
    RowCells {
        id: tx.id.clone(),
        date: order(tx.date.clone()),
        kind: order(kind_label(tx.kind, derive_lending_direction(tx, ctx.accounts)).to_owned()),
        category,
        merchant: order(merchant_cell_text(tx, ctx.merchants)),
        account: order(account_cell_text(tx, ctx.accounts)),
        source: order(source_cell_text(tx)),
        note,
        amount: order(amount),
        amount_semantic: amount_semantic(tx.kind),
    }
}

/// 翻页方向（快捷键语义的纯半边，ADR-0140）：上一页 / 下一页。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageStep {
    /// 上一页。
    Prev,
    /// 下一页。
    Next,
}

/// 翻页决策单点（旧壳翻页快捷键与分页条 on_change 共用的步进半边，
/// ADR-0140 决策 2）：以当前页码为基准步进一页，首/末页越界返回 None
/// （幂等无操作，不提示不抖动）；页码由调用方在读派发前现取（「以最新
/// 页码状态为基准」——快捷键只是页码状态的又一个触发面）。
pub fn next_page(current: u32, total_pages: u32, step: PageStep) -> Option<u32> {
    match step {
        PageStep::Prev => current.checked_sub(1).filter(|p| *p >= 1),
        PageStep::Next => {
            let next = current + 1;
            (next <= total_pages.max(1)).then_some(next)
        }
    }
}

/// 总页数（offset 分页，ADR-0008）：末页不满一页也算一页；`total = 0` 恒 1
/// （分页条在单页时由 UI 层收起，见 [`crate::app`]）。
pub fn total_pages(total: i64) -> u32 {
    let pages = (total.max(0) as u64).div_ceil(PAGE_SIZE as u64);
    pages.max(1) as u32
}

/// 删除一行后的页码决策（旧壳 afterRowDelete / ADR-0045 的决策半边，#2022）：
/// 输入删前快照（当前页码与总数），本页删后剩 0 条且当前页非第一页 → 回退一页
/// （`Some`），其余一律保持当前页（`None`）。本页行数由总数与页码推算
/// （`total - (page-1)×PAGE_SIZE` 钳到页径）；被删行必在当前页（行右键的行
/// 就是屏上行），推算口径与旧壳「本页行数 - 1」同值。
pub fn page_after_row_delete(page: u32, total_before: i64) -> Option<u32> {
    let on_page = total_before
        .saturating_sub((page.max(1) as i64 - 1) * PAGE_SIZE as i64)
        .min(PAGE_SIZE as i64);
    if on_page <= 1 && page > 1 {
        Some(page - 1)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 预算装配族（#2033）：预算视图的行格文本、进度口径与分类树选项——纯 Rust、
// 无 UI 依赖，纪律同本模块头注。文案与口径以旧壳为对拍基准（budget i18n
// zh-CN 词表 / formatAmount 无币种参数形态 / categoryPath 与 issue #356 路径
// 名口径）；金额整数分，本层只做展示换算，不另立第二套舍入。
// ---------------------------------------------------------------------------

/// 预算周期 zh-CN 标签（旧壳 budget.period 词表）。
pub fn budget_period_label(period: ledger_budget::BudgetPeriod) -> &'static str {
    match period {
        ledger_budget::BudgetPeriod::Monthly => "按月",
        ledger_budget::BudgetPeriod::Yearly => "按年",
    }
}

/// 预算执行进度百分比：消耗/上限封顶 100（旧壳 progressPercentage 同口径：
/// 四舍五入、非正上限归 0）。
pub fn budget_progress_percent(spent_cents: i64, amount_cents: i64) -> f32 {
    if amount_cents <= 0 {
        return 0.0;
    }
    ((spent_cents as f64 / amount_cents as f64) * 100.0)
        .round()
        .clamp(0.0, 100.0) as f32
}

/// 年度汇总卡进度百分比：本年已支 / 年度预算总额（年化口径），封顶 100
/// （旧壳 summaryPercentage 同口径）。
pub fn budget_summary_percent(spent_cents: i64, annual_total_cents: i64) -> f32 {
    budget_progress_percent(spent_cents, annual_total_cents)
}

/// 预算金额文本（旧壳 formatAmount 无币种参数形态：无符号、两位小数、去尾零、
/// zh 四位一组）；隐私开啓时与交易表格同掩码。预算金额是默认币种单币种口径
/// （域模型定），展示不带币符与旧壳同。
pub fn budget_amount_text(cents: i64, privacy: bool) -> String {
    if privacy {
        return AMOUNT_PRIVACY_MASK.to_owned();
    }
    format_amount_basic(cents, &CurrencyFormat::default())
}

/// 预算行状态标签（旧壳 budget.status 词表）。
pub fn budget_status_text(over_budget: bool) -> &'static str {
    if over_budget { "超支" } else { "正常" }
}

/// 年度汇总状态标签（旧壳 budget.summary 词表：「已超标」与行状态词不同）。
pub fn budget_summary_status_text(over_budget: bool) -> &'static str {
    if over_budget { "已超标" } else { "正常" }
}

/// 预算分类显示名（旧壳 categoryDisplayName issue #356 口径）：顶级为自身名，
/// 子分类为「父 > 子」路径名；id 解析不到（守卫生效前的历史孤儿预算）回退
/// 「未分类」，不报错。
pub fn budget_category_display(categories: &CategoryTable, id: &str) -> String {
    categories
        .path_of(Some(id))
        .unwrap_or_else(|| "未分类".to_owned())
}

/// 分类下拉选项（拼音可搜下拉的数据源条目）：id + 显示 label。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryOption {
    /// 分类 id（提交载荷）。
    pub id: String,
    /// 显示名（顶级自身名，子分类「父 > 子」路径名）。
    pub label: String,
}

/// 分类下拉选项（旧壳 categoryOptions：`treeCategoryOptions("expense")`
/// flatMap 同口径）：只收支出分类（顶级 + 子分类，收入分类被 kind 过滤排除），
/// 父按 sort_order、子紧跟父且自身按 sort_order（子分类 label 用「父 > 子」
/// 路径名，拼音可搜对 label 整体匹配，父名/子名拼音均可命中）。输入行集只
/// 收在用分类（软删不进选项，[`crate::boot::category_rows`] 口径）。
pub fn expense_category_options(rows: &[ledger_categories::Category]) -> Vec<CategoryOption> {
    category_kind_options("expense", rows)
}

/// 指定 kind 的分类下拉选项（表单族泛化，#2034）：[`expense_category_options`
/// 的 kind 参数化形态——支出/收入两表单共享同一树序与路径名口径，kind 字面
/// 量是分类实体自身的 kind 列闭集（`expense` / `income`）。其余同上。
pub fn category_kind_options(
    kind: &str,
    rows: &[ledger_categories::Category],
) -> Vec<CategoryOption> {
    let mut roots: Vec<&ledger_categories::Category> = rows
        .iter()
        .filter(|c| c.kind == kind && c.parent_id.is_none())
        .collect();
    roots.sort_by_key(|c| c.sort_order);
    let mut options = Vec::new();
    for root in roots {
        options.push(CategoryOption {
            id: root.id.clone(),
            label: root.name.clone(),
        });
        let mut children: Vec<&ledger_categories::Category> = rows
            .iter()
            .filter(|c| c.kind == kind && c.parent_id.as_deref() == Some(root.id.as_str()))
            .collect();
        children.sort_by_key(|c| c.sort_order);
        for child in children {
            options.push(CategoryOption {
                id: child.id.clone(),
                label: format!("{} > {}", root.name, child.name),
            });
        }
    }
    options
}

/// 商户筛选下拉选项（issue #2037，旧壳 `merchantOptions` 同口径）：来自含软删
/// 全量商户行集（[`crate::boot::merchant_rows`] 口径，issue #191——软删商户
/// 仍有历史交易，需可被选中过滤），按名称排序保证稳定（旧壳 `localeCompare
/// "zh"` 的本壳近似：字符串字节序，中文排序稳定性同诉求）。纯投影。
pub fn merchant_filter_options(rows: &[ledger_merchants::Merchant]) -> Vec<CategoryOption> {
    let mut merchants: Vec<&ledger_merchants::Merchant> = rows.iter().collect();
    merchants.sort_by(|a, b| a.name.cmp(&b.name));
    merchants
        .into_iter()
        .map(|m| CategoryOption {
            id: m.id.clone(),
            label: m.name.clone(),
        })
        .collect()
}

/// 类型筛选下拉选项（issue #2037，旧壳 `kindOptions` 同口径）：可选集 = 四通用
/// kind（ADR-0135 主列表收窄，[`crate::tx_filter::MAIN_LIST_KINDS`] 单名单），
/// label 经 [`kind_label`] 随闭集序渲染（旧壳决议 6：展示序 = 闭集序）。
pub fn kind_filter_options() -> Vec<CategoryOption> {
    crate::tx_filter::MAIN_LIST_KINDS
        .iter()
        .map(|kind| CategoryOption {
            id: kind.as_str().to_owned(),
            label: kind_label(*kind, LendingDirection::None).to_owned(),
        })
        .collect()
}

/// 列表空态文案分型（issue #2037，旧壳 `emptyDescription` 两档同源同形）：
/// 搜索态无命中「无匹配结果」（search.noResults）；列表态筛选无结果
/// 「没有符合条件的交易」（list.emptyFiltered）；列表态空库「暂无数据」
/// （list.empty）。纯函数，headless 空态分型断言的对口。
pub fn list_empty_text(searched: bool, filters_active: bool) -> &'static str {
    if searched {
        "无匹配结果"
    } else if filters_active {
        "没有符合条件的交易"
    } else {
        "暂无数据"
    }
}

// ---------------------------------------------------------------------------
// 搜索视图装配家族（#2040）：查询判定 / 筛选激活判定 / 已应用筛选描述拼接 /
// 空态文案分型（旧壳 SearchView 同名 computed 的纯函数化，离线单测对口；
// 金额口径沿换算家族单点 [`amount_text_to_cents`]，展示格式沿缺省币种格式
// ——预算视图先例同口径，见 [`crate::search`] 模块注）。
// ---------------------------------------------------------------------------

/// 查询具备判定（旧壳搜索页 hasQuery 同口径）：关键词非空或任一筛选在场
/// ——仅筛选也可出结果（无关键字仅筛选查询）。纯函数。
pub fn search_has_query(
    keyword: &str,
    amount_min_cents: Option<i64>,
    amount_max_cents: Option<i64>,
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> bool {
    !keyword.trim().is_empty()
        || search_filters_active(amount_min_cents, amount_max_cents, date_from, date_to)
}

/// 筛选激活判定（旧壳搜索页 filtersActive 同口径：金额任一端或期间任一端
/// 在场；关键词不算筛选——清除按钮只清筛选不清关键词）。纯函数。
pub fn search_filters_active(
    amount_min_cents: Option<i64>,
    amount_max_cents: Option<i64>,
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> bool {
    amount_min_cents.is_some()
        || amount_max_cents.is_some()
        || date_from.is_some()
        || date_to.is_some()
}

/// 已应用筛选描述拼接（旧壳 activeFilterDescriptions 同词表同序：金额双端
/// 「金额 x ~ y」→ 金额单端「最低/最高 x」→ 期间「起始/结束 d」；金额以
/// 缺省币种格式展示，分隔符「、」）。纯函数，装配层单测对口。
pub fn search_filter_descriptions(
    amount_min_cents: Option<i64>,
    amount_max_cents: Option<i64>,
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> Vec<String> {
    let fmt = CurrencyFormat::default();
    let mut parts = Vec::new();
    match (amount_min_cents, amount_max_cents) {
        (Some(min), Some(max)) => parts.push(format!(
            "金额 {} ~ {}",
            format_amount_basic(min, &fmt),
            format_amount_basic(max, &fmt)
        )),
        (Some(min), None) => parts.push(format!("最低 {}", format_amount_basic(min, &fmt))),
        (None, Some(max)) => parts.push(format!("最高 {}", format_amount_basic(max, &fmt))),
        (None, None) => {}
    }
    if let Some(from) = date_from {
        parts.push(format!("起始 {from}"));
    }
    if let Some(to) = date_to {
        parts.push(format!("结束 {to}"));
    }
    parts
}

/// 搜索视图空态文案分型（旧壳搜索页两态同源同形）：未搜索「输入关键字或
/// 设置筛选开始搜索」（search.emptyPlaceholder，占位与空结果的区分门）；
/// 已搜索无命中「无匹配结果」（search.noResults）。纯函数。
pub fn search_empty_text(searched: bool) -> &'static str {
    if searched {
        "无匹配结果"
    } else {
        "输入关键字或设置筛选开始搜索"
    }
}

/// 元文本 → 整数分（旧壳 yuanToCents 同口径：十进制文本、四舍五入到分、拒绝
/// 空白/非数/科学计数）。None = 解析失败（呈现面归调用方校验文案）。
pub fn budget_yuan_to_cents(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    if trimmed.is_empty()
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_digit() || ch == '.' || ch == '-')
    {
        return None;
    }
    let yuan: f64 = trimmed.parse().ok()?;
    if !yuan.is_finite() {
        return None;
    }
    let cents = (yuan * 100.0).round();
    // i64 范围外的值拒绝（旧壳 Number.isSafeInteger 同意图）。
    if cents.abs() >= 9.0e18 {
        return None;
    }
    Some(cents as i64)
}

/// 整数分 → 元文本（编辑回填：旧壳 centsToYuan 后填入数字输入框的形态）。
pub fn budget_cents_to_yuan_text(cents: i64) -> String {
    let yuan = cents as f64 / 100.0;
    if yuan == yuan.trunc() {
        format!("{}", yuan as i64)
    } else {
        format!("{}", yuan)
    }
}

// ---------------------------------------------------------------------------
// 交易表单族（#2034）：金额文本判定 / 换算家族（旧壳 packages/utils field-error
// 判定族与 packages/money 换算族的重生，两段口径各自镜像：判定放行超精度尾部
// 小数点的宽松形状、换算拒绝——与旧壳「判定拦红态、装配器 fail fast 掓回」
// 两段语义一致）。渲染期零格式化纪律不变：换算只在表单提交/回填时机调用。
// ---------------------------------------------------------------------------

/// 金额文本判定（旧壳 judgeAmountText 同口径）：输入是输入框中原样保留的原始
/// 文本（不拦截口径）。空串/纯空白 → `Empty`；形状非法 → `ParseError`；形状
/// 合法但超两位小数 → `OverPrecision`；合法 → `Ok` 携带元的数值（负数可解析，
/// 业务类校验留提交通道）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AmountJudgment {
    /// 必填为空（是否红由呈现按时机判定）。
    Empty,
    /// 形状非法（非数 / 双小数点 / 科学计数等）。
    ParseError,
    /// 形状合法但超两位小数（金额以整数分表达）。
    OverPrecision,
    /// 合法，携带元的数值。
    Ok(f64),
}

/// 金额文本判定（旧壳 field-error 判定族的重生）：形状 = 可选负号 +（整数部分
/// + 可选小数点及小数）或（纯小数）——`12.` 合法、`1.2.3` 非法；两端空白容差。
pub fn judge_amount_text(text: &str) -> AmountJudgment {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return AmountJudgment::Empty;
    }
    let shape_ok = {
        let body = trimmed.strip_prefix('-').unwrap_or(trimmed);
        match body.split_once('.') {
            Some((int_part, frac_part)) => {
                (int_part.chars().all(|ch| ch.is_ascii_digit())
                    || (int_part.is_empty()
                        && !frac_part.is_empty()
                        && frac_part.chars().all(|ch| ch.is_ascii_digit())))
                    && frac_part.chars().all(|ch| ch.is_ascii_digit())
            }
            None => !body.is_empty() && body.chars().all(|ch| ch.is_ascii_digit()),
        }
    };
    if !shape_ok {
        return AmountJudgment::ParseError;
    }
    let dot = trimmed.find('.');
    if let Some(dot) = dot
        && trimmed.len() - dot - 1 > 2
    {
        return AmountJudgment::OverPrecision;
    }
    trimmed
        .parse::<f64>()
        .ok()
        .filter(|yuan| yuan.is_finite())
        .map(AmountJudgment::Ok)
        .unwrap_or(AmountJudgment::ParseError)
}

/// 金额文本 → 整数分（旧壳 yuanToCents 单点换算口径的重生，元转分全仓壳侧
/// 只此一处）：十进制文本、四舍五入到分、拒绝空白 / 尾随小数点 / 非数 /
/// 科学计数 / 非有限值与 i64 范围外。None = 解析失败（呈现面归调用方校验文案）。
pub fn amount_text_to_cents(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    // 形状：可选负号，（整数部分 + 可选小数点 + 至少一位小数）或（纯小数）或
    // （纯整数）——`12.` 在换算口径拒绝（旧壳 yuanToCents 同口径）。
    let body = trimmed.strip_prefix('-').unwrap_or(trimmed);
    let (int_part, frac_part) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    let int_ok = !int_part.is_empty() && int_part.chars().all(|ch| ch.is_ascii_digit());
    let frac_ok = match frac_part {
        Some(f) => !f.is_empty() && f.chars().all(|ch| ch.is_ascii_digit()),
        None => true,
    };
    if !int_ok || !frac_ok {
        return None;
    }
    let yuan: f64 = trimmed.parse().ok()?;
    if !yuan.is_finite() {
        return None;
    }
    let cents = (yuan * 100.0).round();
    // i64 范围外的值拒绝（旧壳 Number.isSafeInteger 同意图）。
    if cents.abs() >= 9.0e18 {
        return None;
    }
    Some(cents as i64)
}

/// 整数分 → 金额输入回填文本（旧壳 `String(centsToYuan(cents, currency))`
/// 同形：按币种小数位换算，整数裁零）。回填锚点：合法整数分至多产生币种
/// 小数位的小数，判定必为 `Ok`，不显红态。
pub fn cents_to_amount_text(cents: i64, decimal_places: u32) -> String {
    let yuan = cents as f64 / 10f64.powi(decimal_places as i32);
    if yuan == yuan.trunc() {
        format!("{}", yuan as i64)
    } else {
        format!("{}", yuan)
    }
}

// ---------------------------------------------------------------------------
// 基础收支表单装配器（#2034；旧壳 transaction-input.ts「TransactionInput 装配
// 器」的重生）：表单状态 → 完整 TransactionInput 的唯一装配接缝——同步纯函数，
// per-kind 字段矩阵、元转分与日期转换在此收口，新增表单形态只改矩阵。非法输
// 入 fail fast 显式报错、不静默兜底（旧壳同一原则）。商户解析不在此接缝另立
// 第二份「匹配即建」（域写入协议自带 merchant_name 单点），装配器只消费表单层
// 已成语义闭集的选择器状态。
// ---------------------------------------------------------------------------

/// 商户输入语义闭集（表单选择器状态 → wire 字段）：旧壳 resolveMerchantRef
/// 五分支的收口形态——id 与自由文本二选一，编辑面原值保留单列一个变体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MerchantRef {
    /// 不挂商户。
    None,
    /// 编辑面原样保留（提交值与行上原值相同：后端 existing_merchant_id
    /// unchanged 语义跳过在用校验，软删商户的历史引用原样保留）。
    Keep(String),
    /// 用户选定在用商户（id 提交）。
    Selected(String),
    /// 自由文本（merchant_name 落 wire：后端精确匹配在用名复用、未命中即建）。
    NewName(String),
}

/// 基础收支表单提交状态（提交时点的业务字段；日期已是本地日历日）。
#[derive(Debug, Clone, PartialEq)]
pub struct BasicFormSubmit {
    /// 交易类型（本票闭集：支出 / 收入）。
    pub kind: TransactionKind,
    /// 金额原始文本（判定族消费的原样输入；换算在装配器内收口）。
    pub amount_text: String,
    /// 账户（必选）。
    pub account_id: Option<String>,
    /// 分类（可选；None = 无分类交易）。
    pub category_id: Option<String>,
    /// 商户语义（见 [`MerchantRef`]）。
    pub merchant: MerchantRef,
    /// 可选保单引用（支出/收入可挂）。
    pub policy_id: Option<String>,
    /// 备注（空串落 None）。
    pub note: String,
    /// 交易日期（本地日历日语义，旧壳 toLocalDateISO 单点同口径）。
    pub date: chrono::NaiveDate,
    /// 来源订单号（表单无输入位：创建路径 None，编辑路径透传行上原值——
    /// 全字段替换下缺省即静默清除订单锚点，旧壳同语义）。
    pub source_order_no: Option<String>,
}

/// 基础收支表单校验错误（壳层呈现词表的载体；错误条位呈现，旧壳 toast 同位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BasicFormError {
    /// 账户未选（旧壳 settings.categories.msg.accountRequired 词表）。
    AccountRequired,
    /// 金额非正数（旧壳 settings.categories.msg.amountRequired 词表：业务类
    /// 校验保留提交通道文案，不动红态拦截——ADR-0058 业务不成立不属字段错误态）。
    AmountNotPositive,
    /// 金额换算失败（判定放行、换算拒绝的兜底位；正常流不可达）。
    AmountInvalid,
}

impl BasicFormError {
    /// 错误条位文案（zh-CN 词表）。
    pub fn as_text(self) -> &'static str {
        match self {
            Self::AccountRequired => "请选择账户",
            Self::AmountNotPositive => "请输入金额",
            Self::AmountInvalid => "金额无效，请输入不超过两位小数的金额",
        }
    }
}

/// 基础收支表单状态 → 完整 TransactionInput（旧壳 buildExpenseIncomeInput
/// 同名接缝）：校验 fail fast → 金额换算 → per-kind 矩阵落位 → 表单承载字段
/// 覆写。`currency_code` 由调用方随所选账户推导传入（币种一致性写入不变量的
/// 前端等价：记账币种 = 账户币种，ADR-0134 决策 5）。
pub fn assemble_basic_transaction(
    submit: BasicFormSubmit,
    currency_code: String,
) -> Result<ledger_transaction::TransactionInput, BasicFormError> {
    // 校验段（fail fast 顺序镜像旧壳：账户先于金额业务校验）。
    let Some(account_id) = submit.account_id.filter(|id| !id.trim().is_empty()) else {
        return Err(BasicFormError::AccountRequired);
    };
    let Some(amount_cents) = amount_text_to_cents(&submit.amount_text) else {
        return Err(BasicFormError::AmountInvalid);
    };
    if amount_cents <= 0 {
        return Err(BasicFormError::AmountNotPositive);
    }
    // per-kind 字段矩阵（income / expense 行，旧壳 KIND_FIELD_MATRIX 同形）：
    // 基础收支不携带的字段全部空位。
    Ok(ledger_transaction::TransactionInput {
        kind: submit.kind,
        amount_cents,
        currency_code,
        account_id: Some(account_id),
        to_account_id: None,
        funding_account_id: None,
        funding: Vec::new(),
        purchases: Vec::new(),
        category_id: submit.category_id,
        merchant_id: match &submit.merchant {
            MerchantRef::None | MerchantRef::NewName(_) => None,
            MerchantRef::Keep(id) | MerchantRef::Selected(id) => Some(id.clone()),
        },
        merchant_name: match &submit.merchant {
            MerchantRef::NewName(name) => Some(name.clone()),
            _ => None,
        },
        policy_id: submit.policy_id,
        refund_of_transaction_id: None,
        note: (!submit.note.trim().is_empty()).then(|| submit.note.clone()),
        source_order_no: submit.source_order_no,
        date: submit.date.to_string(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        origin: None,
        fx_rate: None,
        idempotency_key: None,
    })
}

// ---------------------------------------------------------------------------
// 转账 / 借贷 / 退款表单装配器（#2035；旧壳 transaction-input.ts
// buildTransferInput / buildRefundInput 与 useLendingForm 的重生）：表单②三
// 形态的表单状态 → 完整 TransactionInput 的装配接缝。纪律同基础收支装配器：
// 同步纯函数、校验 fail fast 显式报错、金额换算与日期转换在此收口；per-kind
// 矩阵落位（转账双账户 / 退款原交易引用）。借贷是 transfer 的派生视角
// （ADR-0053），提交产物与转账同构，仅方向状态与账户侧别过滤不同。
// ---------------------------------------------------------------------------

/// 借贷表单方向闭集（表单状态；普通转账 none 只在方向派生
/// [`LendingDirection`] 时出现，不是表单状态——旧壳 LendingFormDirection 同
/// 闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LendingFormDirection {
    /// 借出（资金 → 应收）。
    Lend,
    /// 收回（应收 → 资金）。
    Collect,
    /// 借入（负债 → 资金）。
    Borrow,
    /// 还款（资金 → 负债）。
    Repay,
}

/// 借贷方向 zh-CN 词（旧壳 transactions.lending 词表单点）：表单方向切换钮、
/// 提交钮「记{dir}」与列表类型标签（[`kind_label`] 借贷分支）同词表，多处
/// 消费不重复书写。
fn lending_word(direction: LendingFormDirection) -> &'static str {
    match direction {
        LendingFormDirection::Lend => "借出",
        LendingFormDirection::Collect => "收回",
        LendingFormDirection::Borrow => "借入",
        LendingFormDirection::Repay => "还款",
    }
}

/// 借贷表单方向标签（方向切换钮与提交钮「记{dir}」的文案源）。
pub fn lending_direction_label(direction: LendingFormDirection) -> &'static str {
    lending_word(direction)
}

/// 账户在借贷语境下的侧别（表单账户选择器过滤闭集，旧壳 LendingAccountSide
/// 同形）：资金账户 / 借出款（receivable）/ 负债（debt）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LendingAccountSide {
    /// 资金账户（cash/bank/credit/ewallet/investment/other）。
    Fund,
    /// 借出款（receivable）。
    Receivable,
    /// 负债（debt）。
    Debt,
}

/// 各方向允许的（转出侧，转入侧）账户侧别——借贷表单账户选择器过滤的唯一
/// 依据，与方向派生 [`derive_lending_direction`] 同源共表（「过滤哪侧账户」
/// 与「识别什么方向」永不漂移，旧壳 LENDING_DIRECTION_SIDES 同表）：借出/收回
/// 是「资金账户 ↔ 借出款」，借入/还款是「负债 ↔ 资金账户」。
pub fn lending_direction_sides(
    direction: LendingFormDirection,
) -> (LendingAccountSide, LendingAccountSide) {
    match direction {
        LendingFormDirection::Lend => (LendingAccountSide::Fund, LendingAccountSide::Receivable),
        LendingFormDirection::Collect => (LendingAccountSide::Receivable, LendingAccountSide::Fund),
        LendingFormDirection::Borrow => (LendingAccountSide::Debt, LendingAccountSide::Fund),
        LendingFormDirection::Repay => (LendingAccountSide::Fund, LendingAccountSide::Debt),
    }
}

/// 账户类型是否属于某侧别：receivable/debt 之外的资金类归资金侧；**账户类型
/// 缺失（黑洞占位/已删/不可查）不命中任何侧**（旧壳 accountMatchesSide 同
/// 语义——表单过滤比方向派生严格：缺失账户不进新选择；方向派生的缺失归一
/// 另见私有 [`lending_side`]）。
pub fn account_matches_side(kind: Option<AccountType>, side: LendingAccountSide) -> bool {
    match kind {
        Some(AccountType::Receivable) => side == LendingAccountSide::Receivable,
        Some(AccountType::Debt) => side == LendingAccountSide::Debt,
        Some(_) => side == LendingAccountSide::Fund,
        None => false,
    }
}

/// 方向切换的已选账户处置（旧壳 useLendingForm.setDirection 决策半边的纯
/// 函数形态）：反向方向的既有两端贴合新方向 → 交换两端（借出 ↔ 收回 /
/// 借入 ↔ 还款，不丢已选账户）；其余情况保留仍属允许侧别的选择、清掉越侧
/// 的（账户类型缺失不命中任何侧 → 清除），不存在中间态残留。`type_of` 是
/// 账户 id → 类型解析器（调用方从账户行集注入）。
pub fn lending_direction_swap(
    current: LendingFormDirection,
    next: LendingFormDirection,
    from_id: Option<&str>,
    to_id: Option<&str>,
    type_of: impl Fn(&str) -> Option<AccountType>,
) -> (Option<String>, Option<String>) {
    let (owned_from, owned_to) = (from_id.map(str::to_owned), to_id.map(str::to_owned));
    if current == next {
        return (owned_from, owned_to);
    }
    let (from_side, to_side) = lending_direction_sides(next);
    let fits = |id: Option<&str>, side: LendingAccountSide| {
        id.is_some_and(|id| account_matches_side(type_of(id), side))
    };
    if fits(to_id, from_side) && fits(from_id, to_side) {
        // 反向方向贴合：交换两端。
        return (owned_to, owned_from);
    }
    let keep_from = fits(from_id, from_side);
    let keep_to = fits(to_id, to_side);
    (
        keep_from.then(|| owned_from.unwrap_or_default()),
        keep_to.then(|| owned_to.unwrap_or_default()),
    )
}

/// 转账 / 借贷表单提交状态（提交时点的业务字段；日期已是本地日历日）。
/// 借贷变体的提交产物与转账同构（kind=transfer + 双账户），仅商户输入面
/// 不同（普通转账表单无输入位，编辑原样携带——ADR-0092 决策 5）。
#[derive(Debug, Clone, PartialEq)]
pub struct TransferFormSubmit {
    /// 金额原始文本（判定族消费的原样输入；换算在装配器内收口）。
    pub amount_text: String,
    /// 转出账户（必选）。
    pub account_id: Option<String>,
    /// 转入账户（必选；域 Writer 守卫要求两端账户币种一致且等于交易币种）。
    pub to_account_id: Option<String>,
    /// 商户语义（见 [`MerchantRef`]）：借贷变体可关联商户（ADR-0092）；普通
    /// 转账编辑原样携带（Keep），创建恒 None。
    pub merchant: MerchantRef,
    /// 备注（空串落 None）。
    pub note: String,
    /// 交易日期（本地日历日语义）。
    pub date: chrono::NaiveDate,
    /// 来源订单号（表单无输入位：创建 None，编辑透传行上原值）。
    pub source_order_no: Option<String>,
}

/// 转账表单校验错误（壳层呈现词表的载体；错误条位呈现，旧壳 toast 同位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferFormError {
    /// 转出账户未选（旧壳 transactions.form.warnSelectFromAccount 词表）。
    FromAccountRequired,
    /// 转入账户未选（旧壳 transactions.form.warnSelectToAccount 词表）。
    ToAccountRequired,
    /// 两端同账户（旧壳 transactions.form.warnSameAccount 词表）。
    SameAccount,
    /// 金额非正数（旧壳 transactions.form.warnAmount 词表；业务类校验保留
    /// 提交通道文案，ADR-0058）。
    AmountNotPositive,
    /// 金额换算失败（判定放行、换算拒绝的兑底位；正常流不可达）。
    AmountInvalid,
}

impl TransferFormError {
    /// 错误条位文案（zh-CN 词表）。
    pub fn as_text(self) -> &'static str {
        match self {
            Self::FromAccountRequired => "请选择转出账户",
            Self::ToAccountRequired => "请选择转入账户",
            Self::SameAccount => "转出账户和转入账户不能相同",
            Self::AmountNotPositive => "请输入金额",
            Self::AmountInvalid => "金额无效，请输入不超过两位小数的金额",
        }
    }
}

/// 转账 / 借贷表单状态 → 完整 TransactionInput（旧壳 buildTransferInput 同名
/// 接缝）：校验 fail fast（序镜像旧壳 submit：转出 → 转入 → 同账户 → 金额）
/// → 金额换算 → per-kind 矩阵落位。`currency_code` 由调用方随转出账户推导
/// 传入（记账币种 = 转出账户币种，ADR-0134 决策 5；两端一致的守卫归域
/// Writer，壳层候选面过滤是第一道防线）。
pub fn assemble_transfer_transaction(
    submit: TransferFormSubmit,
    currency_code: String,
) -> Result<ledger_transaction::TransactionInput, TransferFormError> {
    let Some(account_id) = submit.account_id.filter(|id| !id.trim().is_empty()) else {
        return Err(TransferFormError::FromAccountRequired);
    };
    let Some(to_account_id) = submit.to_account_id.filter(|id| !id.trim().is_empty()) else {
        return Err(TransferFormError::ToAccountRequired);
    };
    if account_id == to_account_id {
        return Err(TransferFormError::SameAccount);
    }
    let Some(amount_cents) = amount_text_to_cents(&submit.amount_text) else {
        return Err(TransferFormError::AmountInvalid);
    };
    if amount_cents <= 0 {
        return Err(TransferFormError::AmountNotPositive);
    }
    Ok(ledger_transaction::TransactionInput {
        kind: TransactionKind::Transfer,
        amount_cents,
        currency_code,
        account_id: Some(account_id),
        to_account_id: Some(to_account_id),
        funding_account_id: None,
        funding: Vec::new(),
        purchases: Vec::new(),
        category_id: None,
        merchant_id: match &submit.merchant {
            MerchantRef::None | MerchantRef::NewName(_) => None,
            MerchantRef::Keep(id) | MerchantRef::Selected(id) => Some(id.clone()),
        },
        merchant_name: match &submit.merchant {
            MerchantRef::NewName(name) => Some(name.clone()),
            _ => None,
        },
        policy_id: None,
        refund_of_transaction_id: None,
        note: (!submit.note.trim().is_empty()).then(|| submit.note.clone()),
        source_order_no: submit.source_order_no,
        date: submit.date.to_string(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        origin: None,
        fx_rate: None,
        idempotency_key: None,
    })
}

/// 退款表单提交状态（提交时点的业务字段；日期已是本地日历日）。原交易引用
/// 与金额钳制语义（继承原支出账户/币种/分类/商户）以核心交易域为唯一真源
/// （writer::normalize），装配层只消费表单状态、不另立第二份继承。
#[derive(Debug, Clone, PartialEq)]
pub struct RefundFormSubmit {
    /// 退款金额原始文本（缺省回填原交易金额——「金额钳制」的入口形态：初始
    /// 值随原交易，可改；域不做超额钳制，壳层不另立）。
    pub amount_text: String,
    /// 原支出交易引用（必选；fixed-target 入口随行固定，编辑面透传行上原值
    /// ——关联语义不可被编辑破坏）。
    pub refund_of_transaction_id: Option<String>,
    /// 原支出账户（表单随目标带出；域再继承，调用方传什么校验什么）。
    pub account_id: Option<String>,
    /// 备注（空串落 None）。
    pub note: String,
    /// 退款日期（本地日历日语义）。
    pub date: chrono::NaiveDate,
    /// 来源订单号（表单无输入位：创建 None，编辑透传行上原值）。
    pub source_order_no: Option<String>,
}

/// 退款表单校验错误（壳层呈现词表的载体）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefundFormError {
    /// 原支出交易未定（旧壳 transactions.refund.warnNoTarget 词表）。
    TargetRequired,
    /// 退款金额非正数（旧壳 transactions.refund.warnNoAmount 词表）。
    AmountNotPositive,
    /// 金额换算失败（判定放行、换算拒绝的兑底位；正常流不可达）。
    AmountInvalid,
}

impl RefundFormError {
    /// 错误条位文案（zh-CN 词表）。
    pub fn as_text(self) -> &'static str {
        match self {
            Self::TargetRequired => "请选择要退款的原始支出交易",
            Self::AmountNotPositive => "请输入退款金额",
            Self::AmountInvalid => "金额无效，请输入不超过两位小数的金额",
        }
    }
}

/// 退款表单状态 → 完整 TransactionInput（旧壳 buildRefundInput 同名接缝）：
/// 校验 fail fast（目标先于金额，旧壳 submit 守卫序同形）→ 金额换算 →
/// per-kind 矩阵落位（分类/商户/保单全部继承原支出，表单不携带——域
/// writer::normalize 忽略调用方填值；不挂保单是准入集拒绝，ADR-0051 决策 4）。
pub fn assemble_refund_transaction(
    submit: RefundFormSubmit,
    currency_code: String,
) -> Result<ledger_transaction::TransactionInput, RefundFormError> {
    let Some(refund_of_transaction_id) = submit
        .refund_of_transaction_id
        .filter(|id| !id.trim().is_empty())
    else {
        return Err(RefundFormError::TargetRequired);
    };
    let Some(amount_cents) = amount_text_to_cents(&submit.amount_text) else {
        return Err(RefundFormError::AmountInvalid);
    };
    if amount_cents <= 0 {
        return Err(RefundFormError::AmountNotPositive);
    }
    Ok(ledger_transaction::TransactionInput {
        kind: TransactionKind::Refund,
        amount_cents,
        currency_code,
        account_id: submit.account_id.filter(|id| !id.trim().is_empty()),
        to_account_id: None,
        funding_account_id: None,
        funding: Vec::new(),
        purchases: Vec::new(),
        category_id: None,
        merchant_id: None,
        merchant_name: None,
        policy_id: None,
        refund_of_transaction_id: Some(refund_of_transaction_id),
        note: (!submit.note.trim().is_empty()).then(|| submit.note.clone()),
        source_order_no: submit.source_order_no,
        date: submit.date.to_string(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        origin: None,
        fx_rate: None,
        idempotency_key: None,
    })
}

// ---------------------------------------------------------------------------
// 出资分解 / 购买项下单表单装配器（#2036 表单③；域契约唯一真源 =
// ledger_transaction::write::funding_items / purchase_items）：表单③两形态
// 的表单状态 → 完整 TransactionInput 的装配接缝。纪律同前三装配器：同步纯
// 函数、校验 fail fast 显式报错、金额换算与日期转换在此收口；分解行的主账
// 户恒缺省（契约互斥，ADR-0138 决策 2），订单行主账户必选；合计一致性
// （Σ == 交易金额）在本层先行阻拦，域 Writer 守卫是契约兑底。
// ---------------------------------------------------------------------------

/// 购买项行提交状态（表单③两族共用的商品行；件数/单价以原始文本承载，换
/// 算在装配器内收口）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseRowSubmit {
    /// 商品名称（必填，trim 后非空）。
    pub name: String,
    /// 件数原始文本（正整数）。
    pub quantity_text: String,
    /// 商品分类（可选，明细指针；不承载统计口径——报表按交易行分类整单计入，
    /// ADR-0138 决策 11）。
    pub category_id: Option<String>,
    /// 单价原始文本（可空；携带时非负，ADR-0138 决策 10：存而不显示、不进
    /// 金额口径）。
    pub price_text: String,
}

/// 购买项行校验错误（两族共用的行级词表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurchaseRowError {
    /// 商品名称缺失。
    NameRequired,
    /// 件数非正整数。
    QuantityInvalid,
    /// 单价形状/精度非法。
    PriceInvalid,
    /// 单价为负（0 是合法标价——赠品）。
    PriceNegative,
}

impl PurchaseRowError {
    /// 错误条位文案（zh-CN 词表）。
    pub fn as_text(self) -> &'static str {
        match self {
            Self::NameRequired => "请输入商品名称",
            Self::QuantityInvalid => "件数必须为正整数",
            Self::PriceInvalid => "商品单价无效，请输入不超过两位小数的金额",
            Self::PriceNegative => "商品单价不能为负数",
        }
    }
}

/// 购买项行装配（行状态 → 域购买项输入；件数与单价换算在此收口）。纯函数，
/// 出资分解族与购买项族两装配器共用同一行语义（分解行叠加购买项的编辑形态
/// 与订单族同一契约）。
fn assemble_purchase_row(
    row: PurchaseRowSubmit,
) -> Result<ledger_transaction::TransactionPurchaseInput, PurchaseRowError> {
    let name = row.name.trim().to_owned();
    if name.is_empty() {
        return Err(PurchaseRowError::NameRequired);
    }
    let Ok(quantity) = row.quantity_text.trim().parse::<i64>() else {
        return Err(PurchaseRowError::QuantityInvalid);
    };
    if quantity <= 0 {
        return Err(PurchaseRowError::QuantityInvalid);
    }
    let unit_price_cents = if row.price_text.trim().is_empty() {
        None
    } else {
        if !matches!(judge_amount_text(&row.price_text), AmountJudgment::Ok(_)) {
            return Err(PurchaseRowError::PriceInvalid);
        }
        let cents = amount_text_to_cents(&row.price_text).ok_or(PurchaseRowError::PriceInvalid)?;
        if cents < 0 {
            return Err(PurchaseRowError::PriceNegative);
        }
        Some(cents)
    };
    Ok(ledger_transaction::TransactionPurchaseInput {
        name,
        quantity,
        category_id: row.category_id.filter(|id| !id.trim().is_empty()),
        unit_price_cents,
    })
}

/// 出资项行提交状态（账户 / 金额 / 扣款标签；金额以原始文本承载）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundingRowSubmit {
    /// 出资账户（必选；允许同一账户多条——预售定金 + 尾款靠标签区分）。
    pub account_id: Option<String>,
    /// 出资金额原始文本（正数）。
    pub amount_text: String,
    /// 扣款标签（可选自由文本，上限 50 字——域同款契约，壳层先行同文案语义）。
    pub label: String,
}

/// 出资分解表单提交状态（提交时点的业务字段；分解行的主账户在域契约下恒
/// 缺省，表单无主账户位）。
#[derive(Debug, Clone, PartialEq)]
pub struct FundingFormSubmit {
    /// 交易类型（本族闭集：支出 / 收入——仅 expense/income 可带分解）。
    pub kind: TransactionKind,
    /// 交易金额（主行）原始文本。
    pub amount_text: String,
    /// 出资项子行（数组序即顺序位）。
    pub rows: Vec<FundingRowSubmit>,
    /// 叠加购买项（分解行可同时携带购买项——AI 导入多出资多商品订单的编辑
    /// 形态，ADR-0138 决策 15 呈现形态；创建入口默认空）。
    pub purchases: Vec<PurchaseRowSubmit>,
    /// 交易行分类（统计归属）。
    pub category_id: Option<String>,
    /// 商户语义（见 [`MerchantRef`]）。
    pub merchant: MerchantRef,
    /// 可选保单引用。
    pub policy_id: Option<String>,
    /// 备注（空串落 None）。
    pub note: String,
    /// 交易日期。
    pub date: chrono::NaiveDate,
    /// 来源订单号（编辑路径透传行上原值；创建路径 None）。
    pub source_order_no: Option<String>,
}

/// 出资分解表单校验错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FundingFormError {
    /// 主行金额非正数。
    AmountNotPositive,
    /// 主行金额换算失败。
    AmountInvalid,
    /// 分解为空（本族无主账户位，域契约 funding 空 ⇒ account_id 必填，不可
    /// 提交；纠错走改用普通支出表单）。
    RowsEmpty,
    /// 出资行账户未选。
    RowAccountRequired,
    /// 出资行金额非正。
    RowAmountNotPositive,
    /// 出资行金额换算失败。
    RowAmountInvalid,
    /// 扣款标签超长。
    LabelTooLong,
    /// 合计不一致（Σ 出资项 ≠ 交易金额；用户故事 2 明确阻拦不错记）。
    TotalMismatch,
    /// 叠加购买项行错误（行级词表见 [`PurchaseRowError`]）。
    Row(PurchaseRowError),
}

impl FundingFormError {
    /// 错误条位文案（zh-CN 词表）。
    pub fn as_text(self) -> &'static str {
        match self {
            Self::AmountNotPositive => "请输入金额",
            Self::AmountInvalid => "金额无效，请输入不超过两位小数的金额",
            Self::RowsEmpty => "请至少添加一条出资项",
            Self::RowAccountRequired => "请选择出资账户",
            Self::RowAmountNotPositive => "请输入出资金额",
            Self::RowAmountInvalid => "出资金额无效，请输入不超过两位小数的金额",
            Self::LabelTooLong => "扣款标签不能超过 50 字",
            Self::TotalMismatch => "出资合计与交易金额不一致",
            Self::Row(e) => e.as_text(),
        }
    }
}

/// 出资分解表单状态 → 完整 TransactionInput：主金额换算 → 逐行守卫（账户 /
/// 金额 / 标签）→ 合计一致性 → 购买项行 → 契约互斥落位（account_id 恒
/// None）。`currency_code` 由调用方随出资账户推导传入（首行账户币种，域准
/// 入要求逐条与交易币种一致，多账户跨币种不做）。
pub fn assemble_funding_transaction(
    submit: FundingFormSubmit,
    currency_code: String,
) -> Result<ledger_transaction::TransactionInput, FundingFormError> {
    // 主金额（形状换算先行，业务正性随后——与基础收支同序）。
    let Some(amount_cents) = amount_text_to_cents(&submit.amount_text) else {
        return Err(FundingFormError::AmountInvalid);
    };
    if amount_cents <= 0 {
        return Err(FundingFormError::AmountNotPositive);
    }
    if submit.rows.is_empty() {
        return Err(FundingFormError::RowsEmpty);
    }
    // 逐行守卫（fail fast：账户 → 金额换算 → 正性 → 标签长度）。
    let mut funding = Vec::with_capacity(submit.rows.len());
    let mut total_cents = 0i64;
    for row in submit.rows {
        let Some(account_id) = row.account_id.filter(|id| !id.trim().is_empty()) else {
            return Err(FundingFormError::RowAccountRequired);
        };
        let Some(row_cents) = amount_text_to_cents(&row.amount_text) else {
            return Err(FundingFormError::RowAmountInvalid);
        };
        if row_cents <= 0 {
            return Err(FundingFormError::RowAmountNotPositive);
        }
        if row.label.chars().count() > 50 {
            return Err(FundingFormError::LabelTooLong);
        }
        total_cents += row_cents;
        funding.push(ledger_transaction::TransactionFundingInput {
            account_id,
            amount_cents: row_cents,
            label: (!row.label.trim().is_empty()).then(|| row.label.trim().to_owned()),
        });
    }
    // 合计一致性（Σ == 交易金额；域同款守卫在 writer::normalize，本层先行
    // 阻拦给出可纠正呈现位）。
    if total_cents != amount_cents {
        return Err(FundingFormError::TotalMismatch);
    }
    // 叠加购买项行（行级词表共用）。
    let mut purchases = Vec::with_capacity(submit.purchases.len());
    for row in submit.purchases {
        purchases.push(assemble_purchase_row(row).map_err(FundingFormError::Row)?);
    }
    Ok(ledger_transaction::TransactionInput {
        kind: submit.kind,
        amount_cents,
        currency_code,
        // 契约互斥（ADR-0138 决策 2）：非空分解 ⇒ 主账户恒缺省。
        account_id: None,
        to_account_id: None,
        funding_account_id: None,
        funding,
        purchases,
        category_id: submit.category_id,
        merchant_id: match &submit.merchant {
            MerchantRef::None | MerchantRef::NewName(_) => None,
            MerchantRef::Keep(id) | MerchantRef::Selected(id) => Some(id.clone()),
        },
        merchant_name: match &submit.merchant {
            MerchantRef::NewName(name) => Some(name.clone()),
            _ => None,
        },
        policy_id: submit.policy_id,
        refund_of_transaction_id: None,
        note: (!submit.note.trim().is_empty()).then(|| submit.note.clone()),
        source_order_no: submit.source_order_no,
        date: submit.date.to_string(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        origin: None,
        fx_rate: None,
        idempotency_key: None,
    })
}

/// 购买项下单表单提交状态（订单形态：主账户必选 + 商品行；仅 expense）。
#[derive(Debug, Clone, PartialEq)]
pub struct PurchaseFormSubmit {
    /// 订单实付金额（主行）原始文本。
    pub amount_text: String,
    /// 付款账户（必选——订单是单账户支出，与分解族互斥形态）。
    pub account_id: Option<String>,
    /// 商品行（数组序即对账单顺序；空行集合法 = 无明细的普通支出）。
    pub rows: Vec<PurchaseRowSubmit>,
    /// 交易行分类（统计归属——报表按单归一类，ADR-0138 决策 11）。
    pub category_id: Option<String>,
    /// 商户语义（见 [`MerchantRef`]）。
    pub merchant: MerchantRef,
    /// 可选保单引用。
    pub policy_id: Option<String>,
    /// 备注（空串落 None）。
    pub note: String,
    /// 交易日期。
    pub date: chrono::NaiveDate,
    /// 来源订单号（手动录入自然为空——ADR-0138 决策 9 口径；编辑路径透传行
    /// 上原值）。
    pub source_order_no: Option<String>,
}

/// 购买项下单表单校验错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurchaseFormError {
    /// 账户未选。
    AccountRequired,
    /// 主行金额非正数。
    AmountNotPositive,
    /// 主行金额换算失败。
    AmountInvalid,
    /// 商品行错误（行级词表见 [`PurchaseRowError`]）。
    Row(PurchaseRowError),
}

impl PurchaseFormError {
    /// 错误条位文案（zh-CN 词表）。
    pub fn as_text(self) -> &'static str {
        match self {
            Self::AccountRequired => "请选择账户",
            Self::AmountNotPositive => "请输入金额",
            Self::AmountInvalid => "金额无效，请输入不超过两位小数的金额",
            Self::Row(e) => e.as_text(),
        }
    }
}

/// 购买项下单表单状态 → 完整 TransactionInput：账户 → 主金额 → 商品行，
/// kind 恒 expense（仅 expense 可带购买项，ADR-0138 决策 9）。`currency_code`
/// 由调用方随所选账户推导传入。
pub fn assemble_purchase_transaction(
    submit: PurchaseFormSubmit,
    currency_code: String,
) -> Result<ledger_transaction::TransactionInput, PurchaseFormError> {
    let Some(account_id) = submit.account_id.filter(|id| !id.trim().is_empty()) else {
        return Err(PurchaseFormError::AccountRequired);
    };
    let Some(amount_cents) = amount_text_to_cents(&submit.amount_text) else {
        return Err(PurchaseFormError::AmountInvalid);
    };
    if amount_cents <= 0 {
        return Err(PurchaseFormError::AmountNotPositive);
    }
    let mut purchases = Vec::with_capacity(submit.rows.len());
    for row in submit.rows {
        purchases.push(assemble_purchase_row(row).map_err(PurchaseFormError::Row)?);
    }
    Ok(ledger_transaction::TransactionInput {
        kind: TransactionKind::Expense,
        amount_cents,
        currency_code,
        account_id: Some(account_id),
        to_account_id: None,
        funding_account_id: None,
        funding: Vec::new(),
        purchases,
        category_id: submit.category_id,
        merchant_id: match &submit.merchant {
            MerchantRef::None | MerchantRef::NewName(_) => None,
            MerchantRef::Keep(id) | MerchantRef::Selected(id) => Some(id.clone()),
        },
        merchant_name: match &submit.merchant {
            MerchantRef::NewName(name) => Some(name.clone()),
            _ => None,
        },
        policy_id: submit.policy_id,
        refund_of_transaction_id: None,
        note: (!submit.note.trim().is_empty()).then(|| submit.note.clone()),
        source_order_no: submit.source_order_no,
        date: submit.date.to_string(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        origin: None,
        fx_rate: None,
        idempotency_key: None,
    })
}

/// 预算执行行的展示格（渲染期零格式化：文本已是一次性装配产物；金额分与
/// 周期原值随行携带，供编辑弹窗回填——行为锚点，不参与渲染）。
#[derive(Debug, Clone, PartialEq)]
pub struct BudgetRowCells {
    /// 预算 id（编辑/删除的目标）。
    pub id: String,
    /// 分类显示名（「父 > 子」路径名或「未分类」回退）。
    pub category: String,
    /// 周期标签（按月/按年）。
    pub period_label: &'static str,
    /// 预算金额文本。
    pub amount: String,
    /// 已支出文本。
    pub spent: String,
    /// 进度百分比（0–100，封顶）。
    pub percent: f32,
    /// 是否超支（状态色的消费面）。
    pub over_budget: bool,
    /// 状态标签（超支/正常）。
    pub status: &'static str,
    /// 预算金额原值（编辑回填锚点，整数分）。
    pub amount_cents: i64,
    /// 周期原值（编辑回填锚点）。
    pub period: ledger_budget::BudgetPeriod,
}

/// 预算进度行集 → 展示格行集（一次性装配，渲染期零格式化）。
pub fn assemble_budget_rows(
    rows: &[ledger_budget::BudgetProgress],
    categories: &CategoryTable,
    privacy: bool,
) -> Vec<BudgetRowCells> {
    rows.iter()
        .map(|row| BudgetRowCells {
            id: row.budget.id.clone(),
            category: budget_category_display(categories, &row.budget.category_id),
            period_label: budget_period_label(row.budget.period),
            amount: budget_amount_text(row.budget.amount_cents, privacy),
            spent: budget_amount_text(row.spent_cents, privacy),
            percent: budget_progress_percent(row.spent_cents, row.budget.amount_cents),
            over_budget: row.over_budget,
            status: budget_status_text(row.over_budget),
            amount_cents: row.budget.amount_cents,
            period: row.budget.period,
        })
        .collect()
}

/// 年度汇总卡的展示格（渲染期零格式化）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BudgetSummaryCells {
    /// 年度预算总额文本。
    pub annual_total: String,
    /// 本年已支文本。
    pub spent: String,
    /// 进度百分比（0–100，封顶）。
    pub percent: f32,
    /// 是否超标（进度条状态色的消费面）。
    pub over_budget: bool,
    /// 状态标签（已超标/正常）。
    pub status: &'static str,
}

/// 年度汇总 → 展示格（一次性装配，渲染期零格式化）。
pub fn assemble_budget_summary(
    summary: &ledger_budget::BudgetAnnualSummary,
    privacy: bool,
) -> BudgetSummaryCells {
    BudgetSummaryCells {
        annual_total: budget_amount_text(summary.annual_budget_total_cents, privacy),
        spent: budget_amount_text(summary.spent_cents, privacy),
        percent: budget_summary_percent(summary.spent_cents, summary.annual_budget_total_cents),
        over_budget: summary.over_budget,
        status: budget_summary_status_text(summary.over_budget),
    }
}

// ---------------------------------------------------------------------------
// 概览视图四卡格（#2039）：净资产 / 本月收支 / 投资三卡 / 财务自由度——旧壳
// DashboardView 装配层的重生（一次装配、渲染期零格式化）。折算与多币种合计
// 以域为唯一真源（域行集已带折本位币单值），本层只消费域行集产展示文本，
// 不另立第二份口径表达式。
// ---------------------------------------------------------------------------

/// 盈亏涨跌色消费面（旧壳 pnlSemanticColor 同语义：红涨绿跌，与交易类型
/// 语义色正交——两族色值当前同相属巧合，不互相引用；色值归视图层映射）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PnlSemantic {
    /// 涨（正收益含零）。
    Gain,
    /// 跌（负收益）。
    Loss,
}

/// 财务自由度阶段（ADR-0048 决策 5：阈值与文案归 UI 层，不做后端枚举）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreedomStage {
    /// <30% 积累期。
    Accumulation,
    /// 30–100% 接近自由。
    Approaching,
    /// ≥100% 财务自由。
    Free,
}

impl FreedomStage {
    /// 阶段标签（旧壳 dashboard.freedom.stage* 词表）。
    pub fn label(self) -> &'static str {
        match self {
            FreedomStage::Accumulation => "积累期",
            FreedomStage::Approaching => "接近自由",
            FreedomStage::Free => "财务自由",
        }
    }
}

/// JS 数字字面文本（一位小数刻度值的展示形态）：整数刻度不拖尾零（40 →
/// "40"），非整数一位小数（37.5 → "37.5"）——旧壳模板插值 `{{ ratio }}%`
/// 的 JS 数字字符串化同形态。
fn js_number_text(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value:.1}")
    }
}

/// 净资产卡格（置顶主数字：本位币单一主数字 + 币符）。
#[derive(Debug, Clone, PartialEq)]
pub struct NetWorthCells {
    /// 净资产主数字（本位币，隐私态掩码）。
    pub net_worth: String,
}

/// 净资产总览 → 展示格：后端 `dashboard_overview` 单点折算的单一主数字
///（旧壳只呈现 net_worth_cents 一数，组成腿在口径说明弹层交代）。
pub fn assemble_net_worth(
    overview: &ledger_dashboard::DashboardOverview,
    currencies: &CurrencyTable,
    privacy: bool,
) -> NetWorthCells {
    let net_worth = if privacy {
        AMOUNT_PRIVACY_MASK.to_owned()
    } else {
        format_amount_basic(
            overview.net_worth_cents,
            &currencies.format_of(&overview.native_currency),
        )
    };
    NetWorthCells { net_worth }
}

/// 本月收支卡格（收入 / 净支出 / 结余，默认币种单币种无符号形态）。
#[derive(Debug, Clone, PartialEq)]
pub struct MonthlyFlowCells {
    /// 收入（后端净收入列 income_net 直出）。
    pub income: String,
    /// 净支出 = 毛支出 − 退款（退款不单列，与预算消耗口径一致）。
    pub net_expense: String,
    /// 结余 = 收入 − 净支出。
    pub balance: String,
}

/// 月度汇总当月行 → 展示格：行缺失（当月无交易）三格归零（旧壳 `?? 0`
/// 同口径）。
pub fn assemble_monthly_flow(
    month: Option<&ledger_reports::MonthlySummary>,
    privacy: bool,
) -> MonthlyFlowCells {
    let (income, net_expense) = match month {
        Some(row) => (row.income_cents, row.expense_cents - row.refund_cents),
        None => (0, 0),
    };
    let balance = income - net_expense;
    MonthlyFlowCells {
        income: budget_amount_text(income, privacy),
        net_expense: budget_amount_text(net_expense, privacy),
        balance: budget_amount_text(balance, privacy),
    }
}

/// 投资概览单卡格（三卡各一份）：缺料三态显式分离（旧壳 NativeTotal 同形）。
#[derive(Debug, Clone, PartialEq)]
pub struct PortfolioStatCells {
    /// 折本位币单值文本（`None` = 无可计入行或缺折算汇率 → 呈现「-」）；
    /// 隐私态掩码。
    pub text: Option<String>,
    /// 整卡警告文案（缺折算汇率行在场或命令报错）：置位即警告态，不给半截数字。
    pub warning: Option<String>,
    /// 缺现价未计入计数小字（警告态不重复交代，旧壳同规则）。
    pub missing_price_note: Option<String>,
    /// 盈亏涨跌色消费面（`None` = 中性：市值卡恒中性，无可计入值不者色）。
    pub pnl: Option<PnlSemantic>,
}

/// 投资概览三卡格 + 空态消费面。
#[derive(Debug, Clone, PartialEq)]
pub struct PortfolioStatsCells {
    /// 总市值（中性）。
    pub market_value: PortfolioStatCells,
    /// 持仓收益（未实现盈亏，涨跌色）。
    pub unrealized_pnl: PortfolioStatCells,
    /// 累计收益（涨跌色；全账本口径，命令透传单值）。
    pub cumulative_pnl: PortfolioStatCells,
    /// 有无持仓行（空态「暂无持仓」的消费面，三卡不呈现）。
    pub has_holdings: bool,
}

/// 缺折算汇率整卡警告文案（旧壳 investments.statsCards.rateMissing 同词表）。
const RATE_MISSING_WARNING: &str = "部分持仓缺折算汇率，合计暂不可计。";

/// 持仓行集单列（市值或未实现盈亏）→ 缺料三态合计（旧壳 nativeRowTotal 同
/// 语义：缺现价行计数不计入、缺折算汇率行整卡警告、其余行同币求和；行级
/// 折本位币已由域读路径完成，本层不另立折算）。
fn portfolio_column_total(rows: &[(Option<i64>, Option<i64>)]) -> (Option<i64>, usize, usize) {
    let mut cents = None;
    let mut missing_price_count = 0usize;
    let mut rate_missing_count = 0usize;
    for (raw, native) in rows {
        match (raw, native) {
            (None, _) => missing_price_count += 1,
            (Some(_), None) => rate_missing_count += 1,
            (Some(_), Some(value)) => cents = Some(cents.unwrap_or(0) + value),
        }
    }
    // 缺折算汇率行在场即不给半截数字。
    if rate_missing_count > 0 {
        cents = None;
    }
    (cents, missing_price_count, rate_missing_count)
}

/// 投资概览卡装配：持仓行集（域逐行软折算产物）与累计收益命令透传 → 三卡格。
/// `cumulative_error` 是累计收益命令的码化错误文案（缺汇率上抛 → 累计卡警告
/// 与重试消费面）。`native_currency` 为累计命令透传的折算基准币种，`None` 时
/// 取缺省格式（旧壳 reference.getCurrency("") 降级裸数字同口径）。
pub fn assemble_portfolio_stats(
    holdings: &[ledger_investment::Holding],
    cumulative: Option<&ledger_investment::CumulativePnlNativeTotal>,
    cumulative_error: Option<String>,
    currencies: &CurrencyTable,
    privacy: bool,
) -> PortfolioStatsCells {
    let format_of = |code: Option<&str>| code.map(|c| currencies.format_of(c)).unwrap_or_default();
    // 传入币种优先取累计命令的基准币种（旧壳 nativeCurrency 透传同源）；命令
    // 未就绪时缺省格式。
    let native_code = cumulative.map(|c| c.native_currency.as_str());
    let fmt = format_of(native_code);
    let stat = |rows: &[(Option<i64>, Option<i64>)], pnl: bool| {
        let (cents, missing_price_count, rate_missing_count) = portfolio_column_total(rows);
        let warning = (rate_missing_count > 0).then_some(RATE_MISSING_WARNING.to_owned());
        let text = if privacy {
            cents.is_some().then(|| AMOUNT_PRIVACY_MASK.to_owned())
        } else {
            cents.map(|c| format_amount_basic(c, &fmt))
        };
        let missing_price_note = (warning.is_none() && missing_price_count > 0)
            .then(|| format!("{missing_price_count} 只持仓因缺现价未计入。"));
        let pnl = (pnl && cents.is_some()).then(|| {
            if cents.unwrap_or(0) >= 0 {
                PnlSemantic::Gain
            } else {
                PnlSemantic::Loss
            }
        });
        PortfolioStatCells {
            text,
            warning,
            missing_price_note,
            pnl,
        }
    };
    let market_rows: Vec<(Option<i64>, Option<i64>)> = holdings
        .iter()
        .map(|h| (h.market_value_cents, h.native_market_value_cents))
        .collect();
    let unreal_rows: Vec<(Option<i64>, Option<i64>)> = holdings
        .iter()
        .map(|h| (h.unrealized_pnl_cents, h.native_unrealized_pnl_cents))
        .collect();
    let cumulative_cells = match (cumulative_error, cumulative) {
        // 命令报错优先：整卡警告 + 文案透传，不给半截数字（旧壳 error 优先同序）。
        (Some(error), _) => PortfolioStatCells {
            text: None,
            warning: Some(error),
            missing_price_note: None,
            pnl: None,
        },
        (None, Some(total)) => {
            let text = if privacy {
                Some(AMOUNT_PRIVACY_MASK.to_owned())
            } else {
                Some(format_amount_basic(total.total_cents, &fmt))
            };
            PortfolioStatCells {
                text,
                warning: None,
                missing_price_note: None,
                pnl: Some(if total.total_cents >= 0 {
                    PnlSemantic::Gain
                } else {
                    PnlSemantic::Loss
                }),
            }
        }
        // 命令未就绪（装载中）：空值待装，不提前报错。
        (None, None) => PortfolioStatCells {
            text: None,
            warning: None,
            missing_price_note: None,
            pnl: None,
        },
    };
    PortfolioStatsCells {
        market_value: stat(&market_rows, false),
        unrealized_pnl: stat(&unreal_rows, true),
        cumulative_pnl: cumulative_cells,
        has_holdings: !holdings.is_empty(),
    }
}

/// 财务自由度卡格（比例 / 阶段 / 进度 / 口径两行 / 覆盖年数 / 零分母占位态）。
#[derive(Debug, Clone, PartialEq)]
pub struct FreedomCells {
    /// 比例文本（一位小数去尾零，如 "37.5%" / "100%"；零分母占位态为 "0%"）。
    pub ratio_text: String,
    /// 比例数值（进度条消费面，封顶在视图层）。
    pub percent: f32,
    /// 阶段（零分母占位态 `None`——不回退实际支出，旧壳同规则）。
    pub stage: Option<FreedomStage>,
    /// 「可投资资产 {amount}」口径行。
    pub numerator_line: String,
    /// 「年度预算总额 {amount}」口径行。
    pub denominator_line: String,
    /// 「可覆盖 {years} 年」行。
    pub coverage_line: String,
    /// 零分母（未设预算）：占位引导态（去预算页设置），口径不回退实际支出。
    pub zero_denominator: bool,
}

/// 财务自由度总览 → 展示格（域聚合唯一真源；阶段阈值 ADR-0048 决策 5 归 UI）。
pub fn assemble_financial_freedom(
    freedom: &ledger_investment::FinancialFreedomOverview,
    currencies: &CurrencyTable,
    privacy: bool,
) -> FreedomCells {
    let fmt = currencies.format_of(&freedom.native_currency);
    let zero_denominator = freedom.denominator_cents == 0;
    let stage = if zero_denominator {
        None
    } else if freedom.ratio >= 100.0 {
        Some(FreedomStage::Free)
    } else if freedom.ratio >= 30.0 {
        Some(FreedomStage::Approaching)
    } else {
        Some(FreedomStage::Accumulation)
    };
    FreedomCells {
        ratio_text: format!("{}%", js_number_text(freedom.ratio)),
        percent: freedom.ratio as f32,
        stage,
        numerator_line: format!(
            "可投资资产 {}",
            if privacy {
                AMOUNT_PRIVACY_MASK.to_owned()
            } else {
                format_amount_basic(freedom.numerator_cents, &fmt)
            }
        ),
        denominator_line: format!(
            "年度预算总额 {}",
            if privacy {
                AMOUNT_PRIVACY_MASK.to_owned()
            } else {
                format_amount_basic(freedom.denominator_cents, &fmt)
            }
        ),
        coverage_line: format!("可覆盖 {} 年", js_number_text(freedom.coverage_years)),
        zero_denominator,
    }
}

/// 预算执行表列定义（键与列序同旧壳桌面档七列；进度列弹性吸收富余宽，UI 层
/// 映射为宿主列）。
pub const BUDGET_COLUMNS: &[ColumnDef] = &[
    ColumnDef {
        key: "category",
        title: "分类",
        align: ColumnAlign::Start,
        width: 180.0,
    },
    ColumnDef {
        key: "period",
        title: "周期",
        align: ColumnAlign::Start,
        width: 80.0,
    },
    ColumnDef {
        key: "amount",
        title: "预算",
        align: ColumnAlign::End,
        width: 120.0,
    },
    ColumnDef {
        key: "spent",
        title: "已支出",
        align: ColumnAlign::End,
        width: 120.0,
    },
    ColumnDef {
        key: "progress",
        title: "进度",
        align: ColumnAlign::Start,
        width: 200.0,
    },
    ColumnDef {
        key: "status",
        title: "状态",
        align: ColumnAlign::Start,
        width: 80.0,
    },
    ColumnDef {
        key: "actions",
        title: "操作",
        align: ColumnAlign::Start,
        width: 130.0,
    },
];

// ---------------------------------------------------------------------------
// 账户视图装配族（#2041；旧壳 AccountsView 与 src/accounts/credit-card.ts、
// src/scheduled/plan-usage.ts 纯函数的重生）：余额行装配、信用卡档案派生、
// 三分区派生、已归档分区合计与计划引用计数。口径对拍旧壳。
// ---------------------------------------------------------------------------

/// 使用率警示阈值（%，spec #1327 / ADR-0119 判据）：达到或超过它时列表小字
/// 用警示色。阈值只此一处，不做设置项（旧壳同值）。
pub const CREDIT_UTILIZATION_WARNING_PERCENT: i64 = 90;

/// 信用卡档案派生视图（账户列表使用率小字与编辑弹窗只读摘要共用的唯一形态，
/// 旧壳 `creditProfile` 重生）。派生量不落库、不参与余额口径：已用额度由余额
/// 派生（`max(0, −余额)`），与 `account_flow` 符号矩阵同源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditProfile {
    /// 信用额度（未设置 = None）。
    pub limit_cents: Option<i64>,
    /// 已用额度 = `max(0, −余额)`（余额为正即溢缴款，此时已用为 0）。
    pub used_cents: i64,
    /// 可用额度 = 额度 + 溢缴款 = **额度 + 余额**（`used + account_flow` 符号
    /// 矩阵化简）；额度未设置时 `None`；超额使用时为负值（如实呈现不钳 0，
    /// 与使用率 >100% 同口径）。
    pub available_cents: Option<i64>,
    /// 使用率 %（四舍五入取整）；额度未设置时 `None`。
    pub utilization_percent: Option<i64>,
    /// 下次账单日（ISO 短格式）；账单日未设置时 `None`。
    pub next_statement_date: Option<String>,
    /// 下次还款日（ISO 短格式）；还款日未设置时 `None`。
    pub next_due_date: Option<String>,
}

/// 「下次某日」（旧壳 `nextOccurrenceOfDay` 重生）：从 `today` 起最近的该
/// 「每月第 N 日」——当月没有该日取当月最后一天（31 日 → 2 月取 28/29，与
/// 后端调度器月末钳制同口径；展示层钳制不落库）；今天就是该日取今天不跳下月；
/// 日越界（1–31 外的脏值）不猜返回 `None`。
pub fn next_occurrence_of_day(day: i64, today: chrono::NaiveDate) -> Option<String> {
    if !(1..=31).contains(&day) {
        return None;
    }
    let month_last = |y: i32, m: u32| {
        let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
        chrono::NaiveDate::from_ymd_opt(ny, nm, 1)
            .and_then(|first| first.pred_opt())
            .map(|last| i64::from(last.day()))
    };
    let clamped = day.min(month_last(today.year(), today.month())?);
    let (this_year, this_month) = (today.year(), today.month());
    if i64::from(today.day()) <= clamped {
        return chrono::NaiveDate::from_ymd_opt(this_year, this_month, clamped as u32)
            .map(|d| d.to_string());
    }
    let (next_year, next_month) = if today.month() == 12 {
        (today.year() + 1, 1)
    } else {
        (today.year(), today.month() + 1)
    };
    let next_clamped = day.min(month_last(next_year, next_month)?);
    chrono::NaiveDate::from_ymd_opt(next_year, next_month, next_clamped as u32)
        .map(|d| d.to_string())
}

/// 信用卡派生（唯一入口，旧壳 `creditProfile` 同判据，旧壳吃账户行、本函数
/// 吃散字段——域行无 `Clone` 语义，壳侧行快照携带档案三键与余额）：非信用卡
/// 账户返回 `None`——调用方据此分支，不散落 `kind == credit` 判断。
pub fn credit_profile_fields(
    kind: AccountType,
    credit_limit_cents: Option<i64>,
    statement_day: Option<i64>,
    due_day: Option<i64>,
    balance_cents: i64,
    today: chrono::NaiveDate,
) -> Option<CreditProfile> {
    if kind != AccountType::Credit {
        return None;
    }
    let used_cents = (-balance_cents).max(0);
    Some(CreditProfile {
        available_cents: credit_limit_cents.map(|limit| limit + balance_cents),
        utilization_percent: credit_limit_cents
            .filter(|limit| *limit > 0)
            .map(|limit| (used_cents as f64 / limit as f64 * 100.0).round() as i64),
        next_statement_date: statement_day.and_then(|d| next_occurrence_of_day(d, today)),
        next_due_date: due_day.and_then(|d| next_occurrence_of_day(d, today)),
        limit_cents: credit_limit_cents,
        used_cents,
    })
}

/// 「今天」（本地时区日历日）：账户使用率小字的「下次账单节点」派生的时间
/// 基点（与旧壳 `new Date()` 本地日历日语义同形；派生随自然时间滚动，快照
/// 进装配行不持久化）。
fn today_date() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

/// 账户余额清单列（旧壳 accounts.list.col* 同词表；余额列末端对齐）：名称 /
/// 类型 / 币种 / 余额 / 操作。建议宽度供 UI 映射，富余宽度全给名称列（旧壳
/// 名称列弹性，交易列表备注列先例同形）。
pub const ACCOUNT_COLUMNS: &[ColumnDef] = &[
    ColumnDef {
        key: "name",
        title: "名称",
        align: ColumnAlign::Start,
        width: 180.0,
    },
    ColumnDef {
        key: "type",
        title: "类型",
        align: ColumnAlign::Start,
        width: 100.0,
    },
    ColumnDef {
        key: "currency",
        title: "币种",
        align: ColumnAlign::Start,
        width: 90.0,
    },
    ColumnDef {
        key: "balance",
        title: "余额",
        align: ColumnAlign::End,
        width: 130.0,
    },
    ColumnDef {
        key: "actions",
        title: "操作",
        align: ColumnAlign::Start,
        width: 70.0,
    },
];

/// 账户类型选项闭集（创建表单类型下拉；旧壳 ACCOUNT_TYPES 展示序同序、
/// accounts.type.* 同词表）。
pub fn account_type_options() -> Vec<CategoryOption> {
    [
        (AccountType::Cash, "现金"),
        (AccountType::Bank, "银行卡"),
        (AccountType::Credit, "信用卡"),
        (AccountType::Ewallet, "电子钱包"),
        (AccountType::Investment, "投资账户"),
        (AccountType::Debt, "负债"),
        (AccountType::Receivable, "借出款"),
        (AccountType::Other, "其他"),
    ]
    .into_iter()
    .map(|(kind, label)| CategoryOption {
        id: kind.to_string(),
        label: label.to_owned(),
    })
    .collect()
}

/// 账户类型标签（类型列与编辑弹窗只读类型位的普通类型渲染面；目标绑定账户
/// 由装配覆写为「攒钱」，本函数只答普通类型）。
pub fn account_type_label(kind: AccountType) -> &'static str {
    match kind {
        AccountType::Cash => "现金",
        AccountType::Bank => "银行卡",
        AccountType::Credit => "信用卡",
        AccountType::Ewallet => "电子钱包",
        AccountType::Investment => "投资账户",
        AccountType::Debt => "负债",
        AccountType::Receivable => "借出款",
        AccountType::Other => "其他",
    }
}

/// 账户清单行装配产物：每列最终展示文本（字段与 [`ACCOUNT_COLUMNS`] 逐列
/// 对应；操作列恒空串，格内容由 UI 侧「⋯」触发件自绘）+ 行级附加形态
/// （分区归属 / 目标绑定 / 信用卡使用率小字）。`id` 供行级操作定位（右键
/// 菜单与「⋯」按钮的同位目标）。
#[derive(Debug, Clone, PartialEq)]
pub struct AccountRowCells {
    /// 账户 id（操作目标）。
    pub id: String,
    /// 名称列（账户原名；目标绑定账户名称随目标联动，账户侧只读不改写——
    /// 此处只呈现读到的名字）。
    pub name: String,
    /// 类型列（目标绑定账户覆写为「攒钱」，旧壳 accountTypeLabel 同口径）。
    pub type_label: String,
    /// 币种列（ISO 原样）。
    pub currency_code: String,
    /// 余额列（formatAmount 含隐私掩码与数字分组；V017 缓存读数，本层只
    /// 消费不另立算术）。
    pub balance: String,
    /// 信用卡使用率小字百分比（仅已用 > 0 且额度已设置；None = 不呈现）。
    pub credit_percent: Option<i64>,
    /// 使用率 ≥ 警示阈值的小字警示色判定（随 credit_percent 在场才有义）。
    pub credit_warning: bool,
    /// 已归档（分区归属与「已归档」分区呈现的消费面）。
    pub archived: bool,
    /// 目标绑定（分区三分派与编辑弹窗「名称随动只读」判定的同源标注）。
    pub goal_bound: bool,
}

/// 账户行装配上下文：币种格式表、目标绑定集、隐私态（快照入参，纯函数）。
#[derive(Debug, Clone)]
pub struct AccountListContext<'a> {
    pub currencies: &'a CurrencyTable,
    pub goal_account_ids: &'a BTreeSet<String>,
    pub privacy_enabled: bool,
}

/// 账户余额行装配入参 face（#2041）：域行 `ledger_accounts::AccountBalance`
/// 无 `Clone` 语义，壳侧以可克隆行快照持有后按本 face 入参装配（字段全集
/// 与域行一致，纯透传；Lifetime 以借用字段面拖尾——装配期零拷贝）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountRowFace<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub kind: AccountType,
    pub currency_code: &'a str,
    pub archived: bool,
    pub balance_cents: i64,
    pub credit_limit_cents: Option<i64>,
    pub statement_day: Option<i64>,
    pub due_day: Option<i64>,
}

/// 装配账户余额行集（顺序保持域清单序 = created_at 升序，旧壳同口径；三分区
/// 是同一快照的纯派生不做任何加减、重排——余额与合计读数不受分组影响，
/// ADR-0133 决策 2 / ADR-0142 决策 1）。
pub fn assemble_account_rows<'a>(
    rows: &[AccountRowFace<'a>],
    ctx: &AccountListContext<'_>,
) -> Vec<AccountRowCells> {
    rows.iter()
        .map(|row| {
            let goal_bound = ctx.goal_account_ids.contains(row.id);
            let fmt = ctx.currencies.format_of(row.currency_code);
            let (credit_percent, credit_warning) = match credit_profile_fields(
                row.kind,
                row.credit_limit_cents,
                row.statement_day,
                row.due_day,
                row.balance_cents,
                today_date(),
            )
            .filter(|profile| profile.used_cents > 0)
            .and_then(|profile| profile.utilization_percent)
            {
                Some(percent) => (Some(percent), percent >= CREDIT_UTILIZATION_WARNING_PERCENT),
                None => (None, false),
            };
            AccountRowCells {
                id: row.id.to_owned(),
                name: row.name.to_owned(),
                type_label: if goal_bound {
                    "攒钱".to_owned()
                } else {
                    account_type_label(row.kind).to_owned()
                },
                currency_code: row.currency_code.to_owned(),
                balance: if ctx.privacy_enabled {
                    AMOUNT_PRIVACY_MASK.to_owned()
                } else {
                    format_amount_basic(row.balance_cents, &fmt)
                },
                credit_percent,
                credit_warning,
                archived: row.archived,
                goal_bound,
            }
        })
        .collect()
}

/// 三分区（旧壳 goalBalances / normalBalances / archivedBalances 同口径纯
/// 派生）：目标绑定（未归档）→「攒钱」组；未绑定且未归档 → 主表；已归档 →
/// 页底已归档分区。同一快照派生，零加减零重排（组内序保持域清单序）。
pub fn partition_account_rows(
    rows: &[AccountRowCells],
) -> (
    Vec<AccountRowCells>,
    Vec<AccountRowCells>,
    Vec<AccountRowCells>,
) {
    let mut goals = Vec::new();
    let mut normal = Vec::new();
    let mut archived = Vec::new();
    for row in rows {
        if row.archived {
            archived.push(row.clone());
        } else if row.goal_bound {
            goals.push(row.clone());
        } else {
            normal.push(row.clone());
        }
    }
    (goals, normal, archived)
}

/// 已归档分区合计（旧壳 archivedSummary 同口径纯派生）：**按币种各算一份**
/// 跨币种不相加（前端无汇率折算接缝，净资产折算归后端 dashboard_overview，
/// ADR-0067）。计数与余额累加回到「已归档分行 × 余额快照」同一份快照
/// （count 按币种分行数、total 按同币种已归档账户余额原值相加，同一快照直接
/// 累加，与主表各自派生、互不重算，ADR-0142 决策 1）。金额经与主表同一金额
/// 格式化口径（含隐私掩码）；单币种时只余一组，读法仍是一句。
pub fn assemble_archived_summary<'a>(
    archived: &[AccountRowCells],
    rows: &[AccountRowFace<'a>],
    currencies: &CurrencyTable,
    privacy_enabled: bool,
) -> String {
    // 分组：count 按已归档分行、total 按同一份余额快照累加（Map 保快照行
    // 首现顺序——域清单序 = created_at 升序，与旧壳固化首现序同形）。
    let mut order: Vec<String> = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for row in archived {
        // entry 一次并行两件事（首现登记 + 累加）：counts 之前只读不写，
        // get_mut 的「已登记」断言在首行必然落空——修复点（#2041 测试钉住）。
        let registered = !counts.contains_key(&row.currency_code);
        let count = counts.entry(row.currency_code.clone()).or_insert(0);
        *count += 1;
        if registered {
            order.push(row.currency_code.clone());
        }
    }
    let mut totals: BTreeMap<String, i64> = BTreeMap::new();
    for face in rows {
        if face.archived {
            *totals.entry(face.currency_code.to_owned()).or_insert(0) += face.balance_cents;
        }
    }
    let parts: Vec<String> = order
        .iter()
        .filter_map(|code| {
            let count = *counts.get(code)?;
            let total = totals.get(code).copied().unwrap_or(0);
            let amount = if privacy_enabled {
                AMOUNT_PRIVACY_MASK.to_owned()
            } else {
                format_amount_basic(total, &currencies.format_of(code))
            };
            Some(format!("{count} 个账户 · 合计 {amount}"))
        })
        .collect();
    parts.join("；")
}

/// 计划行快照的引用 face（#2041）：域行 `ScheduledTransactionWithExt` 无
/// `Clone` 语义，壳侧以可克隆快照持有后按本 face 入参——只携带引用计数的判
/// 据字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanLeadFace {
    pub is_deleted: bool,
    pub status: String,
    pub account_id: String,
    pub to_account_id: Option<String>,
}

/// 引用该账户的未结束计划数（旧壳 `countUnfinishedPlansUsingAccount` 重生）：
/// `active` / `paused` 计入（仍持有该账户）；`cancelled` / `completed` 不计；
/// 软删计划不计（读命令已滤但谓词独立自足，判据不依赖调用方行集形状）。
/// 付款 / 转出（`account_id`）∪ 转入（`to_account_id`，仅定时转账恒有义）
/// 两端取或，同一计划两端都指向该账户只计一次（谓词内的或，不是两次计数）。
pub fn count_unfinished_plans(plans: &[PlanLeadFace], account_id: &str) -> usize {
    plans
        .iter()
        .filter(|plan| {
            !plan.is_deleted
                && matches!(plan.status.as_str(), "active" | "paused")
                && (plan.account_id == account_id
                    || plan.to_account_id.as_deref() == Some(account_id))
        })
        .count()
}

// ---------------------------------------------------------------------------
// 储蓄目标视图格（#2056）：蓄水进度行装配——旧壳 SavingsGoalsView 装配面的
// 重生（一次装配、渲染期零格式化）。进度读数（已存 / 还差 / 达成态）与双向推
// 算（ETA / 所需月存 / 落后超前）由域读路径 [`ledger_savings_goal::list_savings
// _goal_progress`] 单点产出，本层只消费域行集产展示文本，不另立算术（词汇
// 表「蓄水进度」：只输出金额差值、无百分比口径）。
// ---------------------------------------------------------------------------

/// 储蓄目标列定义（列名与旧壳 savingsGoals.columns zh-CN 词表同文案；金额列
/// 末端对齐）。
pub const SAVINGS_GOAL_COLUMNS: &[ColumnDef] = &[
    ColumnDef {
        key: "name",
        title: "名称",
        align: ColumnAlign::Start,
        width: 140.0,
    },
    ColumnDef {
        key: "target",
        title: "目标金额",
        align: ColumnAlign::End,
        width: 105.0,
    },
    ColumnDef {
        key: "saved",
        title: "已存",
        align: ColumnAlign::End,
        width: 105.0,
    },
    ColumnDef {
        key: "remaining",
        title: "还差",
        align: ColumnAlign::End,
        width: 105.0,
    },
    ColumnDef {
        key: "projection",
        title: "推算",
        align: ColumnAlign::Start,
        width: 230.0,
    },
    ColumnDef {
        key: "deadline",
        title: "截止日",
        align: ColumnAlign::Start,
        width: 100.0,
    },
    ColumnDef {
        key: "status",
        title: "状态",
        align: ColumnAlign::Start,
        width: 110.0,
    },
    ColumnDef {
        key: "actions",
        title: "操作",
        align: ColumnAlign::Start,
        width: 130.0,
    },
];

/// 储蓄目标已归档列定义（旧壳 archivedColumns 同闭集：名称 / 目标金额 / 已存 /
/// 操作）。
pub const SAVINGS_GOAL_ARCHIVED_COLUMNS: &[ColumnDef] = &[
    ColumnDef {
        key: "name",
        title: "名称",
        align: ColumnAlign::Start,
        width: 140.0,
    },
    ColumnDef {
        key: "target",
        title: "目标金额",
        align: ColumnAlign::End,
        width: 105.0,
    },
    ColumnDef {
        key: "saved",
        title: "已存",
        align: ColumnAlign::End,
        width: 105.0,
    },
    ColumnDef {
        key: "actions",
        title: "操作",
        align: ColumnAlign::Start,
        width: 130.0,
    },
];

/// 一行蓄水进度的装配产物（字段与 [`SAVINGS_GOAL_COLUMNS`] 逐列对应）：
/// 金额与推算在到达时一次性成文本，渲染期零格式化；行上的原值字段
///（目标金额分 / 手填计划月存 / 截止目 / 币种）供编辑回填与删除点名消费
///（域行无 Clone 语义外的另一路面板——快照内单点反查）。
#[derive(Debug, Clone, PartialEq)]
pub struct SavingsGoalRowCells {
    /// 目标 id（操作目标）。
    pub goal_id: String,
    /// 名称列（目标原名）。
    pub name: String,
    /// 目标金额列（formatAmount 含隐私掩码与数字分组）。
    pub target: String,
    /// 已存列（专属账户余额 = V017 缓存读数，本层只消费）。
    pub saved: String,
    /// 还差列（带符号差值恒显；超额存入后为负）。
    pub remaining: String,
    /// 推算列逐行文本（节奏 / ETA / 所需月存 / 落后超前各自成行，不拼连
    /// 续一行；达成态整体退场 = 空集）。
    pub projection: Vec<String>,
    /// 截止日列（缺席 = 占位破折号，旧壳 `deadline ?? "—"` 同口径）。
    pub deadline: String,
    /// 状态列标签（已达成 / 进行中）。
    pub status: &'static str,
    /// 达成态（状态色与计划提示判定的消费面）。
    pub achieved: bool,
    /// 已归档（双分区判定的同源标记；分区在装配产物上做，不回读域行）。
    pub is_archived: bool,
    // ---- 原值字段（编辑回填 / 删除点名的消费面，渲染不消费）----
    /// 目标金额原值（整数分）。
    pub target_cents: i64,
    /// 手填计划月存原值（整数分，可空）。
    pub planned_monthly_cents: Option<i64>,
    /// 截止日原值（可空，`YYYY-MM-DD`）。
    pub deadline_raw: Option<String>,
    /// 币种原值（回填小数位查询面）。
    pub currency_code: String,
    /// 达成 + 关联计划仍在执行的提示行（旧壳 achievedPlanHint 同口径：
    /// 只提示用户去计划页处理，计划状态不由目标域改动）。
    pub plan_hint: bool,
}

/// 蓄水进度金额文本（含隐私掩码的消费面：推算行内的金额同一口径）。
fn savings_amount_text(cents: i64, fmt: &CurrencyFormat, privacy: bool) -> String {
    if privacy {
        AMOUNT_PRIVACY_MASK.to_owned()
    } else {
        format_amount_basic(cents, fmt)
    }
}

/// 推算列装配入参 face：域蓄水进度行的推算字段全集投影（借用目视面板，
/// 装配期零拷贝）；`savings_goal_projection_lines` 三参数化收口（clippy
/// too_many_arguments 口径，参数闭集一个结构体收齐）。
struct ProjectionFace<'a> {
    /// 达成态（纯展示态，达成即整体退场）。
    achieved: bool,
    /// 节奏金额（整数分，可空 = 节奏为零）。
    pace_monthly_cents: Option<i64>,
    /// 节奏来源闭集二值。
    pace_source: Option<ledger_savings_goal::SavingsGoalPaceSource>,
    /// 截止日（可空）。
    deadline: Option<&'a str>,
    /// 正推月数（域读数）。
    eta_months: Option<i64>,
    /// 正推年月（域读数）。
    eta_month: Option<&'a str>,
    /// 反推所需月存（域读数）。
    required_monthly_cents: Option<i64>,
    /// 反推节奏差值（域读数）。
    pace_delta_cents: Option<i64>,
}

/// 蓄水进度推算列逐行文本（旧壳 projection 渲染器的重生，算术归域读数，
/// 本层只按读数组合文案）：达成态整体退场；节奏行来源闭集二值随行可见；
/// 无截止正推（还差 N 个月 / 预计年月）与设置引导；有截止反推（每月需存 +
/// 落后 / 超前差值）与对比引导；截止已过只给一句话，不虚构时点。
fn savings_goal_projection_lines(
    face: ProjectionFace<'_>,
    fmt: &CurrencyFormat,
    privacy: bool,
) -> Vec<String> {
    let ProjectionFace {
        achieved,
        pace_monthly_cents,
        pace_source,
        deadline,
        eta_months,
        eta_month,
        required_monthly_cents,
        pace_delta_cents,
    } = face;
    if achieved {
        // 达成 = 读时派生纯展示态：时点已无意义，推算整体退场（状态列
        // 表达达成）。
        return Vec::new();
    }
    let mut lines = Vec::new();
    if let Some(pace) = pace_monthly_cents {
        let amount = savings_amount_text(pace, fmt, privacy);
        lines.push(match pace_source {
            Some(ledger_savings_goal::SavingsGoalPaceSource::Plan) => {
                format!("按计划 {amount}/月")
            }
            Some(ledger_savings_goal::SavingsGoalPaceSource::Manual) => {
                format!("手填 {amount}/月")
            }
            // 节奏在场但来源缺席（读数契约外形态）：退化为无来源标签金额行。
            None => format!("{amount}/月"),
        });
    }
    if deadline.is_none() {
        // 无截止正推：还差 N 个月 / 预计年月；节奏为零 → 设置引导（不虚构
        // 时点）。
        match eta_months {
            Some(months) => {
                lines.push(format!("还差 {months} 个月"));
                if let Some(ym) = eta_month {
                    lines.push(format!("预计 {ym} 攒够"));
                }
            }
            None => lines.push("设置计划月存或挂一条自动转账计划，这里显示推算".to_owned()),
        }
    } else if let Some(required) = required_monthly_cents {
        // 有截止反推：每月需存 + 落后 / 超前差值（差值缺席给对比引导）。
        lines.push(format!(
            "每月需存 {}",
            savings_amount_text(required, fmt, privacy)
        ));
        match pace_delta_cents {
            Some(delta) => {
                let amount = savings_amount_text(delta.abs(), fmt, privacy);
                lines.push(if delta >= 0 {
                    format!("超前 {amount}/月")
                } else {
                    format!("落后 {amount}/月")
                });
            }
            None => lines.push("设置计划月存或挂一条自动转账计划，与所需月存对比".to_owned()),
        }
    } else {
        // 截止日已过：不反推所需月存、不虚构时点。
        lines.push("已过截止日".to_owned());
    }
    lines
}

/// 域蓄水进度行集 → 展示格行集（一次性装配，渲染期零格式化；序保持域读序）。
pub fn assemble_savings_goal_rows(
    rows: &[ledger_savings_goal::SavingsGoalProgress],
    currencies: &CurrencyTable,
    privacy: bool,
) -> Vec<SavingsGoalRowCells> {
    rows.iter()
        .map(|row| {
            let fmt = currencies.format_of(&row.currency_code);
            SavingsGoalRowCells {
                goal_id: row.goal.id.clone(),
                name: row.goal.name.clone(),
                target: savings_amount_text(row.goal.target_amount_cents, &fmt, privacy),
                saved: savings_amount_text(row.saved_cents, &fmt, privacy),
                remaining: savings_amount_text(row.remaining_cents, &fmt, privacy),
                projection: savings_goal_projection_lines(
                    ProjectionFace {
                        achieved: row.achieved,
                        pace_monthly_cents: row.pace_monthly_cents,
                        pace_source: row.pace_source,
                        deadline: row.goal.deadline.as_deref(),
                        eta_months: row.eta_months,
                        eta_month: row.eta_month.as_deref(),
                        required_monthly_cents: row.required_monthly_cents,
                        pace_delta_cents: row.pace_delta_cents,
                    },
                    &fmt,
                    privacy,
                ),
                deadline: row.goal.deadline.clone().unwrap_or_else(|| "—".to_owned()),
                status: if row.achieved {
                    "已达成"
                } else {
                    "进行中"
                },
                achieved: row.achieved,
                is_archived: row.goal.status == ledger_savings_goal::SavingsGoalStatus::Archived,
                target_cents: row.goal.target_amount_cents,
                planned_monthly_cents: row.goal.planned_monthly_cents,
                deadline_raw: row.goal.deadline.clone(),
                currency_code: row.currency_code.clone(),
                plan_hint: row.achieved
                    && row.pace_source == Some(ledger_savings_goal::SavingsGoalPaceSource::Plan),
            }
        })
        .collect()
}

/// 双分区（旧壳 activeGoals / archivedGoals 纯派生同口径）：进行中进默认卡、
/// 已归档进归档卡；同一快照的纯派生，零加减零重排（分区内序保持域读序）。
pub fn partition_savings_goal_rows(
    rows: &[SavingsGoalRowCells],
) -> (Vec<SavingsGoalRowCells>, Vec<SavingsGoalRowCells>) {
    let mut active = Vec::new();
    let mut archived = Vec::new();
    for row in rows {
        if row.is_archived {
            archived.push(row.clone());
        } else {
            active.push(row.clone());
        }
    }
    (active, archived)
}

#[cfg(test)]
mod tests {

    use super::*;

    use ledger_transaction::ConvertFields;

    #[test]
    fn amount_semantics_cover_the_whole_kind_set() {
        assert_eq!(
            amount_semantic(TransactionKind::Income),
            AmountSemantic::Income
        );
        assert_eq!(
            amount_semantic(TransactionKind::Expense),
            AmountSemantic::Expense
        );
        assert_eq!(
            amount_semantic(TransactionKind::Transfer),
            AmountSemantic::Transfer
        );
        assert_eq!(
            amount_semantic(TransactionKind::Refund),
            AmountSemantic::Refund
        );
        assert_eq!(amount_semantic(TransactionKind::Buy), AmountSemantic::Buy);
        assert_eq!(amount_semantic(TransactionKind::Sell), AmountSemantic::Sell);
        assert_eq!(
            amount_semantic(TransactionKind::Convert),
            AmountSemantic::Convert
        );
        assert_eq!(
            amount_semantic(TransactionKind::Split),
            AmountSemantic::Split
        );
        assert_eq!(
            amount_semantic(TransactionKind::Dividend),
            AmountSemantic::Dividend
        );
    }

    /// 分类路径表（用例共用）：顶级 餐饮 + 子级 午餐 + 三级链 夜宵 + 孤儿父引用
    /// （指向不存在的父）。软删分类不在此处——路径表只由在用分类行集投影
    /// （boot 读 list_categories 在用口径），不在表中的 id 解析不到。
    fn categories() -> CategoryTable {
        let make = |id: &str, name: &str, parent: Option<&str>| ledger_categories::Category {
            id: id.to_owned(),
            name: name.to_owned(),
            kind: "expense".to_owned(),
            parent_id: parent.map(str::to_owned),
            icon: None,
            sort_order: 0,
            created_at: String::new(),
            updated_at: String::new(),
            version: 1,
            device_id: String::new(),
            is_deleted: false,
        };
        CategoryTable::from_categories(&[
            make("cat-food", "餐饮", None),
            make("cat-lunch", "午餐", Some("cat-food")),
            make("cat-mid", "夜宵", Some("cat-lunch")),
            make("cat-orphan", "孤儿", Some("cat-gone")),
        ])
    }

    #[allow(clippy::inconsistent_digit_grouping)]
    #[test]
    fn category_paths_follow_the_old_shell_rules() {
        let table = categories();
        assert_eq!(
            table.path_of(Some("cat-food")),
            Some("餐饮".to_owned()),
            "顶级分类为自身名"
        );
        assert_eq!(
            table.path_of(Some("cat-lunch")),
            Some("餐饮 > 午餐".to_owned()),
            "子分类为「父 > 子」路径"
        );
        assert_eq!(
            table.path_of(Some("cat-mid")),
            Some("午餐 > 夜宵".to_owned()),
            "更深层截断为两级（旧壳实现即单跳拼接，分类实体两级为设计形态）"
        );
        assert_eq!(
            table.path_of(Some("cat-orphan")),
            Some("孤儿".to_owned()),
            "父引用解析不到回退自身名"
        );
        assert_eq!(
            table.path_of(Some("cat-gone")),
            None,
            "id 不在表中（软删分类等，投影输入只含在用行）解析不到"
        );
        assert_eq!(table.path_of(None), None, "无分类引用");
    }

    /// 造一笔最小读模型行（只填金额口径相关字段，其余零值）。
    fn row(kind: TransactionKind, native_cents: i64, code: &str) -> Transaction {
        Transaction {
            id: format!("tx-{kind:?}-{native_cents}"),
            kind,
            amount_cents: native_cents,
            currency_code: code.to_owned(),
            amount_native_cents: native_cents,
            account_id: None,
            to_account_id: None,
            funding_account_id: None,
            category_id: None,
            merchant_id: None,
            policy_id: None,
            refund_of_transaction_id: None,
            fx_rate_used: None,
            fx_rate_source: None,
            note: None,
            source_order_no: None,
            date: "2026-01-15".to_owned(),
            created_at: String::new(),
            updated_at: String::new(),
            version: 1,
            device_id: String::new(),
            is_deleted: false,
            fundings: Vec::new(),
            purchases: Vec::new(),
            source: None,
            convert: None,
        }
    }

    fn table() -> CurrencyTable {
        CurrencyTable::from_currencies(&[
            Currency {
                code: "CNY".to_owned(),
                name: "人民币".to_owned(),
                symbol: "¥".to_owned(),
                decimal_places: 2,
            },
            Currency {
                code: "JPY".to_owned(),
                name: "日元".to_owned(),
                symbol: "¥".to_owned(),
                decimal_places: 0,
            },
        ])
    }

    #[test]
    fn columns_match_the_old_shell_set_and_order() {
        let keys: Vec<_> = COLUMNS.iter().map(|c| c.key).collect();
        assert_eq!(
            keys,
            [
                "date", "kind", "category", "merchant", "account", "source", "note", "amount"
            ],
            "列定义与顺序同旧壳桌面表格（操作列随 #2022 行菜单）"
        );
        let titles: Vec<_> = COLUMNS.iter().map(|c| c.title).collect();
        assert_eq!(
            titles,
            [
                "日期",
                "类型",
                "分类",
                "商户",
                "账户",
                "来源",
                "商品 / 备注",
                "金额"
            ],
            "列名 zh-CN 与旧壳 transactions.columns 词表同文案（展开视图一列两用）"
        );
        let amount = COLUMNS.iter().find(|c| c.key == "amount").unwrap();
        assert_eq!(amount.align, ColumnAlign::End, "金额列末端对齐");
    }

    #[test]
    fn kind_labels_cover_the_whole_closed_set() {
        let plain = LendingDirection::None;
        assert_eq!(kind_label(TransactionKind::Income, plain), "收入");
        assert_eq!(kind_label(TransactionKind::Expense, plain), "支出");
        assert_eq!(kind_label(TransactionKind::Transfer, plain), "转账");
        assert_eq!(kind_label(TransactionKind::Refund, plain), "退款");
        assert_eq!(kind_label(TransactionKind::Buy, plain), "买入");
        assert_eq!(kind_label(TransactionKind::Sell, plain), "卖出");
        assert_eq!(kind_label(TransactionKind::Convert, plain), "转换");
        assert_eq!(kind_label(TransactionKind::Split, plain), "份额调整");
        assert_eq!(kind_label(TransactionKind::Dividend, plain), "分红");
        // 借贷派生（transfer 专属）：四方向专属文案。
        assert_eq!(
            kind_label(TransactionKind::Transfer, LendingDirection::Lend),
            "借出"
        );
        assert_eq!(
            kind_label(TransactionKind::Transfer, LendingDirection::Collect),
            "收回"
        );
        assert_eq!(
            kind_label(TransactionKind::Transfer, LendingDirection::Borrow),
            "借入"
        );
        assert_eq!(
            kind_label(TransactionKind::Transfer, LendingDirection::Repay),
            "还款"
        );
    }

    // 断言输入的下划线按 zh-CN 四位分组标注读数（12_345_6789 = 1,2345,6789 分），
    // 与 clippy 的三位分组建议有意不一致——意图优先，局部豁免。
    #[allow(clippy::inconsistent_digit_grouping)]
    #[test]
    fn basic_amount_formats_cents_with_symbol_grouping_and_trim() {
        let cny = table().format_of("CNY");
        assert_eq!(format_amount_basic(2_500, &cny), "¥25", "去尾零 25.00→25");
        assert_eq!(
            format_amount_basic(2_550, &cny),
            "¥25.5",
            "去尾零 25.50→25.5"
        );
        assert_eq!(format_amount_basic(2_555, &cny), "¥25.55");
        assert_eq!(format_amount_basic(-123_45, &cny), "-¥123.45", "负号在最前");
        assert_eq!(
            format_amount_basic(0, &cny),
            "¥0",
            "零金额与旧壳算法同形（去尾零不跨小数点回溯）"
        );
        assert_eq!(
            format_amount_basic(12_345_6789, &cny),
            "¥123,4567.89",
            "整数部分 zh-CN 四位分组、小数部分不分组"
        );
        let jpy = table().format_of("JPY");
        assert_eq!(
            format_amount_basic(123_450, &jpy),
            "¥12,3450",
            "零小数位币种不出现小数点"
        );
    }

    #[allow(clippy::inconsistent_digit_grouping)]
    #[test]
    fn unknown_currency_falls_back_to_plain_two_decimals() {
        let unknown = table().format_of("XYZ");
        assert_eq!(unknown.symbol, "", "未登记币种无符号");
        assert_eq!(unknown.decimal_places, 2, "未登记币种缺省两位小数");
        assert_eq!(format_amount_basic(123_45, &unknown), "123.45");
    }

    #[test]
    fn amount_anchor_follows_the_display_rules() {
        let mut convert = row(TransactionKind::Convert, 999, "CNY");
        convert.convert = Some(ConvertFields {
            to_instrument_id: "ins-1".to_owned(),
            to_symbol: "F001".to_owned(),
            to_quantity: 10.0,
            out_amount_cents: 50_000,
            in_amount_cents: 49_000,
        });
        assert_eq!(
            amount_display_cents(&convert),
            Some(50_000),
            "转换行展示转出金额（确认单权威），不读结转成本锚点"
        );
        let convert_without_fields = row(TransactionKind::Convert, 999, "CNY");
        assert_eq!(
            amount_display_cents(&convert_without_fields),
            Some(999),
            "扩展缺失（旧数据）回退行金额锚点"
        );
        assert_eq!(
            amount_display_cents(&row(TransactionKind::Split, 888, "CNY")),
            None,
            "份额调整无现金腿、无金额"
        );
        assert_eq!(
            amount_display_cents(&row(TransactionKind::Expense, 2_500, "CNY")),
            Some(2_500),
            "其余行走本位币锚点"
        );
    }

    /// 装配上下文（用例共用）：币种表 + 分类路径表 + 账户表 + 商户表 + 隐私开关。
    fn context(privacy: bool) -> RowContext<'static> {
        RowContext {
            currencies: Box::leak(Box::new(table())),
            categories: Box::leak(Box::new(categories())),
            accounts: Box::leak(Box::new(accounts())),
            merchants: Box::leak(Box::new(merchants())),
            privacy_enabled: privacy,
        }
    }

    #[test]
    fn assemble_rows_produce_final_cell_texts() {
        let mut split = row(TransactionKind::Split, 0, "CNY");
        split.id = "tx-split".to_owned();
        split.date = "2026-02-01".to_owned();
        let mut expense = row(TransactionKind::Expense, 2_500, "CNY");
        expense.category_id = Some("cat-lunch".to_owned());
        expense.note = Some("午餐一顿".to_owned());
        let rows = assemble_rows(&[expense, split], &context(false));
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            RowCells {
                id: "tx-Expense-2500".to_owned(),
                date: "2026-01-15".to_owned(),
                kind: "支出".to_owned(),
                category: "餐饮 > 午餐".to_owned(),
                merchant: "-".to_owned(),
                account: "-".to_owned(),
                source: "-".to_owned(),
                note: "午餐一顿".to_owned(),
                amount: "¥25".to_owned(),
                amount_semantic: AmountSemantic::Expense,
            }
        );
        assert_eq!(rows[1].amount, "-", "无金额行渲染 -");
        assert_eq!(rows[1].kind, "份额调整");
        assert_eq!(rows[1].category, "-", "无分类引用渲染 -");
        assert_eq!(rows[1].note, "-", "无备注渲染 -");
        assert_eq!(rows[1].amount_semantic, AmountSemantic::Split);
    }

    /// 分类列：孤儿父引用回退自身名；不在表中的 id（软删分类等）落 `-`；
    /// 备注列无备注同样 `-`。
    #[test]
    fn unresolved_category_and_absent_note_render_dash() {
        let mut orphan = row(TransactionKind::Expense, 2_500, "CNY");
        orphan.category_id = Some("cat-orphan".to_owned());
        orphan.note = Some("带备注".to_owned());
        let mut missing = row(TransactionKind::Expense, 1_000, "CNY");
        missing.category_id = Some("cat-gone".to_owned());
        let rows = assemble_rows(&[orphan, missing], &context(false));
        assert_eq!(rows[0].category, "孤儿", "孤儿父引用回退自身名，不落 -");
        assert_eq!(rows[0].note, "带备注");
        assert_eq!(
            rows[1].category, "-",
            "路径表只收在用分类：软删分类的引用落 -（旧壳在用分类口径）"
        );
        assert_eq!(rows[1].note, "-");
    }

    /// 隐私掩码（issue #566 / spec #564 掩码恒等性）：开 = 全部金额同形
    /// `••••`（正负、零、多币种一视同仁）；`-`（份额调整无金额）不是金额，
    /// 照旧渲染；语义色照常保留（掩码隐藏数字，不隐藏方向——旧壳隐私保留项）；
    /// 关 = 原格式化不变。
    #[allow(clippy::inconsistent_digit_grouping)]
    #[test]
    fn privacy_mask_hides_every_amount_and_keeps_dash_and_semantics() {
        let rows = [
            row(TransactionKind::Expense, -12_345_6789, "CNY"),
            row(TransactionKind::Income, 0, "CNY"),
            row(TransactionKind::Dividend, 123_450, "JPY"),
            row(TransactionKind::Split, 0, "CNY"),
        ];
        let masked = assemble_rows(&rows, &context(true));
        assert_eq!(masked[0].amount, AMOUNT_PRIVACY_MASK, "负金额同形掩码");
        assert_eq!(masked[1].amount, AMOUNT_PRIVACY_MASK, "零金额同形掩码");
        assert_eq!(
            masked[2].amount, AMOUNT_PRIVACY_MASK,
            "多币种同形掩码（无币符）"
        );
        assert_eq!(masked[3].amount, "-", "空值口径不受掩码影响");
        assert_eq!(
            masked[0].amount_semantic,
            AmountSemantic::Expense,
            "掩码态语义色保留"
        );
        assert_eq!(masked[2].amount_semantic, AmountSemantic::Dividend);
        let plain = assemble_rows(&rows, &context(false));
        assert_eq!(plain[0].amount, "-¥123,4567.89");
        assert_eq!(plain[1].amount, "¥0");
        assert_eq!(plain[2].amount, "¥12,3450", "隐私关闭 = 原格式化");
    }

    /// 账户表（用例共用）：现金 / 银行 / 应收 / 负债 / 投资账户；隐藏与软删账户
    /// 不在表中（投影只收可见行——「不在表中」即黑洞/隐藏的表达形态）。
    fn accounts() -> AccountTable {
        let make = |id: &str, name: &str, kind: AccountType| ledger_accounts::Account {
            id: id.to_owned(),
            name: name.to_owned(),
            kind,
            currency_code: "CNY".to_owned(),
            initial_balance_cents: 0,
            created_at: String::new(),
            updated_at: String::new(),
            version: 1,
            device_id: String::new(),
            is_deleted: false,
            is_hidden: false,
            is_archived: false,
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        };
        AccountTable::from_accounts(&[
            make("acc-cash", "现金", AccountType::Cash),
            make("acc-bank", "银行", AccountType::Bank),
            make("acc-receivable", "应收款", AccountType::Receivable),
            make("acc-debt", "花呗", AccountType::Debt),
            make("acc-invest", "证券账户", AccountType::Investment),
        ])
    }

    /// 商户表（用例共用）：在用京东 + 软删商户（历史引用照常可解析）。
    fn merchants() -> MerchantTable {
        let make = |id: &str, name: &str, deleted: bool| ledger_merchants::Merchant {
            id: id.to_owned(),
            name: name.to_owned(),
            created_at: String::new(),
            updated_at: String::new(),
            version: 1,
            device_id: String::new(),
            is_deleted: deleted,
        };
        MerchantTable::from_merchants(&[
            make("m-jd", "京东", false),
            make("m-gone", "已删商户", true),
        ])
    }

    /// 造出资项条目（读模型）。
    fn funding(account: &str, cents: i64, derived: bool) -> ledger_transaction::TransactionFunding {
        ledger_transaction::TransactionFunding {
            account_id: account.to_owned(),
            amount_cents: cents,
            label: None,
            derived,
        }
    }

    /// 造购买项条目（读模型）。
    fn purchase(
        name: &str,
        quantity: i64,
        category: Option<&str>,
    ) -> ledger_transaction::TransactionPurchase {
        ledger_transaction::TransactionPurchase {
            name: name.to_owned(),
            quantity,
            category_id: category.map(str::to_owned),
            unit_price_cents: None,
        }
    }

    /// 造来源投影（读模型）。
    fn source(
        kind: TransactionSourceKind,
        name: &str,
        status: Option<TransactionSourceStatus>,
    ) -> ledger_transaction::TransactionSource {
        ledger_transaction::TransactionSource {
            kind,
            entity_id: "ent-1".to_owned(),
            display_name: name.to_owned(),
            status,
        }
    }

    #[allow(clippy::inconsistent_digit_grouping)]
    #[test]
    fn purchase_orders_expand_per_line_with_order_level_cells_on_first_row() {
        let mut tx = row(TransactionKind::Expense, 28_160, "CNY");
        tx.id = "tx-order".to_owned();
        tx.date = "2026-03-01".to_owned();
        tx.merchant_id = Some("m-jd".to_owned());
        tx.category_id = Some("cat-lunch".to_owned());
        tx.fundings = vec![
            funding("acc-cash", 2_853, false),
            funding("acc-bank", 25_307, false),
        ];
        tx.purchases = vec![
            purchase("猫粮", 2, Some("cat-food")),
            purchase("洗衣液", 1, Some("cat-mid")),
        ];
        let rows = assemble_rows(&[tx], &context(false));
        assert_eq!(rows.len(), 2, "两个购买项各占一行");
        assert_eq!(rows[0].id, "tx-order");
        assert_eq!(rows[0].date, "2026-03-01", "块首行订单级列带值");
        assert_eq!(rows[0].kind, "支出");
        assert_eq!(rows[0].merchant, "京东");
        assert_eq!(
            rows[0].account, "现金 等 2 账户",
            "出资分解标注（首条 + 等 N 账户）"
        );
        assert_eq!(rows[0].source, "-");
        assert_eq!(rows[0].amount, "¥281.6", "金额 = 订单实付，只在块首行");
        assert_eq!(rows[0].category, "餐饮", "分类列逐行显示购买项自己的分类");
        assert_eq!(
            rows[0].note, "猫粮 共 2 件",
            "备注列一列两用（商品名 + 共 N 件）"
        );
        assert_eq!(rows[1].id, "tx-order", "派生行 id 恒为交易 id");
        for (column, value) in [
            ("date", rows[1].date.as_str()),
            ("kind", rows[1].kind.as_str()),
            ("merchant", rows[1].merchant.as_str()),
            ("account", rows[1].account.as_str()),
            ("source", rows[1].source.as_str()),
            ("amount", rows[1].amount.as_str()),
        ] {
            assert_eq!(
                value, "",
                "续行订单级格留空（合并格内容不重复出现）：{column}"
            );
        }
        assert_eq!(rows[1].category, "午餐 > 夜宵", "续行分类取自己的分类");
        assert_eq!(rows[1].note, "洗衣液 共 1 件");
        assert_eq!(rows[1].amount_semantic, AmountSemantic::Expense);
    }

    /// 无购买项行零变化：单行交易的订单级列恒带值（不存在块首行语义），商户/
    /// 账户/来源按各自口径渲染；转账双账户「转出 → 转入」。
    #[test]
    fn plain_rows_keep_all_order_level_cells() {
        let mut transfer = row(TransactionKind::Transfer, 5_000, "CNY");
        transfer.account_id = Some("acc-cash".to_owned());
        transfer.to_account_id = Some("acc-bank".to_owned());
        transfer.note = Some("转账备注".to_owned());
        let rows = assemble_rows(&[transfer], &context(false));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "转账");
        assert_eq!(rows[0].account, "现金 → 银行", "双账户标注（转出 → 转入）");
        assert_eq!(rows[0].merchant, "-", "无商户引用渲染 -");
        assert_eq!(rows[0].source, "-", "无来源渲染 -");
        assert_eq!(rows[0].note, "转账备注");
        assert_eq!(rows[0].amount, "¥50");
    }

    /// 出资分解标注三态 + 派生标注（词汇表「出资项」读侧）：多账户「等 N 账户」/
    /// 同账户多条「同账户 N 笔」/ 单条无标注；refund 派生分解另带「按比例自动分解」。
    #[test]
    fn funding_markers_follow_the_old_shell_wording() {
        let make = |fundings: Vec<ledger_transaction::TransactionFunding>| {
            let mut tx = row(TransactionKind::Expense, 10_000, "CNY");
            tx.fundings = fundings;
            tx
        };
        let rows = assemble_rows(
            &[
                make(vec![
                    funding("acc-cash", 6_000, false),
                    funding("acc-bank", 4_000, false),
                ]),
                make(vec![
                    funding("acc-cash", 6_000, false),
                    funding("acc-cash", 4_000, false),
                ]),
                make(vec![funding("acc-cash", 10_000, false)]),
                make(vec![
                    funding("acc-cash", 6_000, true),
                    funding("acc-bank", 4_000, true),
                ]),
            ],
            &context(false),
        );
        assert_eq!(rows[0].account, "现金 等 2 账户");
        assert_eq!(rows[1].account, "现金 同账户 2 笔");
        assert_eq!(rows[2].account, "现金", "单条出资无标注");
        assert_eq!(
            rows[3].account, "现金 等 2 账户 按比例自动分解",
            "派生分解另带标注（旧壳第二行标注的平铺镜像）"
        );
    }

    /// 双账户三形态（旧壳 accountCellView）：转账「转出 → 转入」；buy「资金流出方
    /// 在前」（出资账户 → 投资账户）；sell 反向；黑洞/隐藏账户（不在账户表）占位「-」。
    #[test]
    fn two_account_forms_follow_the_old_shell_direction_rules() {
        let make = |kind: TransactionKind,
                    account: Option<&str>,
                    to: Option<&str>,
                    funding: Option<&str>| {
            let mut tx = row(kind, 1_000, "CNY");
            tx.account_id = account.map(str::to_owned);
            tx.to_account_id = to.map(str::to_owned);
            tx.funding_account_id = funding.map(str::to_owned);
            tx
        };
        let rows = assemble_rows(
            &[
                make(
                    TransactionKind::Transfer,
                    Some("acc-cash"),
                    Some("acc-bank"),
                    None,
                ),
                make(
                    TransactionKind::Buy,
                    Some("acc-invest"),
                    None,
                    Some("acc-cash"),
                ),
                make(
                    TransactionKind::Sell,
                    Some("acc-invest"),
                    None,
                    Some("acc-cash"),
                ),
                make(TransactionKind::Expense, Some("acc-gone"), None, None),
                make(
                    TransactionKind::Transfer,
                    Some("acc-cash"),
                    Some("acc-gone"),
                    None,
                ),
            ],
            &context(false),
        );
        assert_eq!(rows[0].account, "现金 → 银行");
        assert_eq!(rows[1].account, "现金 → 证券账户", "buy：出资账户在前");
        assert_eq!(rows[2].account, "证券账户 → 现金", "sell：投资账户在前");
        assert_eq!(rows[3].account, "-", "黑洞/隐藏账户不在账户表，占位 -");
        assert_eq!(rows[4].account, "现金 → -");
    }

    /// 借贷派生标签（旧壳 kindLabel / lending.ts 镜像）：借贷由两端账户类型派生，
    /// 仅 transfer；对端类型缺失（黑洞/隐藏）不退回普通转账——借贷侧唯一决定。
    #[test]
    fn lending_transfers_derive_direction_labels_from_account_types() {
        let make = |account: Option<&str>, to: Option<&str>| {
            let mut tx = row(TransactionKind::Transfer, 1_000, "CNY");
            tx.account_id = account.map(str::to_owned);
            tx.to_account_id = to.map(str::to_owned);
            tx
        };
        let mut expense = row(TransactionKind::Expense, 1_000, "CNY");
        expense.account_id = Some("acc-cash".to_owned());
        expense.to_account_id = Some("acc-debt".to_owned());
        let rows = assemble_rows(
            &[
                make(Some("acc-cash"), Some("acc-receivable")),
                make(Some("acc-receivable"), Some("acc-cash")),
                make(Some("acc-debt"), Some("acc-cash")),
                make(Some("acc-cash"), Some("acc-debt")),
                make(Some("acc-cash"), Some("acc-bank")),
                make(Some("acc-gone"), Some("acc-receivable")),
                make(Some("acc-gone"), Some("acc-bank")),
                expense,
            ],
            &context(false),
        );
        assert_eq!(rows[0].kind, "借出");
        assert_eq!(rows[1].kind, "收回");
        assert_eq!(rows[2].kind, "借入");
        assert_eq!(rows[3].kind, "还款");
        assert_eq!(rows[4].kind, "转账", "资金互转无借贷语义");
        assert_eq!(
            rows[5].kind, "借出",
            "对端缺失归一资金侧，借贷侧决定方向（不退回普通转账）"
        );
        assert_eq!(rows[6].kind, "转账", "两端均资金侧（缺失归一）无借贷语义");
        assert_eq!(rows[7].kind, "支出", "借贷派生仅作用于 transfer");
    }

    /// 来源格（spec #704 来源列）：展示名在场用展示名；空串按来源类型名兜底；
    /// 状态标注随名拼接（已取消/已处置/已删除）；无来源渲染「-」（plain_rows 用例）。
    #[test]
    fn source_cells_carry_display_name_kind_fallback_and_status() {
        let mut named = row(TransactionKind::Expense, 1_000, "CNY");
        named.source = Some(source(
            TransactionSourceKind::Subscription,
            "流媒体会员",
            Some(TransactionSourceStatus::Cancelled),
        ));
        let mut fallback = row(TransactionKind::Expense, 1_000, "CNY");
        fallback.source = Some(source(TransactionSourceKind::Policy, "", None));
        let mut deleted = row(TransactionKind::Expense, 1_000, "CNY");
        deleted.source = Some(source(
            TransactionSourceKind::Policy,
            "重疾险",
            Some(TransactionSourceStatus::Deleted),
        ));
        let mut disposed = row(TransactionKind::Expense, 1_000, "CNY");
        disposed.source = Some(source(
            TransactionSourceKind::Item,
            "咖啡机",
            Some(TransactionSourceStatus::Disposed),
        ));
        let rows = assemble_rows(&[named, fallback, deleted, disposed], &context(false));
        assert_eq!(rows[0].source, "流媒体会员 已取消");
        assert_eq!(rows[1].source, "保单", "展示名空串按类型名兜底");
        assert_eq!(rows[2].source, "重疾险 已删除");
        assert_eq!(rows[3].source, "咖啡机 已处置");
    }

    /// 商户格：映射命中显示名称（含软删商户——历史交易显示口径）；无引用或
    /// 解析不到渲染「-」。
    #[test]
    fn merchant_cells_resolve_names_including_soft_deleted() {
        let mut named = row(TransactionKind::Expense, 1_000, "CNY");
        named.merchant_id = Some("m-jd".to_owned());
        let mut deleted = row(TransactionKind::Expense, 1_000, "CNY");
        deleted.merchant_id = Some("m-gone".to_owned());
        let mut missing = row(TransactionKind::Expense, 1_000, "CNY");
        missing.merchant_id = Some("m-none".to_owned());
        let rows = assemble_rows(&[named, deleted, missing], &context(false));
        assert_eq!(rows[0].merchant, "京东");
        assert_eq!(rows[1].merchant, "已删商户", "软删商户历史引用照常显示");
        assert_eq!(rows[2].merchant, "-", "解析不到渲染 -");
    }

    #[test]
    fn total_pages_counts_partial_last_page() {
        assert_eq!(total_pages(0), 1, "空库恒 1 页（分页条由 UI 收起）");
        assert_eq!(total_pages(1), 1);
        assert_eq!(total_pages(PAGE_SIZE as i64), 1, "整页边界");
        assert_eq!(total_pages(PAGE_SIZE as i64 + 1), 2, "末页不满一页也算一页");
        assert_eq!(total_pages(250), 3);
        assert_eq!(total_pages(-5), 1, "负值（不可达口径）钳到 1");
    }

    #[test]
    fn page_after_row_delete_stays_and_falls_back_like_the_old_shell() {
        // 旧壳 afterRowDelete（ADR-0045）的决策半边：本页删后剩 0 条且当前页
        // 非第一页才回退一页，其余一律保持当前页（None）。
        assert_eq!(page_after_row_delete(1, 100), None, "首页删一行不回退");
        assert_eq!(page_after_row_delete(1, 1), None, "首页删空也停在首页");
        assert_eq!(
            page_after_row_delete(2, 150),
            None,
            "末页 50 行删一行剩 49 条：保持当前页"
        );
        assert_eq!(
            page_after_row_delete(2, 101),
            Some(1),
            "第 2 页仅 1 行：删后本页空，回退到第 1 页"
        );
        assert_eq!(
            page_after_row_delete(3, 201),
            Some(2),
            "第 3 页仅 1 行：删后回退"
        );
        assert_eq!(
            page_after_row_delete(2, 200),
            None,
            "整页删一行剩 99 条：保持当前页"
        );
    }

    #[test]
    fn next_page_is_idempotent_at_both_ends() {
        // 首页上一页 / 末页下一页：越界幂等无操作（None）。
        assert_eq!(next_page(1, 3, PageStep::Prev), None, "首页上一页无操作");
        assert_eq!(next_page(3, 3, PageStep::Next), None, "末页下一页无操作");
        // 中间页双向步进。
        assert_eq!(next_page(2, 3, PageStep::Prev), Some(1));
        assert_eq!(next_page(2, 3, PageStep::Next), Some(3));
        // 单页：双向皆无操作。
        assert_eq!(next_page(1, 1, PageStep::Prev), None);
        assert_eq!(next_page(1, 1, PageStep::Next), None);
        // 页码越界（不可达口径）钳到总页数口径内判定。
        assert_eq!(
            next_page(2, 1, PageStep::Prev),
            Some(1),
            "越界页码回退仍合法"
        );
        assert_eq!(next_page(9, 1, PageStep::Next), None);
    }

    // -----------------------------------------------------------------------
    // 预算装配族（#2033）：预算视图的格文本与进度口径——渲染期零格式化纪律
    // 同款：分类树选项（闭集与「父 > 子」路径名）、周期标签、进度百分比封顶、
    // 金额文本（隐私遮蔽）与分类显示名回退（孤儿预算）。
    // -----------------------------------------------------------------------

    fn category(
        id: &str,
        name: &str,
        kind: &str,
        parent: Option<String>,
        sort: i64,
    ) -> ledger_categories::Category {
        ledger_categories::Category {
            id: id.to_owned(),
            name: name.to_owned(),
            kind: kind.to_owned(),
            parent_id: parent,
            icon: None,
            sort_order: sort,
            created_at: String::new(),
            updated_at: String::new(),
            version: 0,
            device_id: String::new(),
            is_deleted: false,
        }
    }

    #[test]
    fn budget_period_labels_use_zh_words() {
        use ledger_budget::BudgetPeriod;
        assert_eq!(budget_period_label(BudgetPeriod::Monthly), "按月");
        assert_eq!(budget_period_label(BudgetPeriod::Yearly), "按年");
    }

    #[test]
    fn budget_progress_percent_caps_at_hundred_and_guards_zero_amount() {
        assert_eq!(budget_progress_percent(0, 50_000), 0.0);
        assert_eq!(budget_progress_percent(25_000, 50_000), 50.0);
        assert_eq!(
            budget_progress_percent(12_345, 50_000),
            25.0,
            "四舍五入口径"
        );
        assert_eq!(
            budget_progress_percent(60_000, 50_000),
            100.0,
            "超支封顶 100"
        );
        assert_eq!(budget_progress_percent(10, 0), 0.0, "零预算守卫");
        assert_eq!(budget_progress_percent(10, -5), 0.0, "非正预算守卫");
    }

    #[test]
    fn budget_summary_percent_caps_at_hundred() {
        assert_eq!(budget_summary_percent(0, 600_000), 0.0);
        assert_eq!(budget_summary_percent(300_000, 600_000), 50.0);
        assert_eq!(budget_summary_percent(700_000, 600_000), 100.0, "封顶 100");
        assert_eq!(budget_summary_percent(10, 0), 0.0, "零总额守卫");
    }

    #[test]
    fn budget_amount_texts_group_and_mask() {
        // 旧壳 formatAmount 无币种参数口径：无符号、两位小数、去尾零、zh 四位一组。
        assert_eq!(budget_amount_text(50_000, false), "500");
        assert_eq!(
            budget_amount_text(123_456, false),
            "1234.56",
            "四位一组：4 位整数单组不插分隔"
        );
        assert_eq!(budget_amount_text(1_2345_6789, false), "123,4567.89");
        assert_eq!(budget_amount_text(0, false), "0");
        // 隐私遮蔽：与交易表格同掩码。
        assert_eq!(budget_amount_text(50_000, true), "••••");
    }

    #[test]
    fn budget_status_words_distinguish_row_and_summary() {
        assert_eq!(budget_status_text(false), "正常");
        assert_eq!(budget_status_text(true), "超支");
        assert_eq!(budget_summary_status_text(false), "正常");
        assert_eq!(budget_summary_status_text(true), "已超标");
    }

    #[test]
    fn kind_filter_options_follow_main_list_closed_set_order() {
        let options = kind_filter_options();
        assert_eq!(
            options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
            vec!["income", "expense", "transfer", "refund"],
            "可选集 = 四通用 kind（ADR-0135 主列表收窄），闭集序"
        );
        assert_eq!(
            options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(),
            vec!["收入", "支出", "转账", "退款"],
            "label 经 kind_label 同一词表"
        );
    }

    #[test]
    fn list_empty_text_covers_three_way_split() {
        assert_eq!(list_empty_text(true, false), "无匹配结果", "搜索态无命中");
        assert_eq!(list_empty_text(true, true), "无匹配结果", "搜索态优先");
        assert_eq!(
            list_empty_text(false, true),
            "没有符合条件的交易",
            "列表态筛选无结果（旧壳 list.emptyFiltered）"
        );
        assert_eq!(list_empty_text(false, false), "暂无数据", "列表态空库");
    }

    // ---- 搜索视图装配家族（#2040，旧壳 SearchView 同名 computed 的纯函数
    // 化）：查询判定 / 筛选激活 / 描述拼接 / 空态分型。 ----

    #[test]
    fn search_has_query_requires_keyword_or_any_filter() {
        // 全空：无查询。
        assert!(!search_has_query("", None, None, None, None));
        assert!(
            !search_has_query("   ", None, None, None, None),
            "纯空白同空"
        );
        // 仅关键词。
        assert!(search_has_query("火锅", None, None, None, None));
        // 仅筛选（无关键字仅筛选查询，旧壳 hasQuery 同口径）。
        assert!(search_has_query("", Some(10_000), None, None, None));
        assert!(search_has_query("", None, Some(10_000), None, None));
        assert!(search_has_query("", None, None, Some("2026-01-01"), None));
        assert!(search_has_query("", None, None, None, Some("2026-01-31")));
    }

    #[test]
    fn search_filters_active_excludes_keyword_and_page_ideas() {
        assert!(!search_filters_active(None, None, None, None));
        assert!(search_filters_active(Some(1), None, None, None));
        assert!(search_filters_active(None, Some(1), None, None));
        assert!(search_filters_active(None, None, Some("2026-01-01"), None));
        assert!(search_filters_active(None, None, None, Some("2026-01-31")));
    }

    #[test]
    fn search_filter_descriptions_follow_old_shell_wording_and_order() {
        // 双端金额：区间形态（缺省币种格式：无币符、两位小数、zh 四位一组
        // ——千位内不插分隔符）。
        assert_eq!(
            search_filter_descriptions(Some(100_000), Some(500_000), None, None),
            vec!["金额 1000 ~ 5000"]
        );
        // 单端：最低/最高形态。
        assert_eq!(
            search_filter_descriptions(Some(100_000), None, None, None),
            vec!["最低 1000"]
        );
        assert_eq!(
            search_filter_descriptions(None, Some(50_500), None, None),
            vec!["最高 505"]
        );
        // 期间：起始/结束两描述（QuickTimeRange 产出闭集为双端有界或双空，
        // 单端在 UI 不可达但拼接面保留逐端描述——旧壳同口径）。
        assert_eq!(
            search_filter_descriptions(None, None, Some("2026-01-01"), Some("2026-01-31")),
            vec!["起始 2026-01-01", "结束 2026-01-31"]
        );
        // 全量：金额在前、期间在后（旧壳 parts 拼接序）。
        assert_eq!(
            search_filter_descriptions(
                Some(100_000),
                Some(500_000),
                Some("2026-01-01"),
                Some("2026-01-31")
            ),
            vec!["金额 1000 ~ 5000", "起始 2026-01-01", "结束 2026-01-31"]
        );
        // 全空：无描述。
        assert_eq!(
            search_filter_descriptions(None, None, None, None),
            Vec::<String>::new()
        );
    }

    #[test]
    fn search_empty_text_splits_placeholder_and_no_hits() {
        assert_eq!(
            search_empty_text(false),
            "输入关键字或设置筛选开始搜索",
            "未搜索占位（旧壳 search.emptyPlaceholder）"
        );
        assert_eq!(search_empty_text(true), "无匹配结果", "已搜索无命中");
    }

    #[test]
    fn expense_category_options_follow_tree_order_with_path_labels() {
        let rows = vec![
            category("c1", "餐饮", "expense", None, 2),
            category("c2", "交通", "expense", None, 1),
            category("c3", "工资", "income", None, 0),
            category("c4", "午餐", "expense", Some("c1".into()), 2),
            category("c5", "火锅", "expense", Some("c1".into()), 1),
            category("c6", "地铁", "expense", Some("c2".into()), 1),
        ];
        let options = expense_category_options(&rows);
        let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["交通", "交通 > 地铁", "餐饮", "餐饮 > 火锅", "餐饮 > 午餐"],
            "只收支出分类（收入排除）；父按 sort_order、子紧跟父且自身按 sort_order；子分类路径名"
        );
        assert_eq!(options[0].id, "c2");
        assert_eq!(options[3].id, "c5");
    }

    #[test]
    fn budget_category_display_falls_back_for_orphans() {
        let rows = vec![
            category("c1", "餐饮", "expense", None, 1),
            category("c2", "午餐", "expense", Some("c1".into()), 1),
        ];
        let table = CategoryTable::from_categories(&rows);
        assert_eq!(budget_category_display(&table, "c1"), "餐饮");
        assert_eq!(budget_category_display(&table, "c2"), "餐饮 > 午餐");
        assert_eq!(
            budget_category_display(&table, "gone"),
            "未分类",
            "孤儿预算（分类已删）回退「未分类」（旧壳同词）"
        );
    }

    #[test]
    fn budget_yuan_cents_conversions_round_like_old_shell() {
        assert_eq!(budget_yuan_to_cents("80"), Some(8_000));
        assert_eq!(budget_yuan_to_cents("80.1"), Some(8_010));
        assert_eq!(budget_yuan_to_cents("0.05"), Some(5));
        assert_eq!(budget_yuan_to_cents("-3"), Some(-300));
        assert_eq!(budget_yuan_to_cents("abc"), None, "非数拒绝");
        assert_eq!(budget_yuan_to_cents(""), None);
        assert_eq!(
            budget_yuan_to_cents("1e3"),
            None,
            "科学计数拒绝（旧壳同口径）"
        );
        assert_eq!(budget_cents_to_yuan_text(50_000), "500");
        assert_eq!(budget_cents_to_yuan_text(12345), "123.45");
        assert_eq!(budget_cents_to_yuan_text(0), "0");
    }

    #[test]
    fn budget_rows_assemble_into_display_cells() {
        use ledger_budget::{Budget, BudgetPeriod, BudgetProgress};
        let budget = |id: &str, amount: i64, period: BudgetPeriod| Budget {
            id: id.to_owned(),
            category_id: "c1".to_owned(),
            period,
            amount_cents: amount,
            start_date: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            version: 0,
            device_id: String::new(),
            is_deleted: false,
        };
        let rows = vec![
            BudgetProgress {
                budget: budget("b1", 50_000, BudgetPeriod::Monthly),
                category_name: "餐饮".to_owned(),
                spent_cents: 12_345,
                over_budget: false,
            },
            BudgetProgress {
                budget: budget("b2", 1_000, BudgetPeriod::Yearly),
                category_name: "交通".to_owned(),
                spent_cents: 2_000,
                over_budget: true,
            },
        ];
        let categories =
            CategoryTable::from_categories(&[category("c1", "餐饮", "expense", None, 1)]);
        let cells = assemble_budget_rows(&rows, &categories, false);
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].id, "b1");
        assert_eq!(cells[0].category, "餐饮");
        assert_eq!(cells[0].period_label, "按月");
        assert_eq!(cells[0].amount, "500");
        assert_eq!(cells[0].spent, "123.45");
        assert_eq!(cells[0].status, "正常");
        assert!(!cells[0].over_budget);
        assert_eq!(cells[0].amount_cents, 50_000, "编辑回填锚点（整数分）");
        assert_eq!(cells[0].period, BudgetPeriod::Monthly);
        assert_eq!(cells[1].amount, "10");
        assert_eq!(cells[1].spent, "20");
        assert_eq!(cells[1].status, "超支");
        assert!(cells[1].over_budget);
        assert_eq!(cells[1].period_label, "按年");
        // 隐私态：金额文本遮蔽，口径字段照常。
        let masked = assemble_budget_rows(&rows, &categories, true);
        assert_eq!(masked[0].amount, "••••");
        assert_eq!(masked[0].spent, "••••");
        assert_eq!(masked[0].amount_cents, 50_000);
        // 孤儿分类回退：路径解析不到落「未分类」。
        let orphan = assemble_budget_rows(
            &[BudgetProgress {
                budget: budget("b3", 5_000, BudgetPeriod::Monthly),
                category_name: "未分类".to_owned(),
                spent_cents: 0,
                over_budget: false,
            }],
            &CategoryTable::default(),
            false,
        );
        assert_eq!(orphan[0].category, "未分类");
    }

    #[test]
    fn budget_summary_assembles_into_display_cells() {
        use ledger_budget::BudgetAnnualSummary;
        let summary = BudgetAnnualSummary {
            annual_budget_total_cents: 600_000,
            spent_cents: 700_000,
            over_budget: true,
        };
        let cells = assemble_budget_summary(&summary, false);
        assert_eq!(cells.annual_total, "6000");
        assert_eq!(cells.spent, "7000");
        assert_eq!(cells.status, "已超标");
        assert!(cells.over_budget);
        assert_eq!(cells.percent, 100.0);
        let normal = assemble_budget_summary(
            &BudgetAnnualSummary {
                annual_budget_total_cents: 600_000,
                spent_cents: 12345,
                over_budget: false,
            },
            true,
        );
        assert_eq!(normal.annual_total, "••••");
        assert_eq!(normal.status, "正常");
        assert_eq!(
            normal.percent, 2.0,
            "比例不遮蔽（12345/600000 → 2%，四舍五入）"
        );
    }

    // -----------------------------------------------------------------------
    // 交易表单族（#2034）：金额文本判定 / 换算（旧壳 field-error + money 家族
    // 的重生）、分类选项 kind 泛化（收入表单消费面）、基础收支装配器（旧壳
    // transaction-input.ts 的 TransactionInput 装配器重生）。
    // -----------------------------------------------------------------------

    #[test]
    fn amount_text_judgment_mirrors_old_shell_field_error() {
        use AmountJudgment::*;
        assert_eq!(judge_amount_text(""), Empty, "空串必填为空");
        assert_eq!(judge_amount_text("   "), Empty, "纯空白必填为空");
        assert_eq!(
            judge_amount_text("abc"),
            ParseError,
            "形状非法（旧壳 parse-error）"
        );
        assert_eq!(
            judge_amount_text("1.2.3"),
            ParseError,
            "双小数点非法（旧壳 parse-error）"
        );
        assert_eq!(
            judge_amount_text("1e3"),
            ParseError,
            "科学计数拒绝（旧壳 parse-error）"
        );
        assert_eq!(
            judge_amount_text("12.345"),
            OverPrecision,
            "超两位小数（旧壳 over-precision）"
        );
        assert_eq!(
            judge_amount_text("25"),
            Ok(25.0),
            "整数合法（携带元的数值）"
        );
        assert_eq!(judge_amount_text("12.34"), Ok(12.34));
        assert_eq!(
            judge_amount_text("12."),
            Ok(12.0),
            "尾随小数点合法（判定口径）"
        );
        assert_eq!(judge_amount_text(".5"), Ok(0.5), "省略整数部分合法");
        assert_eq!(
            judge_amount_text("-3"),
            Ok(-3.0),
            "负数可解析（业务类校验留提交通道，旧壳同口径）"
        );
        assert_eq!(
            judge_amount_text(" 80 "),
            Ok(80.0),
            "两端空白容差（trim 后判定）"
        );
    }

    #[test]
    fn amount_text_to_cents_is_the_single_conversion_point() {
        assert_eq!(amount_text_to_cents("80"), Some(8_000));
        assert_eq!(amount_text_to_cents("80.1"), Some(8_010));
        assert_eq!(amount_text_to_cents("0.05"), Some(5));
        assert_eq!(
            amount_text_to_cents("12.345"),
            Some(1_235),
            "判定拦截超精度在前；换算自身四舍五入到分（旧壳 yuanToCents 同口径）"
        );
        assert_eq!(amount_text_to_cents("-3"), Some(-300), "负数可解析");
        assert_eq!(amount_text_to_cents(""), None, "空串拒绝");
        assert_eq!(amount_text_to_cents("abc"), None, "非数拒绝");
        assert_eq!(
            amount_text_to_cents("12."),
            None,
            "尾随小数点在换算口径拒绝（旧壳 yuanToCents 同口径；判定放行、换算拦回，与装配器 fail fast 语义一致）"
        );
        assert_eq!(
            amount_text_to_cents("1e3"),
            None,
            "科学计数拒绝（旧壳同口径）"
        );
    }

    #[test]
    fn cents_to_amount_text_backfills_with_currency_decimals() {
        assert_eq!(
            cents_to_amount_text(2_500, 2),
            "25",
            "整数裁零（旧壳 String(centsToYuan) 同形）"
        );
        assert_eq!(cents_to_amount_text(12_345, 2), "123.45");
        assert_eq!(cents_to_amount_text(0, 2), "0");
        assert_eq!(cents_to_amount_text(255, 1), "25.5", "一位小数币种");
        assert_eq!(cents_to_amount_text(2_500, 0), "2500", "零小数位币种");
    }

    #[test]
    fn category_kind_options_filters_by_income_kind_with_path_labels() {
        let rows = vec![
            category("c1", "餐饮", "expense", None, 2),
            category("c2", "工资", "income", None, 1),
            category("c3", "奖金", "income", None, 2),
            category("c4", "年终奖", "income", Some("c3".into()), 1),
        ];
        let options = category_kind_options("income", &rows);
        let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["工资", "奖金", "奖金 > 年终奖"],
            "只收收入分类；父按 sort_order、子紧跟父且自身按 sort_order；子分类路径名"
        );
        // 泛化不改变支出面：同函数收支出分类与既有 expense_category_options 同序。
        let expense = category_kind_options("expense", &rows);
        assert_eq!(
            expense.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(),
            vec!["餐饮"],
        );
    }

    fn submit() -> BasicFormSubmit {
        BasicFormSubmit {
            kind: TransactionKind::Expense,
            amount_text: "25".to_owned(),
            account_id: Some("a1".to_owned()),
            category_id: Some("c1".to_owned()),
            merchant: MerchantRef::None,
            policy_id: None,
            note: String::new(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 3, 15).unwrap(),
            source_order_no: None,
        }
    }

    #[test]
    fn basic_assembler_fills_per_kind_matrix_nulls_and_wire_fields() {
        let input = assemble_basic_transaction(submit(), "CNY".to_owned()).unwrap();
        assert_eq!(input.kind, TransactionKind::Expense);
        assert_eq!(
            input.amount_cents, 2_500,
            "元转分收口（amount_text_to_cents 单点）"
        );
        assert_eq!(input.currency_code, "CNY");
        assert_eq!(input.account_id.as_deref(), Some("a1"));
        assert_eq!(input.category_id.as_deref(), Some("c1"));
        assert_eq!(
            input.date, "2026-03-15",
            "本地日历日语义（NaiveDate 原样落 ISO）"
        );
        assert_eq!(
            input.note, None,
            "空备注落 None（旧壳 note || null 同口径）"
        );
        assert_eq!(input.source_order_no, None, "创建路径不携带来源订单号");
        // per-kind 矩阵：基础收支不携带的字段全空（旧壳 KIND_FIELD_MATRIX 行同形）。
        assert_eq!(input.to_account_id, None);
        assert_eq!(input.funding_account_id, None);
        assert!(input.funding.is_empty());
        assert!(input.purchases.is_empty());
        assert_eq!(input.refund_of_transaction_id, None);
        assert_eq!(input.instrument_id, None);
        assert_eq!(
            input.idempotency_key, None,
            "表单不携带幂等键（编辑保留行上原值）"
        );
    }

    #[test]
    fn basic_assembler_carries_optional_relations() {
        let mut s = submit();
        s.note = "午餐".to_owned();
        s.policy_id = Some("p1".to_owned());
        s.source_order_no = Some("SO-1".to_owned());
        let input = assemble_basic_transaction(s, "CNY".to_owned()).unwrap();
        assert_eq!(input.note.as_deref(), Some("午餐"));
        assert_eq!(input.policy_id.as_deref(), Some("p1"));
        assert_eq!(
            input.source_order_no.as_deref(),
            Some("SO-1"),
            "编辑透传行上原值"
        );
    }

    #[test]
    fn basic_assembler_maps_merchant_semantics_to_wire_fields() {
        // 不挂商户。
        let input = assemble_basic_transaction(submit(), "CNY".to_owned()).unwrap();
        assert_eq!(input.merchant_id, None);
        assert_eq!(input.merchant_name, None);

        // 编辑面原样保留（含软删商户历史引用；后端 unchanged 语义跳过在用校验）。
        let mut s = submit();
        s.merchant = MerchantRef::Keep("m-kept".to_owned());
        let input = assemble_basic_transaction(s, "CNY".to_owned()).unwrap();
        assert_eq!(input.merchant_id.as_deref(), Some("m-kept"));
        assert_eq!(input.merchant_name, None);

        // 选定在用商户（id 提交）。
        let mut s = submit();
        s.merchant = MerchantRef::Selected("m1".to_owned());
        let input = assemble_basic_transaction(s, "CNY".to_owned()).unwrap();
        assert_eq!(input.merchant_id.as_deref(), Some("m1"));

        // 自由文本：merchant_name 落 wire（后端精确匹配在用名复用、未命中即建——
        // 域内单点，壳不另立第二份解析）。
        let mut s = submit();
        s.merchant = MerchantRef::NewName("新商户".to_owned());
        let input = assemble_basic_transaction(s, "CNY".to_owned()).unwrap();
        assert_eq!(input.merchant_id, None);
        assert_eq!(input.merchant_name.as_deref(), Some("新商户"));
    }

    #[test]
    fn basic_assembler_rejects_invalid_submissions_fail_fast() {
        // 账户必选（旧壳 settings.categories.msg.accountRequired 词表）。
        let mut s = submit();
        s.account_id = None;
        assert_eq!(
            assemble_basic_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(BasicFormError::AccountRequired),
        );
        // 金额非正数（旧壳 amountRequired「请输入金额」词表）。
        let mut s = submit();
        s.amount_text = "-3".to_owned();
        assert_eq!(
            assemble_basic_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(BasicFormError::AmountNotPositive),
        );
        let mut s = submit();
        s.amount_text = "0".to_owned();
        assert_eq!(
            assemble_basic_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(BasicFormError::AmountNotPositive),
        );
        // 换算兜底（"12." 判定放行、换算拒绝——旧壳装配器 fail fast 同位）。
        let mut s = submit();
        s.amount_text = "12.".to_owned();
        assert_eq!(
            assemble_basic_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(BasicFormError::AmountInvalid),
        );
        // 空金额（按钮禁用是第一道防线；装配器兜底同位拦截）。
        let mut s = submit();
        s.amount_text = "  ".to_owned();
        assert_eq!(
            assemble_basic_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(BasicFormError::AmountInvalid),
        );
    }

    #[test]
    fn basic_form_error_texts_are_the_zh_cn_vocabulary() {
        assert_eq!(BasicFormError::AccountRequired.as_text(), "请选择账户");
        assert_eq!(BasicFormError::AmountNotPositive.as_text(), "请输入金额");
        assert_eq!(
            BasicFormError::AmountInvalid.as_text(),
            "金额无效，请输入不超过两位小数的金额",
            "码化面归视图词表：换算拒绝时给可纠正文案"
        );
    }

    // -------------------------------------------------------------------
    // 转账 / 借贷表单装配器（#2035）
    // -------------------------------------------------------------------

    fn transfer_submit() -> TransferFormSubmit {
        TransferFormSubmit {
            amount_text: "3000".to_owned(),
            account_id: Some("a-cash".to_owned()),
            to_account_id: Some("a-bank".to_owned()),
            merchant: MerchantRef::None,
            note: String::new(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            source_order_no: None,
        }
    }

    #[test]
    fn transfer_assembler_builds_dual_account_wire() {
        let input = assemble_transfer_transaction(transfer_submit(), "CNY".to_owned()).unwrap();
        assert_eq!(input.kind, TransactionKind::Transfer);
        assert_eq!(input.amount_cents, 300_000, "元转分收口（整数分落库）");
        assert_eq!(input.account_id.as_deref(), Some("a-cash"), "转出侧落位");
        assert_eq!(input.to_account_id.as_deref(), Some("a-bank"), "转入侧落位");
        assert_eq!(input.category_id, None, "转账不携带分类（编排入口拒绝）");
        assert_eq!(input.policy_id, None, "转账不挂保单");
        assert_eq!(input.refund_of_transaction_id, None);
        assert!(input.funding.is_empty() && input.purchases.is_empty());
        assert_eq!(input.date, "2026-03-01", "本地日历日 → ISO 文本");
        assert_eq!(input.note, None, "空备注落 None");
    }

    #[test]
    fn transfer_assembler_carries_merchant_and_note_and_order_no() {
        let mut s = transfer_submit();
        s.merchant = MerchantRef::Selected("m1".to_owned());
        s.note = "过桥".to_owned();
        s.source_order_no = Some("JD-1".to_owned());
        let input = assemble_transfer_transaction(s, "CNY".to_owned()).unwrap();
        assert_eq!(input.merchant_id.as_deref(), Some("m1"));
        assert_eq!(input.merchant_name, None);
        assert_eq!(input.note.as_deref(), Some("过桥"));
        assert_eq!(input.source_order_no.as_deref(), Some("JD-1"));

        // 借贷变体自由文本即建：merchant_name 落 wire（域内单点解析）。
        let mut s = transfer_submit();
        s.merchant = MerchantRef::NewName("张三".to_owned());
        let input = assemble_transfer_transaction(s, "CNY".to_owned()).unwrap();
        assert_eq!(input.merchant_id, None);
        assert_eq!(input.merchant_name.as_deref(), Some("张三"));
    }

    #[test]
    fn transfer_assembler_fails_fast_in_old_shell_guard_order() {
        // 校验序镜像旧壳 submit：转出 → 转入 → 同账户 → 金额换算 → 非正。
        let mut s = transfer_submit();
        s.account_id = None;
        assert_eq!(
            assemble_transfer_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(TransferFormError::FromAccountRequired),
            "转出账户缺失先于一切金额校验"
        );
        let mut s = transfer_submit();
        s.to_account_id = None;
        assert_eq!(
            assemble_transfer_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(TransferFormError::ToAccountRequired)
        );
        let mut s = transfer_submit();
        s.to_account_id = s.account_id.clone();
        assert_eq!(
            assemble_transfer_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(TransferFormError::SameAccount)
        );
        let mut s = transfer_submit();
        s.amount_text = "abc".to_owned();
        assert_eq!(
            assemble_transfer_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(TransferFormError::AmountInvalid)
        );
        for bad in ["0", "-3"] {
            let mut s = transfer_submit();
            s.amount_text = bad.to_owned();
            assert_eq!(
                assemble_transfer_transaction(s, "CNY".to_owned()).map(|_| ()),
                Err(TransferFormError::AmountNotPositive),
                "纯零/负数保留提交通道文案（ADR-0058 业务不成立不属字段错误态）"
            );
        }
    }

    #[test]
    fn transfer_error_texts_are_the_zh_cn_vocabulary() {
        assert_eq!(
            TransferFormError::FromAccountRequired.as_text(),
            "请选择转出账户"
        );
        assert_eq!(
            TransferFormError::ToAccountRequired.as_text(),
            "请选择转入账户"
        );
        assert_eq!(
            TransferFormError::SameAccount.as_text(),
            "转出账户和转入账户不能相同"
        );
        assert_eq!(TransferFormError::AmountNotPositive.as_text(), "请输入金额");
        assert_eq!(
            TransferFormError::AmountInvalid.as_text(),
            "金额无效，请输入不超过两位小数的金额"
        );
    }

    // -------------------------------------------------------------------
    // 退款表单装配器（#2035）
    // -------------------------------------------------------------------

    fn refund_submit() -> RefundFormSubmit {
        RefundFormSubmit {
            amount_text: "25".to_owned(),
            refund_of_transaction_id: Some("t-orig".to_owned()),
            account_id: Some("a-cash".to_owned()),
            note: String::new(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(),
            source_order_no: None,
        }
    }

    #[test]
    fn refund_assembler_builds_linked_refund_wire() {
        let input = assemble_refund_transaction(refund_submit(), "CNY".to_owned()).unwrap();
        assert_eq!(input.kind, TransactionKind::Refund);
        assert_eq!(input.amount_cents, 2_500);
        assert_eq!(
            input.refund_of_transaction_id.as_deref(),
            Some("t-orig"),
            "原支出引用落 wire（继承与钳制语义以域为唯一真源）"
        );
        assert_eq!(
            input.account_id.as_deref(),
            Some("a-cash"),
            "原支出账户随行带出"
        );
        assert_eq!(
            input.category_id, None,
            "分类/商户/保单全部继承原支出，表单不携带"
        );
        assert_eq!(input.merchant_id, None);
        assert_eq!(input.merchant_name, None);
        assert_eq!(input.policy_id, None, "退款不挂保单（ADR-0051 决策 4）");
        assert!(input.funding.is_empty() && input.purchases.is_empty());
        assert_eq!(input.date, "2026-04-05");
    }

    #[test]
    fn refund_assembler_requires_target_and_positive_amount() {
        let mut s = refund_submit();
        s.refund_of_transaction_id = None;
        assert_eq!(
            assemble_refund_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(RefundFormError::TargetRequired),
            "目标缺失先于金额校验（旧壳 submit 守卫序同形）"
        );
        let mut s = refund_submit();
        s.amount_text = "-1".to_owned();
        assert_eq!(
            assemble_refund_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(RefundFormError::AmountNotPositive)
        );
        let mut s = refund_submit();
        s.amount_text = "1.2.3".to_owned();
        assert_eq!(
            assemble_refund_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(RefundFormError::AmountInvalid)
        );
    }

    #[test]
    fn refund_error_texts_are_the_zh_cn_vocabulary() {
        assert_eq!(
            RefundFormError::TargetRequired.as_text(),
            "请选择要退款的原始支出交易"
        );
        assert_eq!(
            RefundFormError::AmountNotPositive.as_text(),
            "请输入退款金额"
        );
        assert_eq!(
            RefundFormError::AmountInvalid.as_text(),
            "金额无效，请输入不超过两位小数的金额"
        );
    }

    // -------------------------------------------------------------------
    // 出资分解 / 购买项表单装配器（#2036 表单③）
    // -------------------------------------------------------------------

    /// 出资分解提交载荷（合法基线：两条出资、一条叠加购买项、挂分类与商户）。
    fn funding_submit() -> FundingFormSubmit {
        FundingFormSubmit {
            kind: TransactionKind::Expense,
            amount_text: "50".to_owned(),
            rows: vec![
                FundingRowSubmit {
                    account_id: Some("a-cash".to_owned()),
                    amount_text: "20".to_owned(),
                    label: String::new(),
                },
                FundingRowSubmit {
                    account_id: Some("a-credit".to_owned()),
                    amount_text: "30".to_owned(),
                    label: "尾款".to_owned(),
                },
            ],
            purchases: vec![PurchaseRowSubmit {
                name: "洗衣液".to_owned(),
                quantity_text: "2".to_owned(),
                category_id: None,
                price_text: String::new(),
            }],
            category_id: Some("cat-daily".to_owned()),
            merchant: MerchantRef::NewName("超市".to_owned()),
            policy_id: None,
            note: "组合支付".to_owned(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(),
            source_order_no: None,
        }
    }

    #[test]
    fn funding_assembler_builds_split_wire_with_matching_total() {
        let input = assemble_funding_transaction(funding_submit(), "CNY".to_owned()).unwrap();
        assert_eq!(input.kind, TransactionKind::Expense);
        assert_eq!(input.amount_cents, 5_000, "主行金额元转分收口");
        assert_eq!(
            input.account_id, None,
            "分解行主账户恒缺省（契约互斥，ADR-0138 决策 2）"
        );
        assert_eq!(
            input.funding,
            vec![
                ledger_transaction::TransactionFundingInput {
                    account_id: "a-cash".to_owned(),
                    amount_cents: 2_000,
                    label: None,
                },
                ledger_transaction::TransactionFundingInput {
                    account_id: "a-credit".to_owned(),
                    amount_cents: 3_000,
                    label: Some("尾款".to_owned()),
                },
            ],
            "出资项逐条落 wire（整数分、空标签落 None、数组序即顺序位）"
        );
        assert_eq!(
            input.purchases,
            vec![ledger_transaction::TransactionPurchaseInput {
                name: "洗衣液".to_owned(),
                quantity: 2,
                category_id: None,
                unit_price_cents: None,
            }],
            "叠加购买项随分解行同单提交（分解行可携带购买项，ADR-0138 决策 15 呈现形态）"
        );
        assert_eq!(input.category_id.as_deref(), Some("cat-daily"));
        assert_eq!(input.merchant_name.as_deref(), Some("超市"));
        assert_eq!(input.note.as_deref(), Some("组合支付"));
        assert_eq!(input.date, "2026-04-05");
    }

    #[test]
    fn funding_assembler_fails_fast_on_amount_rows_and_total() {
        // 主金额形状 → 空分解 → 逐行守卫 → 合计一致，fail fast 有序。
        let mut s = funding_submit();
        s.amount_text = "1.2.3".to_owned();
        assert_eq!(
            assemble_funding_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(FundingFormError::AmountInvalid)
        );
        let mut s = funding_submit();
        s.amount_text = "0".to_owned();
        assert_eq!(
            assemble_funding_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(FundingFormError::AmountNotPositive)
        );
        let mut s = funding_submit();
        s.rows.clear();
        assert_eq!(
            assemble_funding_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(FundingFormError::RowsEmpty),
            "本族无主账户位，空分解不可提交（域契约 funding 空 ⇒ account_id 必填）"
        );
        let mut s = funding_submit();
        s.rows[0].account_id = None;
        assert_eq!(
            assemble_funding_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(FundingFormError::RowAccountRequired)
        );
        let mut s = funding_submit();
        s.rows[0].amount_text = "abc".to_owned();
        assert_eq!(
            assemble_funding_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(FundingFormError::RowAmountInvalid)
        );
        let mut s = funding_submit();
        s.rows[0].amount_text = "-1".to_owned();
        assert_eq!(
            assemble_funding_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(FundingFormError::RowAmountNotPositive)
        );
        let mut s = funding_submit();
        s.rows[1].label = "x".repeat(51);
        assert_eq!(
            assemble_funding_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(FundingFormError::LabelTooLong),
            "扣款标签 ≤50 字（域 LABEL_MAX_CHARS 同口径，壳层先行）"
        );
        let mut s = funding_submit();
        s.rows[1].amount_text = "35".to_owned();
        assert_eq!(
            assemble_funding_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(FundingFormError::TotalMismatch),
            "Σ 出资项 ≠ 交易金额阻拦提交（用户故事 2：明确阻拦不错记）"
        );
    }

    #[test]
    fn funding_assembler_rejects_purchase_row_errors_via_shared_vocabulary() {
        let mut s = funding_submit();
        s.purchases[0].name = "  ".to_owned();
        assert_eq!(
            assemble_funding_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(FundingFormError::Row(PurchaseRowError::NameRequired))
        );
    }

    #[test]
    fn funding_error_texts_are_the_zh_cn_vocabulary() {
        assert_eq!(FundingFormError::AmountNotPositive.as_text(), "请输入金额");
        assert_eq!(
            FundingFormError::AmountInvalid.as_text(),
            "金额无效，请输入不超过两位小数的金额"
        );
        assert_eq!(
            FundingFormError::RowsEmpty.as_text(),
            "请至少添加一条出资项"
        );
        assert_eq!(
            FundingFormError::RowAccountRequired.as_text(),
            "请选择出资账户"
        );
        assert_eq!(
            FundingFormError::RowAmountNotPositive.as_text(),
            "请输入出资金额"
        );
        assert_eq!(
            FundingFormError::RowAmountInvalid.as_text(),
            "出资金额无效，请输入不超过两位小数的金额"
        );
        assert_eq!(
            FundingFormError::LabelTooLong.as_text(),
            "扣款标签不能超过 50 字"
        );
        assert_eq!(
            FundingFormError::TotalMismatch.as_text(),
            "出资合计与交易金额不一致"
        );
    }

    /// 购买项下单提交载荷（合法基线：两行商品、主行挂分类/商户/保单）。
    fn purchase_submit() -> PurchaseFormSubmit {
        PurchaseFormSubmit {
            amount_text: "100".to_owned(),
            account_id: Some("a-cash".to_owned()),
            rows: vec![
                PurchaseRowSubmit {
                    name: "洗衣液".to_owned(),
                    quantity_text: "2".to_owned(),
                    category_id: Some("cat-daily".to_owned()),
                    price_text: "39.9".to_owned(),
                },
                PurchaseRowSubmit {
                    name: "纸巾".to_owned(),
                    quantity_text: "1".to_owned(),
                    category_id: None,
                    price_text: String::new(),
                },
            ],
            category_id: Some("cat-daily".to_owned()),
            merchant: MerchantRef::Selected("m-shop".to_owned()),
            policy_id: None,
            note: "网购订单".to_owned(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(),
            source_order_no: None,
        }
    }

    #[test]
    fn purchase_assembler_builds_order_wire() {
        let input = assemble_purchase_transaction(purchase_submit(), "CNY".to_owned()).unwrap();
        assert_eq!(
            input.kind,
            TransactionKind::Expense,
            "仅 expense 可带购买项"
        );
        assert_eq!(input.amount_cents, 10_000, "订单实付元转分收口");
        assert_eq!(
            input.account_id.as_deref(),
            Some("a-cash"),
            "订单有单一付款账户（与分解族互斥形态）"
        );
        assert_eq!(
            input.purchases,
            vec![
                ledger_transaction::TransactionPurchaseInput {
                    name: "洗衣液".to_owned(),
                    quantity: 2,
                    category_id: Some("cat-daily".to_owned()),
                    unit_price_cents: Some(3_990),
                },
                ledger_transaction::TransactionPurchaseInput {
                    name: "纸巾".to_owned(),
                    quantity: 1,
                    category_id: None,
                    unit_price_cents: None,
                },
            ],
            "商品行逐条落 wire（单价可空、整数分、数组序即对账单顺序）"
        );
        assert!(input.funding.is_empty());
        assert_eq!(input.merchant_id.as_deref(), Some("m-shop"));
        assert_eq!(input.note.as_deref(), Some("网购订单"));
    }

    #[test]
    fn purchase_assembler_allows_empty_rows_and_fails_fast_on_guards() {
        // 零商品行合法（= 无明细的普通支出，增行是可选增强不强制）。
        let mut s = purchase_submit();
        s.rows.clear();
        let input = assemble_purchase_transaction(s, "CNY".to_owned()).unwrap();
        assert!(input.purchases.is_empty());

        let mut s = purchase_submit();
        s.account_id = None;
        assert_eq!(
            assemble_purchase_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(PurchaseFormError::AccountRequired)
        );
        let mut s = purchase_submit();
        s.amount_text = "0".to_owned();
        assert_eq!(
            assemble_purchase_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(PurchaseFormError::AmountNotPositive)
        );
        let mut s = purchase_submit();
        s.rows[0].name = String::new();
        assert_eq!(
            assemble_purchase_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(PurchaseFormError::Row(PurchaseRowError::NameRequired))
        );
        let mut s = purchase_submit();
        s.rows[0].quantity_text = "0".to_owned();
        assert_eq!(
            assemble_purchase_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(PurchaseFormError::Row(PurchaseRowError::QuantityInvalid)),
            "件数为正整数（域契约 quantity > 0，壳层先行）"
        );
        let mut s = purchase_submit();
        s.rows[0].price_text = "-5".to_owned();
        assert_eq!(
            assemble_purchase_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(PurchaseFormError::Row(PurchaseRowError::PriceNegative)),
            "单价非负（0 是合法标价——赠品）"
        );
        let mut s = purchase_submit();
        s.rows[0].price_text = "1.234".to_owned();
        assert_eq!(
            assemble_purchase_transaction(s, "CNY".to_owned()).map(|_| ()),
            Err(PurchaseFormError::Row(PurchaseRowError::PriceInvalid))
        );
    }

    #[test]
    fn purchase_error_texts_are_the_zh_cn_vocabulary() {
        assert_eq!(PurchaseFormError::AccountRequired.as_text(), "请选择账户");
        assert_eq!(PurchaseFormError::AmountNotPositive.as_text(), "请输入金额");
        assert_eq!(
            PurchaseFormError::AmountInvalid.as_text(),
            "金额无效，请输入不超过两位小数的金额"
        );
        assert_eq!(
            PurchaseFormError::Row(PurchaseRowError::NameRequired).as_text(),
            "请输入商品名称"
        );
        assert_eq!(
            PurchaseFormError::Row(PurchaseRowError::QuantityInvalid).as_text(),
            "件数必须为正整数"
        );
        assert_eq!(
            PurchaseFormError::Row(PurchaseRowError::PriceInvalid).as_text(),
            "商品单价无效，请输入不超过两位小数的金额"
        );
        assert_eq!(
            PurchaseFormError::Row(PurchaseRowError::PriceNegative).as_text(),
            "商品单价不能为负数"
        );
    }

    // -------------------------------------------------------------------
    // 借贷方向族（#2035）：侧别表 / 标签 / 方向切换处置——与方向派生
    // [`derive_lending_direction`] 同源共表。
    // -------------------------------------------------------------------

    #[test]
    fn lending_direction_sides_follow_the_domain_table() {
        assert_eq!(
            lending_direction_sides(LendingFormDirection::Lend),
            (LendingAccountSide::Fund, LendingAccountSide::Receivable)
        );
        assert_eq!(
            lending_direction_sides(LendingFormDirection::Collect),
            (LendingAccountSide::Receivable, LendingAccountSide::Fund)
        );
        assert_eq!(
            lending_direction_sides(LendingFormDirection::Borrow),
            (LendingAccountSide::Debt, LendingAccountSide::Fund)
        );
        assert_eq!(
            lending_direction_sides(LendingFormDirection::Repay),
            (LendingAccountSide::Fund, LendingAccountSide::Debt)
        );
    }

    #[test]
    fn account_matches_side_excludes_unknown_types() {
        assert!(account_matches_side(
            Some(AccountType::Cash),
            LendingAccountSide::Fund
        ));
        assert!(account_matches_side(
            Some(AccountType::Bank),
            LendingAccountSide::Fund
        ));
        assert!(account_matches_side(
            Some(AccountType::Receivable),
            LendingAccountSide::Receivable
        ));
        assert!(account_matches_side(
            Some(AccountType::Debt),
            LendingAccountSide::Debt
        ));
        assert!(!account_matches_side(
            Some(AccountType::Cash),
            LendingAccountSide::Receivable
        ));
        assert!(!account_matches_side(
            Some(AccountType::Credit),
            LendingAccountSide::Debt
        ));
        // 账户类型缺失（黑洞/已删占位）不命中任何侧（旧壳 accountMatchesSide 同语义）。
        assert!(!account_matches_side(None, LendingAccountSide::Fund));
        assert!(!account_matches_side(None, LendingAccountSide::Receivable));
    }

    #[test]
    fn lending_direction_labels_share_one_vocabulary_with_kind_label() {
        for (direction, lending) in [
            (LendingFormDirection::Lend, LendingDirection::Lend),
            (LendingFormDirection::Collect, LendingDirection::Collect),
            (LendingFormDirection::Borrow, LendingDirection::Borrow),
            (LendingFormDirection::Repay, LendingDirection::Repay),
        ] {
            assert_eq!(
                lending_direction_label(direction),
                kind_label(TransactionKind::Transfer, lending),
                "表单方向标签与列表类型标签同词表：{}",
                lending_direction_label(direction)
            );
        }
    }

    #[test]
    fn direction_switch_swaps_reverse_pair_and_clears_out_of_side() {
        let types = |id: &str| match id {
            "cash" => Some(AccountType::Cash),
            "recv" => Some(AccountType::Receivable),
            "debt" => Some(AccountType::Debt),
            _ => None,
        };
        // 反向方向（借出 ↔ 收回）：两端贴合新方向 → 交换两端，不丢已选账户
        // （旧壳 setDirection 同语义）。
        assert_eq!(
            lending_direction_swap(
                LendingFormDirection::Lend,
                LendingFormDirection::Collect,
                Some("cash"),
                Some("recv"),
                types,
            ),
            (Some("recv".to_owned()), Some("cash".to_owned())),
        );
        // 其余情况：保留仍属允许侧别的选择、清掉越侧的。
        assert_eq!(
            lending_direction_swap(
                LendingFormDirection::Lend,
                LendingFormDirection::Repay,
                Some("cash"),
                Some("recv"),
                types,
            ),
            (Some("cash".to_owned()), None),
            "转出侧贴合新方向保留，转入侧越侧清除"
        );
        assert_eq!(
            lending_direction_swap(
                LendingFormDirection::Lend,
                LendingFormDirection::Borrow,
                Some("cash"),
                Some("recv"),
                types,
            ),
            (None, None),
            "两端都越侧 → 双清"
        );
        // 账户类型缺失不命中任何侧 → 清除（防御：不残留越侧选择；缺失账户
        // 永不贴合反向侧，不会触发交换）。
        assert_eq!(
            lending_direction_swap(
                LendingFormDirection::Lend,
                LendingFormDirection::Repay,
                Some("cash"),
                Some("ghost"),
                types,
            ),
            (Some("cash".to_owned()), None),
        );
        // 同向切换是调用方守卫的 no-op（原样返回，不重置已选）。
        assert_eq!(
            lending_direction_swap(
                LendingFormDirection::Lend,
                LendingFormDirection::Lend,
                Some("cash"),
                Some("recv"),
                types,
            ),
            (Some("cash".to_owned()), Some("recv".to_owned())),
        );
    }

    // -----------------------------------------------------------------------
    // 概览视图四卡格（#2039）：净资产 / 本月收支 / 投资三卡 / 财务自由度——
    // 旧壳 DashboardView 装配层的重生（一次装配、渲染期零格式化）；金额换算
    // 与折算口径以域为唯一真源，本层只消费域行集产展示文本。
    // -----------------------------------------------------------------------

    fn cny_table() -> CurrencyTable {
        CurrencyTable::from_currencies(std::iter::once(&currency("CNY", "¥", 2)))
    }

    fn currency(code: &str, symbol: &str, places: i64) -> Currency {
        Currency {
            code: code.to_owned(),
            symbol: symbol.to_owned(),
            decimal_places: places,
            name: code.to_owned(),
        }
    }

    #[test]
    fn net_worth_card_assembles_native_main_number_with_symbol_and_privacy() {
        let overview = ledger_dashboard::DashboardOverview {
            native_currency: "CNY".to_owned(),
            net_worth_cents: 1_234_567_890,
            accounts_balance_cents: 0,
            holdings_market_value_cents: 0,
            physical_assets_value_cents: 0,
        };
        let cells = assemble_net_worth(&overview, &cny_table(), false);
        assert_eq!(
            cells.net_worth, "¥1234,5678.9",
            "本位币主数字带币符、zh 四位一组"
        );
        let masked = assemble_net_worth(&overview, &cny_table(), true);
        assert_eq!(masked.net_worth, "••••", "隐私态同表格掩码");
        // 基准币种未登记（字典缺行）→ 缺省格式（无符号两位小数），不静默换符号。
        let unknown = assemble_net_worth(&overview, &CurrencyTable::default(), false);
        assert_eq!(unknown.net_worth, "1234,5678.9");
    }

    #[test]
    fn monthly_flow_card_derives_net_expense_and_balance_with_refund() {
        let month = ledger_reports::MonthlySummary {
            month: "2026-03".to_owned(),
            income_cents: 50_000,
            expense_cents: 30_000,
            refund_cents: 5_000,
        };
        let cells = assemble_monthly_flow(Some(&month), false);
        assert_eq!(cells.income, "500", "收入直用后端净收入列");
        assert_eq!(
            cells.net_expense, "250",
            "净支出 = 毛支出 − 退款（退款不单列）"
        );
        assert_eq!(cells.balance, "250", "结余 = 收入 − 净支出");
        // 当月无行 → 三零（旧壳 ?? 0 同口径）。
        let empty = assemble_monthly_flow(None, false);
        assert_eq!(
            (
                empty.income.as_str(),
                empty.net_expense.as_str(),
                empty.balance.as_str()
            ),
            ("0", "0", "0")
        );
        // 隐私态三格同掩码。
        let masked = assemble_monthly_flow(Some(&month), true);
        assert_eq!(masked.income, "••••");
        assert_eq!(masked.net_expense, "••••");
        assert_eq!(masked.balance, "••••");
    }

    fn holding(
        market: Option<i64>,
        native_market: Option<i64>,
        unreal: Option<i64>,
        native_unreal: Option<i64>,
    ) -> ledger_investment::Holding {
        ledger_investment::Holding {
            id: "h".to_owned(),
            account_id: "a".to_owned(),
            instrument_id: "i".to_owned(),
            quantity: 1.0,
            cost_basis_cents: 0,
            cost_currency_code: "CNY".to_owned(),
            latest_price_cents: None,
            latest_price_currency_code: None,
            latest_nav_date: None,
            market_value_cents: market,
            unrealized_pnl_cents: unreal,
            native_market_value_cents: native_market,
            native_unrealized_pnl_cents: native_unreal,
            updated_at: "2026-03-14T09:26:53Z".to_owned(),
        }
    }

    #[test]
    fn portfolio_stats_cards_sum_native_values_and_surface_missing_states() {
        let rows = vec![
            holding(Some(10_000), Some(10_000), Some(1_000), Some(1_000)),
            holding(Some(20_000), Some(20_000), Some(-2_000), Some(-2_000)),
        ];
        let cumulative = ledger_investment::CumulativePnlNativeTotal {
            total_cents: 3_000,
            native_currency: "CNY".to_owned(),
        };
        let cells = assemble_portfolio_stats(&rows, Some(&cumulative), None, &cny_table(), false);
        assert!(cells.has_holdings);
        assert_eq!(cells.market_value.text.as_deref(), Some("¥300"));
        assert_eq!(
            cells.unrealized_pnl.text.as_deref(),
            Some("-¥10"),
            "负号在最前、币符随后"
        );
        assert_eq!(
            cells.unrealized_pnl.pnl,
            Some(PnlSemantic::Loss),
            "跌色消费面"
        );
        assert_eq!(cells.market_value.pnl, None, "市值卡中性（涨跌色不外溢）");
        assert_eq!(cells.cumulative_pnl.text.as_deref(), Some("¥30"));
        assert_eq!(cells.cumulative_pnl.pnl, Some(PnlSemantic::Gain));
        assert!(cells.market_value.warning.is_none());

        // 缺料三态显式分离：缺现价行 → 计数小字；缺折算汇率行 → 整卡警告不给半截数字。
        let mixed = vec![
            holding(Some(10_000), Some(10_000), Some(1_000), Some(1_000)),
            holding(None, None, None, None),
            holding(Some(30_000), None, Some(3_000), None),
        ];
        let cells = assemble_portfolio_stats(&mixed, Some(&cumulative), None, &cny_table(), false);
        assert_eq!(
            cells.market_value.text, None,
            "缺折算汇率行在场即不给半截合计"
        );
        assert_eq!(
            cells.market_value.warning.as_deref(),
            Some("部分持仓缺折算汇率，合计暂不可计。")
        );
        assert!(
            cells.market_value.missing_price_note.is_none(),
            "警告态不重复交代缺价"
        );
        // 未实现列同样含缺折算汇率行 → 同样整卡警告、计数小字不重复交代。
        assert_eq!(
            cells.unrealized_pnl.warning.as_deref(),
            Some("部分持仓缺折算汇率，合计暂不可计。")
        );
        assert!(cells.unrealized_pnl.missing_price_note.is_none());

        // 只有缺现价（无缺汇率行）→ 计数小字随数值态呈现。
        let price_only = vec![
            holding(Some(10_000), Some(10_000), Some(1_000), Some(1_000)),
            holding(None, None, None, None),
        ];
        let cells =
            assemble_portfolio_stats(&price_only, Some(&cumulative), None, &cny_table(), false);
        assert_eq!(cells.market_value.text.as_deref(), Some("¥100"));
        assert!(cells.market_value.warning.is_none());
        assert_eq!(
            cells.market_value.missing_price_note.as_deref(),
            Some("1 只持仓因缺现价未计入。")
        );

        // 累计收益命令报错（如缺汇率码化上抛）→ 累计卡整卡警告 + 文案透传。
        let err = assemble_portfolio_stats(
            &rows,
            None,
            Some("汇率缺失".to_owned()),
            &cny_table(),
            false,
        );
        assert_eq!(err.cumulative_pnl.warning.as_deref(), Some("汇率缺失"));
        assert_eq!(err.cumulative_pnl.text, None);

        // 无持仓 → 空态消费面（「暂无持仓」），三卡不呈现。
        let empty = assemble_portfolio_stats(&[], Some(&cumulative), None, &cny_table(), false);
        assert!(!empty.has_holdings);
    }

    #[test]
    fn freedom_card_formats_ratio_stage_and_caliber_lines() {
        use ledger_investment::FinancialFreedomOverview;
        let freedom = FinancialFreedomOverview {
            ratio: 37.5,
            numerator_cents: 300_000,
            denominator_cents: 240_000,
            coverage_years: 12.5,
            native_currency: "CNY".to_owned(),
        };
        let cells = assemble_financial_freedom(&freedom, &cny_table(), false);
        assert_eq!(cells.ratio_text, "37.5%", "一位小数去尾零（JS 数字同形态）");
        assert_eq!(
            cells.stage,
            Some(FreedomStage::Approaching),
            "30–100 接近自由"
        );
        assert_eq!(cells.percent, 37.5);
        assert_eq!(cells.numerator_line, "可投资资产 ¥3000");
        assert_eq!(cells.denominator_line, "年度预算总额 ¥2400");
        assert_eq!(cells.coverage_line, "可覆盖 12.5 年");
        assert!(!cells.zero_denominator);

        // 阈值两端（<30 积累期 / ≥100 财务自由）与整数刻度去尾零。
        let low = FinancialFreedomOverview {
            ratio: 0.0,
            coverage_years: 0.0,
            numerator_cents: 0,
            ..freedom.clone()
        };
        assert_eq!(
            assemble_financial_freedom(&low, &cny_table(), false).stage,
            Some(FreedomStage::Accumulation)
        );
        let reached = FinancialFreedomOverview {
            ratio: 100.0,
            coverage_years: 12.0,
            ..freedom.clone()
        };
        let reached_cells = assemble_financial_freedom(&reached, &cny_table(), false);
        assert_eq!(reached_cells.stage, Some(FreedomStage::Free));
        assert_eq!(reached_cells.ratio_text, "100%", "整数刻度不拖尾零");
        assert_eq!(reached_cells.coverage_line, "可覆盖 12 年");

        // 零分母（未设预算）：占位态，阶段与口径行不呈现（不回退实际支出）。
        let zero = FinancialFreedomOverview {
            ratio: 0.0,
            denominator_cents: 0,
            ..freedom.clone()
        };
        let zero_cells = assemble_financial_freedom(&zero, &cny_table(), false);
        assert!(zero_cells.zero_denominator);
        assert_eq!(zero_cells.stage, None);

        // 金额行隐私掩码；比例与年数不遮蔽（非金额量）。
        let masked = assemble_financial_freedom(&freedom, &cny_table(), true);
        assert_eq!(masked.numerator_line, "可投资资产 ••••");
        assert_eq!(masked.ratio_text, "37.5%");
    }
    // =====================================================================
    // 账户视图装配族（#2041）：清单格文本与余额口径（V017 缓存读数只消费）。
    // =====================================================================

    /// 账户行快照（用例共用）：名称 / 类型 / 币种 / 余额 / 归档态随参。
    fn account_row(
        id: &'static str,
        name: &'static str,
        kind: AccountType,
        code: &'static str,
        balance_cents: i64,
        archived: bool,
        credit_limit_cents: Option<i64>,
    ) -> crate::accounts::AccountRowSnap {
        crate::accounts::AccountRowSnap {
            id: id.to_owned(),
            name: name.to_owned(),
            kind,
            currency_code: code.to_owned(),
            is_archived: archived,
            balance_cents,
            credit_limit_cents,
            statement_day: None,
            due_day: None,
        }
    }

    /// 账户 face 集映射（快照 → 装配入参面）。
    fn faces<'a>(rows: &'a [crate::accounts::AccountRowSnap]) -> Vec<AccountRowFace<'a>> {
        rows.iter()
            .map(|row| AccountRowFace {
                id: &row.id,
                name: &row.name,
                kind: row.kind,
                currency_code: &row.currency_code,
                archived: row.is_archived,
                balance_cents: row.balance_cents,
                credit_limit_cents: row.credit_limit_cents,
                statement_day: row.statement_day,
                due_day: row.due_day,
            })
            .collect()
    }

    /// 行装配与分区（旧壳三分区 filter 同口径）：目标绑定进「攒钱」、已归档
    /// 进页底、普通留主表；格文本（类型标签 / 余额格式化 / 类型覆写）与列集
    /// 逐列对拍。删任一装配分支即红（先例同形：删除即红负向条目——面在
    /// 单测即函数分支本身）。
    #[test]
    fn account_rows_assemble_and_partition() {
        let asset = account_row(
            "a1",
            "现金",
            AccountType::Cash,
            "CNY",
            123_4567,
            false,
            None,
        );
        let goal = account_row(
            "a2",
            "旅行基金",
            AccountType::Other,
            "CNY",
            1000,
            false,
            None,
        );
        let archived = account_row("a3", "旧户", AccountType::Bank, "CNY", 500, true, None);
        let rows = vec![asset.clone(), goal.clone(), archived.clone()];

        let ctx = AccountListContext {
            currencies: Box::leak(Box::new(table())),
            goal_account_ids: Box::leak(Box::new({
                let mut set = std::collections::BTreeSet::new();
                set.insert("a2".to_owned());
                set
            })),
            privacy_enabled: false,
        };
        let cells = assemble_account_rows(&faces(&rows), &ctx);
        assert_eq!(cells.len(), 3, "一行的输入一行装配（顺序保持域清单序）");
        // 格文本：普通行 / 目标绑定覆写「攒钱」/ 余额格式化。
        assert_eq!(cells[0].name, "现金");
        assert_eq!(cells[0].type_label, "现金");
        assert_eq!(cells[0].currency_code, "CNY");
        assert_eq!(
            cells[0].balance, "¥1,2345.67",
            "V017 缓存读数经同一金额格式化（zh 四位一组，与列表同口径）"
        );
        assert_eq!(cells[1].type_label, "攒钱", "目标绑定账户类型覆写");
        assert!(!cells[1].archived && cells[1].goal_bound);
        assert_eq!(cells[2].type_label, "银行卡", "普通行不覆写");
        assert!(cells[2].archived && !cells[2].goal_bound);

        // 三分区：攒钱卡 / 主表 / 页底已归档（同快照纯派生零重排）。
        let (goals, normal, archived) = partition_account_rows(&cells);
        assert_eq!(
            goals.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["旅行基金"]
        );
        assert_eq!(
            normal.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["现金"]
        );
        assert_eq!(
            archived.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["旧户"]
        );
    }

    /// 隐私掩码在余额列与已归档合计收口（金额隐私同一装配入口）。
    #[test]
    fn account_rows_mask_balance_when_privacy_enabled() {
        let asset = account_row(
            "a1",
            "现金",
            AccountType::Cash,
            "CNY",
            123_4567,
            false,
            None,
        );
        let archived = account_row("a2", "旧户", AccountType::Bank, "CNY", 500, true, None);
        let rows = vec![asset, archived];
        let ctx = AccountListContext {
            currencies: Box::leak(Box::new(table())),
            goal_account_ids: Box::leak(Box::new(std::collections::BTreeSet::new())),
            privacy_enabled: true,
        };
        let cells = assemble_account_rows(&faces(&rows), &ctx);
        assert_eq!(cells[0].balance, AMOUNT_PRIVACY_MASK);
        assert_eq!(cells[1].balance, AMOUNT_PRIVACY_MASK);
        // 已归档合计同掩码（与主表同一格式化接缝）。
        let summary = assemble_archived_summary(&cells[1..2], &faces(&rows), &table(), true);
        assert!(
            summary.contains(AMOUNT_PRIVACY_MASK),
            "已归档合计掩码：{summary}"
        );
        assert!(summary.contains("1 个账户"), "计数并读法同旧壳");
    }

    /// 未结束计划数（旧壳 countUnfinishedPlansUsingAccount 重生）：付款 / 转出
    /// 两端取或、同计划双端只计一次；软删与非在用状态不计。
    #[test]
    fn count_unfinished_plans_fuses_both_account_ends() {
        let plan_face = |_id: &'static str,
                         account_id: &str,
                         to_account_id: Option<&str>,
                         status: &str,
                         deleted: bool| crate::assembly::PlanLeadFace {
            is_deleted: deleted,
            status: status.to_owned(),
            account_id: account_id.to_owned(),
            to_account_id: to_account_id.map(str::to_owned),
        };
        let plans = vec![
            plan_face("p1", "a1", None, "active", false),
            plan_face("p2", "a2", Some("a1"), "paused", false),
            plan_face("p3", "a1", None, "cancelled", false),
            plan_face("p4", "a1", None, "active", true),
        ];
        assert_eq!(
            count_unfinished_plans(&plans, "a1"),
            2,
            "p1 命中 + p2 传入端命中；同计划取或不双计"
        );
    }

    /// 信用卡使用率（旧壳 listUtilizationPercent 重生）：已用 > 0 且额度已设
    /// 才呈现；已用 ≥ 90% 判警示。
    #[test]
    fn credit_profile_derives_utilization_and_warning() {
        let money = account_row(
            "a1",
            "白金卡",
            AccountType::Credit,
            "CNY",
            -15_000,
            false,
            Some(20_000),
        );
        let owned = [money.clone()];
        let face = faces(&owned);
        let profile = credit_profile_fields(
            face[0].kind,
            face[0].credit_limit_cents,
            face[0].statement_day,
            face[0].due_day,
            face[0].balance_cents,
            chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(),
        )
        .expect("信用卡账户有档案派生");
        assert_eq!(profile.used_cents, 15_000);
        assert_eq!(profile.available_cents, Some(5_000));
        assert_eq!(profile.utilization_percent, Some(75));
        // 警示阈值两端（<90 正常 / ≥90 警示）。
        let near = account_row(
            "a2",
            "刷爆卡",
            AccountType::Credit,
            "CNY",
            -18_000,
            false,
            Some(20_000),
        );
        let near_owned = [near];
        let near_face = faces(&near_owned);
        let near_profile = credit_profile_fields(
            near_face[0].kind,
            near_face[0].credit_limit_cents,
            near_face[0].statement_day,
            near_face[0].due_day,
            near_face[0].balance_cents,
            chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(),
        )
        .unwrap();
        assert_eq!(near_profile.utilization_percent, Some(90));
        // 非信用卡账户无派生。
        assert!(
            credit_profile_fields(
                AccountType::Cash,
                None,
                None,
                None,
                -1_000,
                chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(),
            )
            .is_none()
        );
    }

    // -----------------------------------------------------------------
    // 储蓄目标视图格（#2056）：进度行装配 / 双分区 / 金额文本
    // -----------------------------------------------------------------

    use ledger_savings_goal::{
        SavingsGoal, SavingsGoalPaceSource, SavingsGoalProgress, SavingsGoalStatus,
    };

    /// 目标进度行（用例共用）：目标面 + 进度读数随参（币种恒 CNY）；
    /// 归档态 / 截止日 / 节奏字段由调用点就地赋值（行构造器窄参收口）。
    fn goal_row(
        id: &'static str,
        name: &'static str,
        target_cents: i64,
        saved_cents: i64,
    ) -> SavingsGoalProgress {
        let remaining = target_cents - saved_cents;
        SavingsGoalProgress {
            goal: SavingsGoal {
                id: id.to_owned(),
                name: name.to_owned(),
                target_amount_cents: target_cents,
                deadline: None,
                status: SavingsGoalStatus::Active,
                planned_monthly_cents: None,
                account_id: format!("acct-{id}"),
                created_at: "2027-03-04T05:06:07Z".to_owned(),
                updated_at: "2027-03-04T05:06:07Z".to_owned(),
                version: 1,
                device_id: "d".to_owned(),
                is_deleted: false,
            },
            saved_cents,
            remaining_cents: remaining,
            achieved: saved_cents >= target_cents,
            currency_code: "CNY".to_owned(),
            pace_monthly_cents: None,
            pace_source: None,
            eta_months: None,
            eta_month: None,
            required_monthly_cents: None,
            pace_delta_cents: None,
        }
    }

    #[test]
    fn savings_goal_rows_assemble_and_partition() {
        let mut g1 = goal_row("g1", "买车基金", 20_000, 5_000);
        g1.pace_monthly_cents = Some(100_000);
        g1.pace_source = Some(SavingsGoalPaceSource::Manual);
        let mut g2 = goal_row("g2", "已存满的目标", 10_000, 40_000);
        g2.pace_monthly_cents = Some(100_000);
        g2.pace_source = Some(SavingsGoalPaceSource::Plan);
        let g3 = {
            let mut row = goal_row("g3", "已归档的目标", 10_000, 1_000);
            row.goal.status = SavingsGoalStatus::Archived;
            row
        };
        let rows = vec![g1, g2, g3];
        let currencies = CurrencyTable::default();
        let (active, archived);
        {
            let cells = assemble_savings_goal_rows(&rows, &currencies, false);
            assert_eq!(SAVINGS_GOAL_COLUMNS.len(), 8, "列闭集 = 旧壳主表 8 列");
            assert_eq!(
                SAVINGS_GOAL_ARCHIVED_COLUMNS.len(),
                4,
                "归档列闭集 = 旧壳归档卡 4 列"
            );
            assert_eq!(cells.len(), 3);
            assert_eq!(cells[0].name, "买车基金");
            // 缺省币种表（未知 code 走缺省缺省参：无币符、两位小数去零）。
            assert_eq!(cells[0].target, "200");
            assert_eq!(cells[0].saved, "50");
            assert_eq!(cells[0].remaining, "150");
            // 推算列（旧壳 projection 渲染器同文案）：非达成行首行 = 节奏行
            //（来源闭集二值随行首可见），无截止正推 = ETA 行。
            // 推算列（旧壳 projection 渲染器同文案）：非达成行首行 = 节奏行
            //（来源闭集二值随行可见）。缺省币幅无币符：手填 1000/月。
            assert_eq!(
                cells[0].projection,
                vec![
                    "手填 1000/月".to_owned(),
                    "设置计划月存或挂一条自动转账计划，这里显示推算".to_owned()
                ],
                "节奏行（来源闭集二值随行可见）在；读数未给 ETA 时给设置引导而非虚构时点"
            );
            // status 列已初始化。达成行:推算整体退场,计划提示在。删任一
            // 分支即红(面在单测即函数分支本身)。
            assert_eq!(cells[1].status, "已达成");
            assert!(cells[1].projection.is_empty(), "达成态推算整体退场");
            assert!(cells[1].plan_hint, "达成 + 计划节奏 = 关联计划提示在场");
            assert_eq!(cells[0].status, "进行中");
            assert!(!cells[0].plan_hint);
            // 截止日缺席 = 占位破折号（旧壳 deadline ?? "—" 同口径）。
            assert_eq!(cells[0].deadline, "—");
            assert_eq!(cells[1].remaining, "-300");
            // 分区：进行中二行 / 归档一行，分区内序保持域序。
            let (a, ar) = partition_savings_goal_rows(&cells);
            (active, archived) = (a, ar);
        }
        assert_eq!(active.len(), 2);
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].name, "已归档的目标");
    }

    /// 双向推算的有截止反推与截止已过分支（词汇表 ETA / 所需月存）：
    /// 每月需存行 + 落后 / 超前差值行（差值缺席给对比引导）；截止日已过
    /// 只给一句话，不反推不虚构；手填 / 计划节奏来源闭集二值随行可见。
    #[test]
    fn savings_goal_projection_lines_cover_deadline_branches() {
        use ledger_savings_goal::SavingsGoalPaceSource as Pace;
        let fmt = CurrencyFormat::default();
        // 有截止 + 反推字段（域读数单点产出）：每月需存 + 落后差值。
        let lines = savings_goal_projection_lines(
            ProjectionFace {
                achieved: false,
                pace_monthly_cents: Some(10_000),
                pace_source: Some(Pace::Plan),
                deadline: Some("2027-01-31"),
                eta_months: None,
                eta_month: None,
                required_monthly_cents: Some(20_000),
                pace_delta_cents: Some(-8_000),
            },
            &fmt,
            false,
        );
        assert_eq!(
            lines,
            vec![
                "按计划 100/月".to_owned(),
                "每月需存 200".to_owned(),
                "落后 80/月".to_owned()
            ],
            "节奏来源闭集二值 + 反推所需月存 + 落后差值逐行"
        );
        // 反推 + 差值缺席：给对比引导（不虚构方向）。
        let lines = savings_goal_projection_lines(
            ProjectionFace {
                achieved: false,
                pace_monthly_cents: Some(10_000),
                pace_source: Some(Pace::Manual),
                deadline: Some("2027-01-31"),
                eta_months: None,
                eta_month: None,
                required_monthly_cents: Some(20_000),
                pace_delta_cents: None,
            },
            &fmt,
            false,
        );
        assert_eq!(
            lines,
            vec![
                "手填 100/月".to_owned(),
                "每月需存 200".to_owned(),
                "设置计划月存或挂一条自动转账计划，与所需月存对比".to_owned()
            ]
        );
        // 截止日已过：既不反推所需月存、也不虚构时点。
        let lines = savings_goal_projection_lines(
            ProjectionFace {
                achieved: false,
                pace_monthly_cents: None,
                pace_source: None,
                deadline: Some("2020-01-31"),
                eta_months: None,
                eta_month: None,
                required_monthly_cents: None,
                pace_delta_cents: None,
            },
            &fmt,
            false,
        );
        assert_eq!(lines, vec!["已过截止日".to_owned()]);
        // 超前差值行（反推 + 节奏充足）。
        let lines = savings_goal_projection_lines(
            ProjectionFace {
                achieved: false,
                pace_monthly_cents: Some(50_000),
                pace_source: Some(Pace::Plan),
                deadline: Some("2027-01-31"),
                eta_months: None,
                eta_month: None,
                required_monthly_cents: Some(20_000),
                pace_delta_cents: Some(30_000),
            },
            &fmt,
            false,
        );
        assert_eq!(lines.last(), Some(&"超前 300/月".to_owned()));
    }

    /// 蓄水进度金额隐私掩码（旧壳 formatAmount 隐私口径随行生效）：目标额 /
    /// 已存 / 还差 / 推算行金额全部掩码，文案行骨架保留。
    #[test]
    fn savings_goal_rows_mask_amounts_when_privacy_enabled() {
        let mut row = goal_row("g1", "买车基金", 20_000, 5_000);
        row.pace_monthly_cents = Some(100_000);
        row.pace_source = Some(SavingsGoalPaceSource::Manual);
        let rows = vec![row];
        let cells = assemble_savings_goal_rows(&rows, &CurrencyTable::default(), true);
        assert_eq!(cells[0].target, AMOUNT_PRIVACY_MASK);
        assert_eq!(cells[0].saved, AMOUNT_PRIVACY_MASK);
        assert_eq!(cells[0].remaining, AMOUNT_PRIVACY_MASK);
        assert_eq!(
            cells[0].projection[0],
            format!("手填 {AMOUNT_PRIVACY_MASK}/月"),
            "节奏行金额掩码、文案骨架保留"
        );
    }
}
