#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteEvidence {
    None,
    PriceWritten(bool),
    BlackHoleCreated(bool),
    MerchantCreated(bool),
    LedgerApplied(bool),
}

impl WriteEvidence {
    pub fn price_written(&self) -> bool {
        matches!(self, WriteEvidence::PriceWritten(true))
    }

    pub(crate) fn black_hole_created(&self) -> bool {
        matches!(self, WriteEvidence::BlackHoleCreated(true))
    }

    pub fn merchant_created(&self) -> bool {
        matches!(self, WriteEvidence::MerchantCreated(true))
    }

    pub(crate) fn ledger_applied(&self) -> bool {
        matches!(self, WriteEvidence::LedgerApplied(true))
    }
}
