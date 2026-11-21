use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::Mutex as AsyncMutex;

use super::channels::FetchFuture;
use super::http::{Pacer, lock_pacer};
use super::sina_fund::{fetch_sina_fund_nav_rows, fund_batch_from_rows};

pub type FundNameDictionary = HashMap<String, String>;

#[derive(Debug, Clone, PartialEq)]
pub struct BulkNavPoint {
    pub date: String,
    pub nav: f64,
}

impl BulkNavPoint {
    pub fn is_not_newer_than(&self, watermark: Option<&str>) -> bool {
        watermark.is_some_and(|watermark| self.date.as_str() <= watermark)
    }
}

pub type FundNavTable = HashMap<String, BulkNavPoint>;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FundBatch {
    pub names: FundNameDictionary,
    pub nav: FundNavTable,
}

impl FundBatch {
    pub fn covers(&self, code: &str) -> bool {
        self.names.contains_key(code)
    }

    pub fn name_of(&self, code: &str) -> Option<&str> {
        self.names.get(code).map(String::as_str)
    }

    pub fn nav_of(&self, code: &str) -> Option<&BulkNavPoint> {
        self.nav.get(code)
    }
}

pub type FetchFundBatch = Box<dyn FnMut(&[String]) -> FetchFuture<FundBatch> + Send>;

pub const BULK_FAILURE_THRESHOLD: u32 = 3;
pub const BULK_DISABLE_PERIOD: Duration = Duration::from_secs(1800);

#[derive(Debug)]
pub struct BulkFetchCircuit {
    consecutive_failures: u32,
    disabled_until: Option<Instant>,
}

impl BulkFetchCircuit {
    pub fn new() -> Self {
        Self {
            consecutive_failures: 0,
            disabled_until: None,
        }
    }

    pub fn should_attempt(&mut self, now: Instant) -> bool {
        match self.disabled_until {
            None => true,
            Some(until) if now < until => false,
            Some(_) => {
                self.disabled_until = Some(now + BULK_DISABLE_PERIOD);
                true
            }
        }
    }

    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.disabled_until = None;
    }

    pub fn record_failure(&mut self, now: Instant) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= BULK_FAILURE_THRESHOLD {
            self.disabled_until = Some(now + BULK_DISABLE_PERIOD);
        }
    }

    pub fn is_disabled(&self, now: Instant) -> bool {
        matches!(self.disabled_until, Some(until) if now < until)
    }

    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }
}

impl Default for BulkFetchCircuit {
    fn default() -> Self {
        Self::new()
    }
}

pub fn shared_circuit() -> Arc<Mutex<BulkFetchCircuit>> {
    static SHARED: OnceLock<Arc<Mutex<BulkFetchCircuit>>> = OnceLock::new();
    SHARED
        .get_or_init(|| Arc::new(Mutex::new(BulkFetchCircuit::new())))
        .clone()
}

pub struct BulkFetchSurfaces {
    pub funds: FetchFundBatch,
    pub circuit: Arc<Mutex<BulkFetchCircuit>>,
}

impl BulkFetchSurfaces {
    pub(super) fn production(
        client: &reqwest::Client,
        pacer: Arc<AsyncMutex<Pacer>>,
        hosts: Vec<String>,
    ) -> Self {
        Self {
            funds: {
                let client = client.clone();
                let pacer = pacer.clone();
                Box::new(move |codes: &[String]| {
                    let codes = codes.to_vec();
                    let client = client.clone();
                    let pacer = pacer.clone();
                    let hosts = hosts.clone();
                    Box::pin(async move {
                        let mut pacer = lock_pacer(&pacer).await;
                        let hosts: Vec<&str> = hosts.iter().map(String::as_str).collect();
                        let rows =
                            fetch_sina_fund_nav_rows(&client, &mut pacer, &hosts, &codes).await?;
                        Ok(fund_batch_from_rows(rows))
                    })
                })
            },
            circuit: shared_circuit(),
        }
    }

    pub fn absent() -> Self {
        Self {
            funds: Box::new(|_: &[String]| Box::pin(async { Ok(FundBatch::default()) })),
            circuit: Arc::new(Mutex::new(BulkFetchCircuit::new())),
        }
    }
}
