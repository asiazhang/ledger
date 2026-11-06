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
//! - **范围（#2019 全家桶）**：数字分组、金额语义色、隐私模式遮蔽（掩码消费
//!   设备轻量设置项，同一域来源）、分类路径、备注列、通用 kind 标签；借贷派生
//!   transfer 专属文案与派生行形态（购买项 / 双账户 / 出资 / 来源）归 #2020；
//! - **界面文案仅 zh-CN**（spec #2015 实施决策：i18n 框架级方案不在立桩票），
//!   数字分组固定 zh-CN 四位一组（分组位数由界面语言隐含——NSH 无界面语言
//!   切换，恒 zh；en-US 三位一组随 i18n 方案票再议）。

use std::collections::BTreeMap;

use ledger_currencies::Currency;
use ledger_transaction::{Transaction, TransactionKind};

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
/// 旧壳桌面表格一致（旧壳列全集 = 日期/类型/分类/商户/账户/来源/备注/金额/操作，
/// 商户/账户/来源/操作列随 #2020 接线）。金额列末端对齐（数字惯例）。备注列旧
/// 壳不设固定宽（弹性列），此处给建议宽度供 UI 映射。
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
        key: "note",
        title: "备注",
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
/// 供行级操作定位（行右键删除随 #2022 消费）。
#[derive(Debug, Clone, PartialEq)]
pub struct RowCells {
    /// 交易 id。
    pub id: String,
    /// 日期列（ISO 原样，与旧壳同口径）。
    pub date: String,
    /// 类型列（kind 直译标签；借贷派生专属文案随 #2020 接线）。
    pub kind: String,
    /// 分类列（`父 > 子` 路径；未解析渲染 `-`）。
    pub category: String,
    /// 备注列（备注原文；无备注渲染 `-`）。
    pub note: String,
    /// 金额列（含隐私掩码与数字分组；无金额行渲染 `-`）。
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

/// 类型标签（首版 = kind 直译，文案与旧壳 zh-CN 词表一致）：借贷派生 transfer
/// 的「借出/收回/借入/还款」专属文案随 #2020 接线，普通转账恒「转账」。
pub fn kind_label(kind: TransactionKind) -> &'static str {
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

/// 装配消费面（数据到达时随行集一并备齐的参考数据与开关）：装配函数的
/// 唯一入参形态——参考数据表与设备开关随票增长收在此处，不散成位置参数。
pub struct RowContext<'a> {
    /// 币种展示参数表。
    pub currencies: &'a CurrencyTable,
    /// 分类路径表。
    pub categories: &'a CategoryTable,
    /// 金额隐私模式（设备轻量设置项；真源与水合见 [`crate::app`]）。
    pub privacy_enabled: bool,
}

/// 金额隐私掩码（spec #564 掩码恒等性 / issue #566）：固定四点、无负号、无
/// 币符、不随语言与数量级变化——与旧壳 `AMOUNT_PRIVACY_MASK` 同字面。
pub const AMOUNT_PRIVACY_MASK: &str = "••••";

/// 装配行集（数据到达时一次性产出全部格文本）：读路径行 → 表格行的唯一入口。
pub fn assemble_rows(items: &[Transaction], ctx: &RowContext) -> Vec<RowCells> {
    items
        .iter()
        .map(|row| {
            let amount = match amount_display_cents(row) {
                None => "-".to_owned(),
                // 隐私模式收口在金额格式化层（旧壳 formatAmount 同位）：调用点
                // 零判断；`-`（份额调整无金额）是空值口径、不是金额，照旧渲染。
                Some(_) if ctx.privacy_enabled => AMOUNT_PRIVACY_MASK.to_owned(),
                Some(cents) => {
                    format_amount_basic(cents, &ctx.currencies.format_of(&row.currency_code))
                }
            };
            let category = ctx
                .categories
                .path_of(row.category_id.as_deref())
                .unwrap_or_else(|| "-".to_owned());
            RowCells {
                id: row.id.clone(),
                date: row.date.clone(),
                kind: kind_label(row.kind).to_owned(),
                category,
                note: row.note.clone().unwrap_or_else(|| "-".to_owned()),
                amount,
                amount_semantic: amount_semantic(row.kind),
            }
        })
        .collect()
}

/// 总页数（offset 分页，ADR-0008）：末页不满一页也算一页；`total = 0` 恒 1
/// （分页条在单页时由 UI 层收起，见 [`crate::app`]）。
pub fn total_pages(total: i64) -> u32 {
    let pages = (total.max(0) as u64).div_ceil(PAGE_SIZE as u64);
    pages.max(1) as u32
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
            ["date", "kind", "category", "note", "amount"],
            "列定义与顺序同旧壳桌面表格（商户/账户/来源/操作随 #2020）"
        );
        let titles: Vec<_> = COLUMNS.iter().map(|c| c.title).collect();
        assert_eq!(
            titles,
            ["日期", "类型", "分类", "备注", "金额"],
            "列名 zh-CN 与旧壳 transactions.columns 词表同文案"
        );
        let amount = COLUMNS.iter().find(|c| c.key == "amount").unwrap();
        assert_eq!(amount.align, ColumnAlign::End, "金额列末端对齐");
    }

    #[test]
    fn kind_labels_cover_the_whole_closed_set() {
        assert_eq!(kind_label(TransactionKind::Income), "收入");
        assert_eq!(kind_label(TransactionKind::Expense), "支出");
        assert_eq!(kind_label(TransactionKind::Transfer), "转账");
        assert_eq!(kind_label(TransactionKind::Refund), "退款");
        assert_eq!(kind_label(TransactionKind::Buy), "买入");
        assert_eq!(kind_label(TransactionKind::Sell), "卖出");
        assert_eq!(kind_label(TransactionKind::Convert), "转换");
        assert_eq!(kind_label(TransactionKind::Split), "份额调整");
        assert_eq!(kind_label(TransactionKind::Dividend), "分红");
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

    /// 装配上下文（用例共用）：币种表 + 分类路径表 + 隐私开关。
    fn context(privacy: bool) -> RowContext<'static> {
        RowContext {
            currencies: Box::leak(Box::new(table())),
            categories: Box::leak(Box::new(categories())),
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

    #[test]
    fn total_pages_counts_partial_last_page() {
        assert_eq!(total_pages(0), 1, "空库恒 1 页（分页条由 UI 收起）");
        assert_eq!(total_pages(1), 1);
        assert_eq!(total_pages(PAGE_SIZE as i64), 1, "整页边界");
        assert_eq!(total_pages(PAGE_SIZE as i64 + 1), 2, "末页不满一页也算一页");
        assert_eq!(total_pages(250), 3);
        assert_eq!(total_pages(-5), 1, "负值（不可达口径）钳到 1");
    }
}
