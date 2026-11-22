use crate::envelope::EnvelopeMode;

static SESSION_ENVELOPE: std::sync::Mutex<Option<SessionEnvelope>> = std::sync::Mutex::new(None);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEnvelope {
    Plaintext,
    Encrypted(String),
}

impl SessionEnvelope {
    pub fn remember(session: SessionEnvelope) {
        *SESSION_ENVELOPE.lock().unwrap_or_else(|e| e.into_inner()) = Some(session);
    }

    pub fn forget() {
        *SESSION_ENVELOPE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn current() -> Self {
        SESSION_ENVELOPE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or(SessionEnvelope::Plaintext)
    }

    pub fn mode(&self) -> EnvelopeMode<'_> {
        match self {
            Self::Plaintext => EnvelopeMode::Plaintext,
            Self::Encrypted(passphrase) => EnvelopeMode::Encrypted { passphrase },
        }
    }
}
