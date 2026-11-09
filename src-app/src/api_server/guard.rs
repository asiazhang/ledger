//! HTTP 半边信号守门（ADR-0073 决策 5 / ADR-0044 决策 3 修订；#2081 随宿主
//! 自根包 `signals_cross_check` 迁居）：HTTP 壳写路径接线的源码扫描核对。
//! 仅测试可见（`api_server/mod.rs` 以 `#[cfg(test)]` 挂载），生产构建不含本
//! 模块。
//!
//! 迁居裁决（双壳并存期守门拆分）：守门本体是「文本级扫描 + 契约真源消费」，
//! 与编译单元无关；HTTP 半边（handler 扫描、HTTP 派生声明、契约端点键核对）
//! 随宿主迁入本壳，IPC 半边（命令扫描、IPC 派生声明、例外白名单）留根包。
//! 跨壳的「孤儿身份」判定（每个映射身份至少被一壳声明）在根包——HTTP 壳的
//! 声明集在那里以静态登记表补入（根包 `signals_cross_check::
//! HTTP_SHELL_DECLARED_OPS`，逐条附端点动机；本壳演进写端点时同步维护该表），
//! #2083 切轨后根包守门随旧壳退役、孤儿判定全集由本模块接管（届时 IPC 消亡，
//! 并集 = 本模块派生声明 + `AutoBackupDeepPath` 特例豁免）。
//!
//! 扫描形态与根包守门同款（先例 `scripts/check-structure.ts`）：测试期文本级
//! 扫描，掩码注释与字符串/char 字面量后匹配；词法器具住 `crate::test_support::
//! scan`（#1433 单一维护点的本壳副本，三载体同规防漂移）；经别名改名的间接
//! 引用文本不可达，靠评审兜底。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use super::ApiDoc;
use crate::test_support::scan::{mask_non_code, matching_brace_end};
use ledger_infra::signals::{Signal, WriteEvidence, WriteOp, signals_for};
use utoipa::OpenApi;

// ---------------------------------------------------------------------------
// 源码扫描器具：分块、令牌提取（与根包 signals_cross_check 逐字节同构；
// 词法掩码与花括号配对原语住 crate::test_support::scan）
// ---------------------------------------------------------------------------

/// 一个 handler 函数体的扫描单元：锚点属性（`#[utoipa::path(...)]`）起、下一
/// 锚点（或文件尾 / 测试模块）止。
struct Chunk {
    /// 函数名（块内首个 `pub [async] fn` 的标识符）。
    name: String,
    /// 掩码后文本（注释与字符串已空白化），供令牌扫描。
    masked: String,
    /// 原文文本，供归因串 / 端点键等字符串字面量提取。
    raw: String,
}

/// 从掩码文本中按锚点切分函数体块：锚点位置在掩码文本上查找（注释中的锚点
/// 字样不误伤），块边界切在原文上（保留字符串字面量供提取）。
fn split_chunks(source: &str, anchor: &str) -> Vec<Chunk> {
    let masked = mask_non_code(source);
    let masked_chars: Vec<char> = masked.chars().collect();
    let source_chars: Vec<char> = source.chars().collect();
    // 掩码保长：字符下标在两份文本上同位（按 char 计）。
    assert_eq!(masked_chars.len(), source_chars.len(), "掩码必须保长");

    let needle: Vec<char> = anchor.chars().collect();
    let mut starts: Vec<usize> = Vec::new();
    let mut i = 0;
    while i + needle.len() <= masked_chars.len() {
        if masked_chars[i..i + needle.len()] == needle[..] {
            starts.push(i);
            i += needle.len();
        } else {
            i += 1;
        }
    }

    // 附加边界：测试模块起点（handler 后的 #[cfg(test)] 不属于 handler 体）。
    const TEST_ANCHOR: &str = "#[cfg(test)]";
    let test_needle: Vec<char> = TEST_ANCHOR.chars().collect();
    let mut ends: Vec<usize> = Vec::new();
    let mut i = 0;
    while i + test_needle.len() <= masked_chars.len() {
        if masked_chars[i..i + test_needle.len()] == test_needle[..] {
            ends.push(i);
            i += test_needle.len();
        } else {
            i += 1;
        }
    }

    // 与既有守门先例同款：块边界 = 下一锚点（或文件尾），函数体定位经
    // fn_body_end 花括号配对收窄（根包 #1433 后形态——不再把后续函数的
    // bypass 形态算进命令体）。找不到函数体（器具漂移）时回退旧边界，由下游
    // 断言兜底。
    let mut chunks = Vec::new();
    for (idx, &start) in starts.iter().enumerate() {
        let next = ends
            .iter()
            .chain(starts[idx + 1..].iter())
            .filter(|&&e| e > start)
            .copied()
            .min()
            .unwrap_or(source_chars.len());
        // fn_body_end 返回切片内相对偏移，加 start 绝对化；找不到函数体时回退
        // 旧边界。
        let end = fn_body_end(&masked_chars[start..next].iter().collect::<String>())
            .map(|rel| start + rel)
            .unwrap_or(next);
        let raw: String = source_chars[start..end].iter().collect();
        let masked_text: String = masked_chars[start..end].iter().collect();
        if let Some(name) = fn_name(&masked_text) {
            chunks.push(Chunk {
                name,
                masked: masked_text,
                raw,
            });
        }
    }
    chunks
}

/// 块内首个 `pub [async] fn` 的标记对（[`fn_name`] 命名提取与
/// [`fn_body_end`] 函数体定位共用，防两处漂移）。
const FN_MARKERS: [&str; 2] = ["pub async fn ", "pub fn "];

/// 掩码文本切片内首个 `pub [async] fn` 函数体的花括号配对结束位置（切片末尾
/// 相对偏移）。注释与字符串已在掩码中空白化，花括号只来自真实代码，配对可靠。
fn fn_body_end(masked: &str) -> Option<usize> {
    let fn_pos = FN_MARKERS
        .iter()
        .filter_map(|marker| masked.find(marker))
        .min()?;
    let open = masked[fn_pos..].find('{')? + fn_pos;
    matching_brace_end(masked, open)
}

/// 块内首个 `pub [async] fn` 的标识符（handler 函数体的命名约定）。
fn fn_name(masked: &str) -> Option<String> {
    for marker in FN_MARKERS {
        if let Some(pos) = masked.find(marker) {
            let rest = &masked[pos + marker.len()..];
            let ident: String = rest
                .chars()
                .take_while(|c| *c == '_' || c.is_alphanumeric())
                .collect();
            if !ident.is_empty() {
                return Some(ident);
            }
        }
    }
    None
}

/// 掩码文本中某令牌出现次数。
fn count_token(masked: &str, token: &str) -> usize {
    masked.matches(token).count()
}

/// 掩码文本中出现的全部 `WriteOp::<Variant>` 身份（去重）。
fn write_op_identities(masked: &str) -> HashSet<String> {
    let mut ops = HashSet::new();
    let mut rest = masked;
    while let Some(pos) = rest.find("WriteOp::") {
        let tail = &rest[pos + "WriteOp::".len()..];
        let ident: String = tail
            .chars()
            .take_while(|c| *c == '_' || c.is_alphanumeric())
            .collect();
        if !ident.is_empty() {
            ops.insert(ident);
        }
        rest = tail;
    }
    ops
}

/// 掩码文本中是否出现 `db::write` / `.write(` / 发射调用（反向守门的 bypass
/// 形态；文本级扫描，别名改写不可达靠评审兜底）。
fn has_bypass_write_or_emit(masked: &str, include_http_emit: bool) -> bool {
    if count_token(masked, "db::write(") > 0 || count_token(masked, ".write(") > 0 {
        return true;
    }
    if count_token(masked, "emit_for(") > 0 || count_token(masked, "emit_all(") > 0 {
        return true;
    }
    include_http_emit && count_token(masked, "emit_after_write(") > 0
}

/// 写入口调用形态闭集（issue #1276）：整段（`write_entry(`）与分段取锁、
/// 整体裁决（`write_entry_segmented(`，同步轮次形态）及 async 编排体分段形态
///（`write_entry_segmented_async(`，issue #1412）。「每个端点恰好一处写入口
/// 调用」的裁决对各形态同责；三个分段名共享前缀 `write_entry_segmented`，
/// 后随 `_` 不构成前名，需逐一枚举计数。
const WRITE_ENTRY_CALL_TOKENS: [&str; 3] = [
    "write_entry(",
    "write_entry_segmented(",
    "write_entry_segmented_async(",
];

/// 掩码文本中写入口调用点总数（各形态合计）。
fn write_entry_call_count(masked: &str) -> usize {
    WRITE_ENTRY_CALL_TOKENS
        .iter()
        .map(|token| count_token(masked, token))
        .sum()
}

/// 从原文中提取写入口调用点（各形态取先出现者）的首个字符串字面量（span
/// 归因串）。
fn write_entry_span(raw: &str) -> Option<String> {
    let (pos, token) = WRITE_ENTRY_CALL_TOKENS
        .iter()
        .filter_map(|token| raw.find(token).map(|p| (p, *token)))
        .min_by_key(|(p, _)| *p)?;
    let rest = raw[pos + token.len()..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// 从原文的 `#[utoipa::path(` 属性提取契约端点键（`METHOD /path`，与 OpenAPI
/// 契约端点键同款形状）。
fn handler_endpoint_key(raw: &str) -> Option<String> {
    let pos = raw.find("#[utoipa::path(")? + "#[utoipa::path(".len();
    let rest = raw[pos..].trim_start();
    let method = ["get", "post", "put", "delete"]
        .into_iter()
        .find(|m| rest.starts_with(m))?;
    let path_pos = raw[pos..].find("path = ")? + "path = ".len();
    let rest = raw[pos + path_pos..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(format!("{} {}", method.to_uppercase(), &rest[..end]))
}

/// handlers 目录下全部 .rs 文件的源文本（按文件名排序，输出确定）。
fn read_sources(rel: &str) -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("扫描目录不可读 {dir:?}: {e}"))
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    entries
        .into_iter()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            name.ends_with(".rs")
                .then(|| fs::read_to_string(e.path()).ok().map(|src| (name, src)))
                .flatten()
        })
        .collect()
}

/// HTTP 壳的 `#[utoipa::path(...)]` handler 函数体块（全 handler 文件）。
fn http_handler_chunks() -> Vec<Chunk> {
    let mut chunks = Vec::new();
    for (_, src) in read_sources("src/api_server/handlers") {
        chunks.extend(split_chunks(&src, "#[utoipa::path("));
    }
    chunks
}

/// HTTP 派生声明表：handler 块内 `write_entry` 调用点提取的（端点键, 身份）。
/// 端点键取自 handler 自身的 `#[utoipa::path]` 契约注解（`METHOD /path`，与
/// OpenAPI 同源派生）——注解漂移即键漂移，守门顺带核对。
fn http_derived_declarations() -> Vec<(String, WriteOp)> {
    let mut declared = Vec::new();
    for chunk in http_handler_chunks() {
        let call_sites = write_entry_call_count(&chunk.masked);
        if call_sites == 0 {
            continue;
        }
        let identities = write_op_identities(&chunk.masked);
        assert_eq!(
            call_sites, 1,
            "HTTP handler {} 含 {call_sites} 处 write_entry 调用（应恰为一行调用）",
            chunk.name
        );
        assert_eq!(
            identities.len(),
            1,
            "HTTP handler {} 的 write_entry 块内身份不唯一：{identities:?}",
            chunk.name
        );
        let endpoint_key = handler_endpoint_key(&chunk.raw)
            .unwrap_or_else(|| panic!("HTTP handler {} 缺 #[utoipa::path] 端点注解", chunk.name));
        let ident = identities.into_iter().next().expect("长度已断言为 1");
        let op = resolve_write_op(&ident);
        declared.push((endpoint_key, op));
    }
    declared
}

/// 扫描提取的身份标识符 → `WriteOp`（ADR-0102 决策 2）：经
/// [`WriteOp::from_ident`] 映射（宏清单构造性保证完备）；提取到非变体文本
///（扫描器具自身漂移）以断言失败报「扫描提取漂移」。
fn resolve_write_op(ident: &str) -> WriteOp {
    WriteOp::from_ident(ident).unwrap_or_else(|| {
        panic!(
            "扫描提取漂移：{ident} 不是 WriteOp 变体标识符——\
             信号守门扫描器具与 enum 本体不一致"
        )
    })
}

/// OpenAPI 契约自描述的端点集（`"METHOD /path"` 形状）：HTTP 壳注册面真源，
/// 与 `#[utoipa::path]` 注解同源派生。
fn openapi_endpoints() -> HashSet<String> {
    let doc = ApiDoc::openapi();
    let mut endpoints = HashSet::new();
    for (path, item) in doc.paths.paths.iter() {
        for (method, op) in [
            ("GET", &item.get),
            ("POST", &item.post),
            ("PUT", &item.put),
            ("DELETE", &item.delete),
        ] {
            if op.is_some() {
                endpoints.insert(format!("{method} {path}"));
            }
        }
    }
    endpoints
}

// ---------------------------------------------------------------------------
// 守门：单壳身份唯一 · 声明身份必被映射 · 反向守门 + 归因串/端点键核对
//（与根包守门二/三/五的 HTTP 半边逐条同构；孤儿身份判定留根包，见模块文档）
// ---------------------------------------------------------------------------

/// HTTP 壳里两个端点声明同一 `WriteOp` 视为复制粘贴错误（与根包守门二的
/// HTTP 分支同构——跨壳共享身份合法，本壳内不唯一即红）。
#[test]
fn write_op_is_declared_at_most_once_in_http_shell() {
    let declared = http_derived_declarations();
    let mut seen: HashMap<WriteOp, &str> = HashMap::new();
    for (name, op) in &declared {
        let first = seen.insert(*op, name);
        assert!(
            first.is_none(),
            "HTTP 壳中 {op:?} 被重复声明：{} 与 {name}",
            first.unwrap_or("")
        );
    }
}

/// 每个声明身份经 `signals_for` 的返回值必须落在四个已知信号集之内——映射行
/// 不缺、形状不漂（与根包守门三的 HTTP 半边同构）。
#[test]
fn every_declared_write_op_is_mapped() {
    for (key, op) in http_derived_declarations() {
        let signals = signals_for(op, WriteEvidence::None);
        let known = matches!(signals, &[])
            || matches!(signals, &[Signal::LedgerChanged])
            || matches!(signals, &[Signal::PricesChanged])
            || matches!(signals, &[Signal::BackupsChanged]);
        assert!(
            known,
            "声明身份 {op:?}（{key}）经 signals_for 返回未知信号集 {signals:?}——\
             映射行缺位或形状漂移"
        );
    }
}

/// HTTP 反向守门：handler 函数体内出现 `db::write` / `.write(` / 发射调用而无
/// `write_entry` 即红，「绕开入口写库」的回归被兜住。HTTP 壳无例外白名单——
/// 9 个写端点全部经统一写入口（ADR-0073 决策 7）。
#[test]
fn http_write_calls_must_go_through_write_entry() {
    for chunk in http_handler_chunks() {
        let has_write_entry = write_entry_call_count(&chunk.masked) > 0;
        if has_write_entry || !has_bypass_write_or_emit(&chunk.masked, true) {
            continue;
        }
        panic!(
            "HTTP handler {} 的函数体出现 db::write / 发射调用却未走 write_entry——\
             绕开统一写入口的回归（ADR-0073）",
            chunk.name
        );
    }
}

/// HTTP 归因串核对（ADR-0073 决策 4 顺带）：`write_entry` 归因串 == 契约端点键
///（`METHOD /path`，取自 handler 自身的 `#[utoipa::path]` 注解），SQL 耗时日志
/// 逐字节不漂（ADR-0009 / ADR-0068）。
#[test]
fn http_write_entry_span_matches_endpoint_key() {
    for chunk in http_handler_chunks() {
        if write_entry_call_count(&chunk.masked) == 0 {
            continue;
        }
        let span = write_entry_span(&chunk.raw).unwrap_or_else(|| {
            panic!(
                "HTTP handler {} 的 write_entry 调用缺 span 归因串",
                chunk.name
            )
        });
        let endpoint_key = handler_endpoint_key(&chunk.raw)
            .unwrap_or_else(|| panic!("HTTP handler {} 缺 #[utoipa::path] 端点注解", chunk.name));
        assert_eq!(
            span, endpoint_key,
            "HTTP handler {} 的 write_entry 归因串与契约端点键漂移",
            chunk.name
        );
    }
}

/// HTTP 派生端点键必须都在 OpenAPI 契约端点集上（契约自描述真源）：注解与
/// 契约装配同源派生，本核对把该事实显式登记在测试面——扫描提取漂移或契约
/// 漏记在此即红（接替旧「声明表 × OpenAPI」双向比对的写端点方向）。
#[test]
fn http_derived_endpoint_keys_are_in_openapi_contract() {
    let endpoints = openapi_endpoints();
    for (key, _) in http_derived_declarations() {
        assert!(
            endpoints.contains(&key),
            "HTTP 派生端点键 {key} 不在 OpenAPI 契约端点集上——扫描提取漂移或契约漏记"
        );
    }
}

/// 壳层标准锁行模式（ADR-0104）：与根包守门六同一根常量（`.lock().map_err(...)`
/// 手抄仪式）；读侧反向守门的 HTTP 半边随 #2081 迁居本壳。
const STANDARD_LOCK_LINE: &str = ".lock().map_err(|e| AppError::Db(e.to_string()))";

/// HTTP 反向守门（读侧，ADR-0104 决策 6，自根包 signals_cross_check 迁居）：
/// handler 函数体内出现标准锁行而同函数体无 `read_entry` / `write_entry` 即红。
/// HTTP 壳无豁免清单——7 个读端点全部迁入读入口（ADR-0104 决策 7），出现
/// 手写锁行即回归。
#[test]
fn http_lock_ritual_must_go_through_read_entry() {
    for chunk in http_handler_chunks() {
        let through_entry = count_token(&chunk.masked, "read_entry(") > 0
            || write_entry_call_count(&chunk.masked) > 0;
        if through_entry || count_token(&chunk.masked, STANDARD_LOCK_LINE) == 0 {
            continue;
        }
        panic!(
            "HTTP handler {} 的函数体出现手写标准锁行却未走 read_entry——\n             锁仪式回潮（ADR-0104 决策 6）",
            chunk.name
        );
    }
}
