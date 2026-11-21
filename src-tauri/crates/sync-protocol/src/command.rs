use std::borrow::Cow;

pub trait SyncCommand: serde::Serialize {
    const ENTITY: &'static str;
    fn subject(&self) -> Option<Cow<'_, str>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayEffect {
    Applied,
    IdempotentHit,
}
