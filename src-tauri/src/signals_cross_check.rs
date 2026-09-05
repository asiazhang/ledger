//! 信号守门测试（ADR-0044 决策 3 修订 / ADR-0073 决策 5，spec #523）：信号映射单点
//! × 两壳写路径接线的源码扫描核对。仅测试可见（lib.rs 以 `#[cfg(test)]` 挂载），
//! 生产构建不含本模块。
//!
//! 手写声明表已消亡为源码扫描派生物（ADR-0073 决策 5）：从命令 / handler 函数体
//! 扫描 `write_entry` 调用点提取（声明壳, 身份）派生表，配合例外白名单与反向守门，
//! 「漏声明即红」的守门价值不降级：
//! - **孤儿身份**：`signals_for` 映射表中的每个写操作身份都必须被至少一个壳声明
//!   （`write_entry` 调用点或例外白名单）；特例条目 `AutoBackupDeepPath` 除外
//!   （登记生产者清单、刻意不做命令键）；
//! - **单壳身份唯一**：同一壳里两个命令/端点声明同一 `WriteOp` 视为复制粘贴错误；
//! - **反向守门**：命令 / handler 函数体内出现 `db::write` / 发射调用而无
//!   `write_entry` 即红——「绕开入口写库」的回归被兜住，不经入口的声明写命令
//!   必须进例外白名单（逐个附动机）；
//! - **归因串核对**（ADR-0073 决策 4 顺带）：IPC 侧 `write_entry` 归因串 == 命令名；
//!   HTTP 侧归因串 == 契约端点键（`METHOD /path`），日志逐字节不漂。
//!
//! 「声明了但映射缺行」方向由 `signals_for` 的编译期穷尽 match 兜底（enum 新增
//! 变体漏改映射编译即错）；测试仍对每个声明身份复核映射返回值形状，把该方向
//! 的保障显式登记在测试面。信号知识本身（每身份一条直测断言，含零信号显式）
//! 全部保留在 `signals::tests`，不在本文件。
//!
//! 扫描形态与既有守门先例同款（`scripts/check-structure.js`）：测试期文本级扫描，
//! 掩码注释与字符串/char 字面量后匹配；经别名改名的间接引用文本不可达，靠评审兜底。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::api_server::ApiDoc;
use crate::signals::{Signal, WriteEvidence, WriteOp, signals_for};
use utoipa::OpenApi;

include!(concat!(env!("OUT_DIR"), "/commands_manifest.rs"));

// ---------------------------------------------------------------------------
// 源码扫描器具：掩码、分块、令牌提取（测试期文本级扫描，ADR-0073 决策 5）
// ---------------------------------------------------------------------------

/// 掩码 Rust 源文本中的注释与字符串/char 字面量：内容替换为等长空白（保留换行
/// 与列位），使令牌扫描只落在真实代码上。与 `check-structure.js` 的 `maskNonCode`
/// 同款规则（行注释、可嵌套块注释、字符串、原始字符串、char 字面量）。
fn mask_non_code(text: &str) -> String {
    let bytes: Vec<char> = text.chars().collect();
    let n = bytes.len();
    let mut out = bytes.clone();
    let blank = |out: &mut Vec<char>, from: usize, to: usize| {
        for k in out.iter_mut().take(to.min(n)).skip(from) {
            if *k != '\n' {
                *k = ' ';
            }
        }
    };
    let mut i = 0;
    while i < n {
        let c = bytes[i];
        if c == '/' && i + 1 < n && bytes[i + 1] == '/' {
            // 行注释（含 /// 与 //!）到行尾
            let end = bytes[i..]
                .iter()
                .position(|&b| b == '\n')
                .map_or(n, |p| i + p);
            blank(&mut out, i, end);
            i = end;
        } else if c == '/' && i + 1 < n && bytes[i + 1] == '*' {
            // 块注释（Rust 可嵌套）
            let mut depth = 1usize;
            let mut j = i + 2;
            while j < n && depth > 0 {
                if j + 1 < n && bytes[j] == '/' && bytes[j + 1] == '*' {
                    depth += 1;
                    j += 2;
                } else if j + 1 < n && bytes[j] == '*' && bytes[j + 1] == '/' {
                    depth -= 1;
                    j += 2;
                } else {
                    j += 1;
                }
            }
            blank(&mut out, i, j);
            i = j;
        } else if c == '"' {
            // 普通字符串：跳过转义对
            let mut j = i + 1;
            while j < n {
                if bytes[j] == '\\' {
                    j += 2;
                } else if bytes[j] == '"' {
                    j += 1;
                    break;
                } else {
                    j += 1;
                }
            }
            blank(&mut out, i, j);
            i = j;
        } else if c == 'r'
            && i + 1 < n
            && (bytes[i + 1] == '"' || (bytes[i + 1] == '#' && i + 2 < n && bytes[i + 2] == '"'))
        {
            // 原始字符串 r"…" / r#"…"#；前一字符为标识符成分时是普通名字，不误伤
            let prev_is_ident = i > 0 && bytes[i - 1].is_alphanumeric() || bytes[i - 1] == '_';
            if prev_is_ident && i > 0 {
                i += 1;
                continue;
            }
            let mut hashes = 0usize;
            let mut j = i + 1;
            while j < n && bytes[j] == '#' {
                hashes += 1;
                j += 1;
            }
            let close: Vec<char> = format!("\"{}", "#".repeat(hashes)).chars().collect();
            let mut end = n;
            let mut k = j + 1;
            while k + close.len() <= n {
                if bytes[k..k + close.len()] == close[..] {
                    end = k + close.len();
                    break;
                }
                k += 1;
            }
            blank(&mut out, i, end);
            i = end;
        } else if c == '\'' {
            // char 字面量 vs 生命周期：有闭引号为字面量，否则是生命周期标注（'a）
            let mut j = i + 1;
            if j < n && bytes[j] == '\\' {
                j += 1;
                if j < n && bytes[j] == '{' {
                    while j < n && bytes[j] != '}' {
                        j += 1;
                    }
                }
                j += 1;
            } else {
                j += 1;
            }
            if j < n && bytes[j] == '\'' {
                blank(&mut out, i, j + 1);
                i = j + 1;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out.into_iter().collect()
}

/// 一个命令 / handler 函数体的扫描单元：锚点属性（`#[tauri::command]` 或
/// `#[utoipa::path(...)]`）起、下一锚点（或文件尾 / 测试模块）止。
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

    // 附加边界：测试模块起点（ai.rs 先例：命令后的 #[cfg(test)] 不属于命令体）。
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

    let mut chunks = Vec::new();
    for (idx, &start) in starts.iter().enumerate() {
        let end = starts
            .get(idx + 1)
            .copied()
            .or_else(|| ends.iter().copied().find(|&e| e > start))
            .unwrap_or(masked_chars.len());
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

/// 块内首个 `pub [async] fn` 的标识符（命令 / handler 函数体的命名约定）。
fn fn_name(masked: &str) -> Option<String> {
    for marker in ["pub async fn ", "pub fn "] {
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

/// 掩码文本中是否出现 `db::write` / `.write(` / 发射调用（反向守门的 bypass 形态；
/// 文本级扫描，别名改写不可达靠评审兜底）。
fn has_bypass_write_or_emit(masked: &str, include_http_emit: bool) -> bool {
    if count_token(masked, "db::write(") > 0 || count_token(masked, ".write(") > 0 {
        return true;
    }
    if count_token(masked, "emit_for(") > 0 || count_token(masked, "emit_all(") > 0 {
        return true;
    }
    include_http_emit && count_token(masked, "emit_after_write(") > 0
}

/// 从原文中提取 `write_entry(` 调用点的首个字符串字面量（span 归因串）。
fn write_entry_span(raw: &str) -> Option<String> {
    let pos = raw.find("write_entry(")? + "write_entry(".len();
    let rest = raw[pos..].trim_start();
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

/// 目录下全部 .rs 文件的源文本（按文件名排序，输出确定）。
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

/// IPC 壳的 `#[tauri::command]` 函数体块（全命令域文件）。
fn ipc_command_chunks() -> Vec<Chunk> {
    let mut chunks = Vec::new();
    for (_, src) in read_sources("src/commands") {
        chunks.extend(split_chunks(&src, "#[tauri::command]"));
    }
    chunks
}

/// 例外白名单（ADR-0073 决策 5）：不经 `write_entry` 的声明写命令，逐个附动机。
/// 这些命令保留身份声明（孤儿身份核对把本表并入声明集）；除白名单登记外，
/// 新写命令一律走 `write_entry`，不新增本表条目。两壳共用本清单：HTTP 壳
/// 无例外（9 个写端点全部迁入写入口，ADR-0073 决策 7）。
const IPC_WRITE_ENTRY_EXCEPTIONS: &[(&str, WriteOp, &str)] = &[
    (
        "create_backup",
        WriteOp::CreateBackup,
        "文件级备份产物（库快照 → zip），非 DB 写：不经 db::write（置脏豁免，ADR-0032），刻意零信号",
    ),
    (
        "restore_backup",
        WriteOp::RestoreBackup,
        "整库恢复路径：不经 db::write（置脏豁免单点，ADR-0032 决策 2），成功后整体重启、零信号",
    ),
    (
        "prune_backups",
        WriteOp::PruneBackups,
        "受管备份文件清理：纯文件操作不经 db::write，壳层自发射 backups-changed（ADR-0044）",
    ),
    (
        "set_auto_backup_enabled",
        WriteOp::SetAutoBackupEnabled,
        "设置 KV 写入（app_settings）：经 settings.rs 单点收口、置脏豁免（ADR-0017/0032）",
    ),
    (
        "set_auto_backup_dir",
        WriteOp::SetAutoBackupDir,
        "设备偏好进程镜像推送 + 首次兜底备份：命令体非 DB 写（兜底备份产物经 AutoBackupDeepPath 发信号）",
    ),
    (
        "submit_data_location_change",
        WriteOp::SubmitDataLocationChange,
        "引导指针文件写入（建连前必须可读的库外配置）：不经 db::write（ADR-0018）",
    ),
    (
        "restore_default_data_location",
        WriteOp::RestoreDefaultDataLocation,
        "引导指针文件写入（同上，ADR-0018）",
    ),
    (
        "set_auto_execution_enabled",
        WriteOp::SetAutoExecutionEnabled,
        "设备级「自动执行」运行时标志镜像推送：纯内存不触 DB（ADR-0042）",
    ),
    (
        "set_log_level",
        WriteOp::SetLogLevel,
        "设置日志等级 KV 写入（app_settings 的 logging.level）：经 settings.rs 单点收口、\
         置脏豁免（ADR-0032/0017），刻意零信号（设置不是账本数据，ADR-0006）",
    ),
    (
        "audit_balance_cache",
        WriteOp::AuditBalanceCache,
        "余额缓存审计修复：派生缓存行直连锁内维护，不置脏不发信号（ADR-0067；置脏口径为已知开放点，不在本票裁决）",
    ),
    (
        "repair_note_pinyin",
        WriteOp::RepairNotePinyin,
        "备注拼音派生列回填：搜索派生数据直连锁内维护，不置脏不发信号（issue #513）",
    ),
    (
        "sync_instruments",
        WriteOp::SyncInstruments,
        "全量同步「发射后不管」：分离线程自推进自发射（ADR-0069 决策 2 保留同步形态），命令体不触 DB",
    ),
];

/// IPC 派生声明表：`write_entry` 调用点提取的（命令, 身份）。
/// 每个调用点块恰有一次调用、恰有一个身份（多身份/零身份即红）。
fn ipc_derived_declarations() -> Vec<(String, WriteOp)> {
    let mut declared = Vec::new();
    for chunk in ipc_command_chunks() {
        let call_sites = count_token(&chunk.masked, "write_entry(");
        if call_sites == 0 {
            continue;
        }
        let identities = write_op_identities(&chunk.masked);
        assert_eq!(
            call_sites, 1,
            "IPC 命令 {} 含 {call_sites} 处 write_entry 调用（应恰为一行调用）",
            chunk.name
        );
        assert_eq!(
            identities.len(),
            1,
            "IPC 命令 {} 的 write_entry 块内身份不唯一：{identities:?}",
            chunk.name
        );
        let ident = identities.into_iter().next().expect("长度已断言为 1");
        let op = parse_write_op(&ident);
        declared.push((chunk.name.clone(), op));
    }
    declared
}

/// 身份标识符 → WriteOp（穷尽清单，漏登变体在孤儿核对处即红）。
fn parse_write_op(ident: &str) -> WriteOp {
    match ident {
        "CreateAccount" => WriteOp::CreateAccount,
        "UpdateAccount" => WriteOp::UpdateAccount,
        "DeleteAccount" => WriteOp::DeleteAccount,
        "CreateCategory" => WriteOp::CreateCategory,
        "UpdateCategory" => WriteOp::UpdateCategory,
        "ReorderCategories" => WriteOp::ReorderCategories,
        "DeleteCategory" => WriteOp::DeleteCategory,
        "CreateMerchant" => WriteOp::CreateMerchant,
        "UpdateMerchant" => WriteOp::UpdateMerchant,
        "DeleteMerchant" => WriteOp::DeleteMerchant,
        "CreateItem" => WriteOp::CreateItem,
        "UpdateItem" => WriteOp::UpdateItem,
        "DisposeItem" => WriteOp::DisposeItem,
        "DeleteItem" => WriteOp::DeleteItem,
        "CreatePolicy" => WriteOp::CreatePolicy,
        "UpdatePolicy" => WriteOp::UpdatePolicy,
        "DeletePolicy" => WriteOp::DeletePolicy,
        "CreatePhysicalAsset" => WriteOp::CreatePhysicalAsset,
        "UpdatePhysicalAsset" => WriteOp::UpdatePhysicalAsset,
        "UpdatePhysicalAssetValuation" => WriteOp::UpdatePhysicalAssetValuation,
        "DisposePhysicalAsset" => WriteOp::DisposePhysicalAsset,
        "DeletePhysicalAsset" => WriteOp::DeletePhysicalAsset,
        "AdjustAccountBalance" => WriteOp::AdjustAccountBalance,
        "AuditBalanceCache" => WriteOp::AuditBalanceCache,
        "RepairNotePinyin" => WriteOp::RepairNotePinyin,
        "SyncHoldingPrices" => WriteOp::SyncHoldingPrices,
        "SyncInstruments" => WriteOp::SyncInstruments,
        "AddFundByCode" => WriteOp::AddFundByCode,
        "RecordManualPrice" => WriteOp::RecordManualPrice,
        "CreateInstrument" => WriteOp::CreateInstrument,
        "DeleteInstrument" => WriteOp::DeleteInstrument,
        "CreateMarketPrice" => WriteOp::CreateMarketPrice,
        "CreateExchangeRate" => WriteOp::CreateExchangeRate,
        "CreateBackup" => WriteOp::CreateBackup,
        "PruneBackups" => WriteOp::PruneBackups,
        "RestoreBackup" => WriteOp::RestoreBackup,
        "AutoBackupDeepPath" => WriteOp::AutoBackupDeepPath,
        "CreateTransaction" => WriteOp::CreateTransaction,
        "BatchCreateTransactions" => WriteOp::BatchCreateTransactions,
        "UpdateTransaction" => WriteOp::UpdateTransaction,
        "DeleteTransaction" => WriteOp::DeleteTransaction,
        "ExecuteScheduledOccurrence" => WriteOp::ExecuteScheduledOccurrence,
        "ExpandScheduledOccurrences" => WriteOp::ExpandScheduledOccurrences,
        "CreateBudget" => WriteOp::CreateBudget,
        "UpdateBudget" => WriteOp::UpdateBudget,
        "DeleteBudget" => WriteOp::DeleteBudget,
        "CreateScheduledTransaction" => WriteOp::CreateScheduledTransaction,
        "UpdateScheduledTransactionStatus" => WriteOp::UpdateScheduledTransactionStatus,
        "UpdateScheduledSubscription" => WriteOp::UpdateScheduledSubscription,
        "SetAutoBackupEnabled" => WriteOp::SetAutoBackupEnabled,
        "SetAutoBackupDir" => WriteOp::SetAutoBackupDir,
        "SetAutoExecutionEnabled" => WriteOp::SetAutoExecutionEnabled,
        "SubmitDataLocationChange" => WriteOp::SubmitDataLocationChange,
        "RestoreDefaultDataLocation" => WriteOp::RestoreDefaultDataLocation,
        other => panic!(
            "未知 WriteOp 变体标识符 {other}——enum 新增变体须同步 parse_write_op 与 WriteOp::ALL"
        ),
    }
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
        let call_sites = count_token(&chunk.masked, "write_entry(");
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
        declared.push((endpoint_key, parse_write_op(&ident)));
    }
    declared
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

/// 两壳声明（IPC 派生 + 白名单、HTTP 派生）的 `Some` 身份并集（跨壳共享同一
/// `WriteOp` 合法，如账户删除命令与 `DELETE /api/v1/accounts/{id}`）。
fn declared_ops() -> HashSet<WriteOp> {
    ipc_derived_declarations()
        .iter()
        .map(|(_, op)| *op)
        .chain(IPC_WRITE_ENTRY_EXCEPTIONS.iter().map(|(_, op, _)| *op))
        .chain(http_derived_declarations().iter().map(|(_, op)| *op))
        .collect()
}

// ---------------------------------------------------------------------------
// 守门一：孤儿身份——每个被映射身份至少被一壳声明（ADR-0044 决策 3 原样保留）
// ---------------------------------------------------------------------------

/// 「映射未声明」反向核对（#335 验收项原样保留）：`signals_for` 映射表中的每个
/// 写操作身份（穷尽 match = 全部变体）都必须被至少一个壳声明——`write_entry`
/// 调用点或例外白名单；特例条目 `AutoBackupDeepPath` 豁免（生产者清单登记，
/// 发射走镜像句柄，不经壳层）。
#[test]
fn every_mapped_write_op_is_declared_by_some_shell() {
    let declared = declared_ops();
    for op in WriteOp::ALL {
        if matches!(op, WriteOp::AutoBackupDeepPath) {
            continue;
        }
        assert!(
            declared.contains(&op),
            "写操作身份 {op:?} 已在 signals_for 映射，却未被任何壳声明——\
             新写命令忘了经 write_entry 接线（或在例外白名单登记）或漏登 WriteOp::ALL"
        );
    }
}

// ---------------------------------------------------------------------------
// 守门二：单壳身份唯一（#335 原样保留，声明源改为扫描派生 + 白名单）
// ---------------------------------------------------------------------------

/// 同一壳里两个命令/端点声明同一 `WriteOp` 视为复制粘贴错误（跨壳共享合法——
/// 两壳写同一数据本就共享身份）。
#[test]
fn write_op_is_declared_at_most_once_per_shell() {
    for (shell, declared) in [
        (
            "IPC",
            ipc_derived_declarations()
                .into_iter()
                .chain(
                    IPC_WRITE_ENTRY_EXCEPTIONS
                        .iter()
                        .map(|(name, op, _)| ((*name).to_string(), *op)),
                )
                .collect::<Vec<_>>(),
        ),
        ("HTTP", http_derived_declarations()),
    ] {
        let mut seen: HashMap<WriteOp, &str> = HashMap::new();
        for (name, op) in &declared {
            let first = seen.insert(*op, name);
            assert!(
                first.is_none(),
                "{shell} 壳中 {op:?} 被重复声明：{} 与 {name}",
                first.unwrap_or("")
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 守门三：声明身份必被映射（编译期穷尽 match 的测试面显式登记，#335 原样保留）
// ---------------------------------------------------------------------------

/// 每个声明身份经 `signals_for` 的返回值必须落在四个已知信号集之内——映射行
/// 不缺、形状不漂。
#[test]
fn every_declared_write_op_is_mapped() {
    let ipc: Vec<(String, WriteOp)> = ipc_derived_declarations();
    let mut declared: Vec<(&str, WriteOp)> =
        ipc.iter().map(|(name, op)| (name.as_str(), *op)).collect();
    declared.extend(
        IPC_WRITE_ENTRY_EXCEPTIONS
            .iter()
            .map(|(name, op, _)| (*name, *op)),
    );
    let http: Vec<(String, WriteOp)> = http_derived_declarations();
    declared.extend(http.iter().map(|(key, op)| (key.as_str(), *op)));
    for (name, op) in &declared {
        let signals = signals_for(*op, WriteEvidence::None);
        let known = matches!(signals, &[])
            || matches!(signals, &[Signal::LedgerChanged])
            || matches!(signals, &[Signal::PricesChanged])
            || matches!(signals, &[Signal::BackupsChanged]);
        assert!(
            known,
            "声明身份 {op:?}（{name}）经 signals_for 返回未知信号集 {signals:?}——\
             映射行缺位或形状漂移"
        );
    }
}

// ---------------------------------------------------------------------------
// 守门四：IPC 反向守门 + 归因串核对 + 注册面耦合（ADR-0073 决策 5）
// ---------------------------------------------------------------------------

/// IPC 反向守门：命令体内出现 `db::write` / `.write(` / 发射调用而无
/// `write_entry` 即红，「绕开入口写库」的回归被兜住；不经入口的声明写命令
/// 必须在例外白名单登记。
#[test]
fn ipc_write_calls_must_go_through_write_entry() {
    let exceptions: HashSet<&str> = IPC_WRITE_ENTRY_EXCEPTIONS
        .iter()
        .map(|(n, _, _)| *n)
        .collect();
    for chunk in ipc_command_chunks() {
        let has_write_entry = count_token(&chunk.masked, "write_entry(") > 0;
        if has_write_entry || !has_bypass_write_or_emit(&chunk.masked, false) {
            continue;
        }
        assert!(
            exceptions.contains(chunk.name.as_str()),
            "IPC 命令 {} 的函数体出现 db::write / 发射调用却未走 write_entry——\
             绕开统一写入口的回归（ADR-0073）；如确属不经入口的声明写命令，\
             在例外白名单登记并附动机",
            chunk.name
        );
    }
}

/// IPC 归因串核对（ADR-0073 决策 4 顺带）：`write_entry` 归因串 == 命令名，
/// SQL 耗时日志逐字节不漂（ADR-0009 / ADR-0068）。
#[test]
fn ipc_write_entry_span_matches_command_name() {
    for chunk in ipc_command_chunks() {
        if count_token(&chunk.masked, "write_entry(") == 0 {
            continue;
        }
        let span = write_entry_span(&chunk.raw).unwrap_or_else(|| {
            panic!(
                "IPC 命令 {} 的 write_entry 调用缺 span 归因串字面量",
                chunk.name
            )
        });
        assert_eq!(
            span, chunk.name,
            "IPC 命令 {} 的 write_entry 归因串与命令名漂移",
            chunk.name
        );
    }
}

/// 启动期两扇门禁白名单（锁定 `LOCKED_ALLOWED_COMMANDS` / 启动失败
/// `BOOT_FAILURE_ALLOWED_COMMANDS`，lib.rs）中的每个命令名都必须真实注册
/// （build.rs 生成的 ADR-0047 真源）：白名单字符串漂移（命令改名后白名单
/// 残留旧名）在此即红，而非运行期静默放行失败。
#[test]
fn startup_gate_allowlists_only_contain_registered_commands() {
    let registry: HashSet<&str> = IPC_COMMAND_MANIFEST.iter().copied().collect();
    for name in crate::LOCKED_ALLOWED_COMMANDS
        .iter()
        .chain(crate::BOOT_FAILURE_ALLOWED_COMMANDS.iter())
    {
        assert!(
            registry.contains(name),
            "门禁白名单命令 {name} 不在命令注册清单上——白名单漂移（命令已改名或删除）"
        );
    }
}

/// IPC 派生命令必须都在注册清单上（build.rs 生成的 ADR-0047 真源）：扫描器具
/// 自身的命名提取漂移（误把非命令 fn 当命令）在此即红。
#[test]
fn ipc_derived_declarations_are_registered_commands() {
    let registry: HashSet<&str> = IPC_COMMAND_MANIFEST.iter().copied().collect();
    for (name, _) in ipc_derived_declarations() {
        assert!(
            registry.contains(name.as_str()),
            "IPC 派生声明 {name} 不在命令注册清单上——扫描提取漂移或注册缺失"
        );
    }
}

// ---------------------------------------------------------------------------
// 守门五：HTTP 反向守门 + 归因串/端点键核对 + 契约面耦合（ADR-0073 决策 5）
// ---------------------------------------------------------------------------

/// HTTP 反向守门：handler 函数体内出现 `db::write` / `.write(` / 发射调用而无
/// `write_entry` 即红，「绕开入口写库」的回归被兜住。HTTP 壳无例外白名单——
/// 9 个写端点全部经统一写入口（ADR-0073 决策 7）。
#[test]
fn http_write_calls_must_go_through_write_entry() {
    for chunk in http_handler_chunks() {
        let has_write_entry = count_token(&chunk.masked, "write_entry(") > 0;
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
        if count_token(&chunk.masked, "write_entry(") == 0 {
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
