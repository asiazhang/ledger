use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SyncInstrumentInfoResult {
    pub synced: usize,
    pub skipped: usize,
    pub message: String,
    #[serde(skip)]
    pub written: usize,
    #[serde(skip)]
    pub renamed: usize,
    pub bulk_degraded: bool,
    #[serde(skip)]
    pub bulk_gaps: usize,
}

impl SyncInstrumentInfoResult {
    pub fn any_written(&self) -> bool {
        self.written > 0 || self.renamed > 0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriteWitness {
    written: bool,
}

impl WriteWitness {
    pub fn mark_written(&mut self) {
        self.written = true;
    }

    pub fn any_written(&self) -> bool {
        self.written
    }
}
