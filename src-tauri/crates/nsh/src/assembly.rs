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
/// 点），借出/收回/借入/还款专属文案；普通转账恒「转账」；非 transfer kind 按
/// 自身 kind 标签。
pub fn kind_label(kind: TransactionKind, lending: LendingDirection) -> &'static str {
    if kind == TransactionKind::Transfer {
        return match lending {
            LendingDirection::Lend => "借出",
            LendingDirection::Collect => "收回",
            LendingDirection::Borrow => "借入",
            LendingDirection::Repay => "还款",
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
    by_code: BTreeMap<String, CurrencyFormat>,
}

impl CurrencyTable {
    /// 从币种参考数据行集投影。
    pub fn from_currencies<'a>(rows: impl IntoIterator<Item = &'a Currency>) -> Self {
        Self {
            by_code: rows
                .into_iter()
                .map(|c| {
                    (
                        c.code.clone(),
                        CurrencyFormat {
                            symbol: c.symbol.clone(),
                            decimal_places: u32::try_from(c.decimal_places.max(0)).unwrap_or(2),
                        },
                    )
                })
                .collect(),
        }
    }

    /// 币种 `code` 的展示参数；未登记（币种被删等）走缺省，不静默换符号。
    pub fn format_of(&self, code: &str) -> CurrencyFormat {
        self.by_code.get(code).cloned().unwrap_or_default()
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
}
