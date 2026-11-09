//! 取数尾部契约（spec #1675）：取数单元公共尾部的单一实现——**源畸形时的处置
//! 顺序**（统一截断 warn → 补降速信号 → 码化错误映射）由本模块拥有，五个取数
//! 单元（场内批量报价、场外基金批量净值面、场外基金单只全历史面、披露区间查询、
//! ECB 汇率文档，ADR-0130 链路选源）与未来的新单元共享它。
//!
//! 契约形状是**全尾函数**（[`finish`]），无 trait / 注册表：没有「遍历取数单元统一
//! 处理」的多态消费点，机制税不付（ADR-0103 / ADR-0121 / ADR-0130 决策 9 同一口味）。
//! 单元只出**解码闭包 + 解析闭包**与**错误映射**：解析失败回 `Err(detail)`，失败
//! 原因保留在错误详情；解码失败与形状判据失败在本函数内顺序汇入同一条源畸形
//! 路径，降速信号结构性不可绕过（ADR-0121 决策 5：解析失败即数据源异常信号）。
//!
//! 统一日志形状 = 恰一条 warn：固定「源畸形」文案（grep 锚）+ `source`（单元标识，
//! 分源用）/ `error`（错误详情）/ `body`（**解码后**响应文本的截断片段——解码都
//! 失败时才有损转换兜底）三字段——单元内散落的形状 warn 一律上收于此，一次失败
//! 一条、不再两处各记一半。

use ledger_infra::error::{AppError, Result};

use super::http::Pacer;

/// 日志用响应体截断片段的字符上限（单点，spec #1675 裁决 2：原两份逐字符相同的
/// `body_head` 副本随本契约删除）。
const BODY_HEAD_CHARS: usize = 120;

/// 取数尾部契约的 `source` 字段之上的统一 warn 文案——固定含「源畸形」，是排查者
/// 定位任一源异常的 grep 锚（三字段之外的这句文案五单元逐字同形）。
const SOURCE_MALFORMED_MESSAGE: &str = "数据源响应源畸形（已补降速信号）";

/// 日志用的响应文本截断片段（按字符 × [`BODY_HEAD_CHARS`]，单点）：避免日志吞下
/// 整页 HTML。只在失败路径求值。
fn body_head(body: &str) -> String {
    body.chars().take(BODY_HEAD_CHARS).collect()
}

/// UTF-8 文本通道的解码闭包：传输层已按 UTF-8 解码过，此处的再校验是防御
///（不可达失败与形状失败同走 [`finish`] 的源畸形路径）。拷贝一份归 [`finish`]
/// 统一持有，换取解码 / 解析两闭包同一条所有权路径。
pub(super) fn utf8(response: &[u8]) -> std::result::Result<String, String> {
    std::str::from_utf8(response)
        .map(str::to_owned)
        .map_err(|error| error.to_string())
}

/// 源畸形的统一尾部（顺序单点）：截断 warn → 补降速信号 → 码化错误映射。
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

/// 取数尾部契约的全尾执行：解码 → 解析，成功即原样返回；失败（解码失败与形状
/// 判据失败同途）按契约顺序处置——
///
/// 1. **统一截断 warn**：固定「源畸形」文案 + `source` / `error` / `body` 三字段，
///    恰一条（单元解析器内部的形状 warn 已上收，不再叠加）；`body` 取解码后的
///    响应文本，解码失败时才有损转换兜底；
/// 2. **补降速信号**：`pacer.record_throttled()`（ADR-0121 决策 5——源异常即降速，
///    防止单一源的异常响应把请求节奏顶回基线）；
/// 3. **码化错误映射**：以单元提供的映射产出错误（用户可见的源畸形一律专码，
///    ADR-0050；码值与分码是各单元的既定契约，本契约不收码值、只收构造时机）。
///
/// 顺序与日志形状的单点即本函数——单元的 fetch 尾部只许经此退出源畸形路径，
/// 约定的遵守由此从注释先例变成可测的模块 API（spec #1675）。
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
        // 解码失败没有可信文本可截，有损片段是唯一选择——与形状失败同一条尾部。
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
