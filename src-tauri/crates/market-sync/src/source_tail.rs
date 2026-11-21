use ledger_infra::error::{AppError, Result};

use super::http::Pacer;

const BODY_HEAD_CHARS: usize = 120;

const SOURCE_MALFORMED_MESSAGE: &str = "数据源响应源畸形（已补降速信号）";

fn body_head(body: &str) -> String {
    body.chars().take(BODY_HEAD_CHARS).collect()
}

pub(super) fn utf8(response: &[u8]) -> std::result::Result<String, String> {
    std::str::from_utf8(response)
        .map(str::to_owned)
        .map_err(|error| error.to_string())
}

fn malformed(
    source: &str,
    body: &str,
    error: impl std::fmt::Display,
    pacer: &mut Pacer,
    map_error: impl FnOnce() -> AppError,
) -> AppError {
    tracing::warn!(
        source,
        error = %error,
        body = %body_head(body),
        "{}",
        SOURCE_MALFORMED_MESSAGE
    );
    pacer.record_throttled();
    map_error()
}

pub(super) fn finish<T>(
    source: &str,
    response: &[u8],
    pacer: &mut Pacer,
    decode: impl FnOnce(&[u8]) -> std::result::Result<String, String>,
    parse: impl FnOnce(&str) -> std::result::Result<T, String>,
    map_error: impl FnOnce() -> AppError,
) -> Result<T> {
    let body = match decode(response) {
        Ok(body) => body,
        Err(error) => {
            let lossy = String::from_utf8_lossy(response).into_owned();
            return Err(malformed(source, &lossy, error, pacer, map_error));
        }
    };
    match parse(&body) {
        Ok(value) => Ok(value),
        Err(error) => Err(malformed(source, &body, error, pacer, map_error)),
    }
}
