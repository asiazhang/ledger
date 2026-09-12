//! 结果证据类型（ADR-0044 决策 1 / 决策 4）：写操作自然返回值的归一化形状。
//!
/// 结果证据（ADR-0044 决策 1 / 决策 4）：写操作本次执行的**自然返回值**归一化，
/// 承载条件信号的「条件」一半。默认 [`WriteEvidence::None`]（无证据，静态行决定信号）；
/// 三类条件信号各占一个布尔变体，「真 / 假」由调用方按域内口径归一化（如
/// sync `written > 0`、基金增强 `price_written`、行为层「即建商户」），
/// 映射表内只保留一份「实际写入」判定（[`WriteEvidence::price_written`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteEvidence {
    /// 无证据（默认）：信号完全由写操作身份的静态映射行决定。
    None,
    /// 价格实际写入：增量同步「落库 n>0」、
    /// 按代码即拉「落现价缓存」、手动报价「实际写入任一落点」的统一形状。
    PriceWritten(bool),
    /// 余额调整按需新建黑洞账户（参考表变更；纯转账零变化）。
    BlackHoleCreated(bool),
    /// 交易写「即建商户」（入参带 `merchant_name` 且未命中，写第四张参考表；
    /// 仅命中复用为零信号，ADR-0028）。
    MerchantCreated(bool),
    /// 多端同步轮次「本轮实际应用外来 op」（applied > 0，issue #862）：零应用轮次
    /// （全部跳过 / 去重 / 压制 / 挂起）为假，不广播。
    LedgerApplied(bool),
}

impl WriteEvidence {
    /// 「实际写入」判定（映射内唯一一份，ADR-0044 决策 4）：价格证据为真。
    /// 其余证据形状（含 [`WriteEvidence::None`]）一律为否——证据错配保守降级为
    /// 零信号，不发错信号。
    pub(crate) fn price_written(&self) -> bool {
        matches!(self, WriteEvidence::PriceWritten(true))
    }

    /// 黑洞账户即建证据为真。
    pub(crate) fn black_hole_created(&self) -> bool {
        matches!(self, WriteEvidence::BlackHoleCreated(true))
    }

    /// 商户即建证据为真（映射判定与批量聚合共享这一份形状判定）。
    pub fn merchant_created(&self) -> bool {
        matches!(self, WriteEvidence::MerchantCreated(true))
    }

    /// 同步轮次「本轮实际应用外来 op」证据为真（applied > 0；映射判定与轮次
    /// 报告归一化共享这一份形状判定，issue #862）。
    pub(crate) fn ledger_applied(&self) -> bool {
        matches!(self, WriteEvidence::LedgerApplied(true))
    }
}
