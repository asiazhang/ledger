use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::api_server::ApiDoc;
use crate::test_support::scan::{mask_non_code, matching_brace_end};
use ledger_infra::signals::{Signal, WriteEvidence, WriteOp, signals_for};
use utoipa::OpenApi;

include!(concat!(env!("OUT_DIR"), "/commands_manifest.rs"));

struct Chunk {
    name: String,
    masked: String,
    raw: String,
}

fn split_chunks(source: &str, anchor: &str) -> Vec<Chunk> {
    let masked = mask_non_code(source);
    let masked_chars: Vec<char> = masked.chars().collect();
    let source_chars: Vec<char> = source.chars().collect();
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
        let next = starts
            .get(idx + 1)
            .copied()
            .or_else(|| ends.iter().copied().find(|&e| e > start))
            .unwrap_or(masked_chars.len());
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

const FN_MARKERS: [&str; 2] = ["pub async fn ", "pub fn "];

fn fn_body_end(masked: &str) -> Option<usize> {
    let fn_pos = FN_MARKERS
        .iter()
        .filter_map(|marker| masked.find(marker))
        .min()?;
    let open = masked[fn_pos..].find('{')? + fn_pos;
    matching_brace_end(masked, open)
}

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

fn count_token(masked: &str, token: &str) -> usize {
    masked.matches(token).count()
}

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

fn has_bypass_write_or_emit(masked: &str, include_http_emit: bool) -> bool {
    if count_token(masked, "db::write(") > 0 || count_token(masked, ".write(") > 0 {
        return true;
    }
    if count_token(masked, "emit_for(") > 0 || count_token(masked, "emit_all(") > 0 {
        return true;
    }
    include_http_emit && count_token(masked, "emit_after_write(") > 0
}

const WRITE_ENTRY_CALL_TOKENS: [&str; 3] = [
    "write_entry(",
    "write_entry_segmented(",
    "write_entry_segmented_async(",
];

fn write_entry_call_count(masked: &str) -> usize {
    WRITE_ENTRY_CALL_TOKENS
        .iter()
        .map(|token| count_token(masked, token))
        .sum()
}

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

fn ipc_command_chunks() -> Vec<Chunk> {
    let mut chunks = Vec::new();
    for (_, src) in read_sources("src/commands") {
        chunks.extend(split_chunks(&src, "#[tauri::command]"));
    }
    chunks
}

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
        "set_auto_update_enabled",
        WriteOp::SetAutoUpdateEnabled,
        "设备级「自动检查更新」开关运行时镜像推送：纯内存不触 DB（ADR-0124 决策 8 / \
         issue #1429，ADR-0017 修订记录例外先例；与 set_auto_execution_enabled 同形）",
    ),
    (
        "set_log_level",
        WriteOp::SetLogLevel,
        "设置日志等级 KV 写入（app_settings 的 logging.level）：经 settings.rs 单点收口、\
         置脏豁免（ADR-0032/0017），刻意零信号（设置不是账本数据，ADR-0006）",
    ),
    (
        "set_sync_channel_config",
        WriteOp::SetSyncChannelConfig,
        "多端同步通道配置（issue #862 / #1217 / #1221）：通道凭据（S3 兼容对象存储单后端）写 \
         app_settings，经 settings.rs 单点收口、置脏豁免（ADR-0032/0017），刻意零信号——本机\
         设备配置不同步（同步边界 SyncBoundary）；地址形态门与凭据构库校验（域侧 \
         `build_channel`）无 DB 写，不经写入口",
    ),
    (
        "audit_balance_cache",
        WriteOp::AuditBalanceCache,
        "余额缓存审计修复：派生缓存行直连锁内维护，不置脏不发信号（ADR-0067；置脏口径为已知开放点，不在本票裁决）",
    ),
];

fn ipc_derived_declarations() -> Vec<(String, WriteOp)> {
    let mut declared = Vec::new();
    for chunk in ipc_command_chunks() {
        let call_sites = write_entry_call_count(&chunk.masked);
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
        let op = resolve_write_op(&ident);
        declared.push((chunk.name.clone(), op));
    }
    declared
}

fn resolve_write_op(ident: &str) -> WriteOp {
    WriteOp::from_ident(ident).unwrap_or_else(|| {
        panic!(
            "扫描提取漂移：{ident} 不是 WriteOp 变体标识符——\
             信号守门扫描器具与 enum 本体不一致"
        )
    })
}

fn http_handler_chunks() -> Vec<Chunk> {
    let mut chunks = Vec::new();
    for (_, src) in read_sources("src/api_server/handlers") {
        chunks.extend(split_chunks(&src, "#[utoipa::path("));
    }
    chunks
}

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
        declared.push((endpoint_key, resolve_write_op(&ident)));
    }
    declared
}

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

fn declared_ops() -> HashSet<WriteOp> {
    ipc_derived_declarations()
        .iter()
        .map(|(_, op)| *op)
        .chain(IPC_WRITE_ENTRY_EXCEPTIONS.iter().map(|(_, op, _)| *op))
        .chain(http_derived_declarations().iter().map(|(_, op)| *op))
        .collect()
}

#[test]
fn every_mapped_write_op_is_declared_by_some_shell() {
    let declared = declared_ops();
    for &op in WriteOp::ALL {
        if matches!(op, WriteOp::AutoBackupDeepPath) {
            continue;
        }
        assert!(
            declared.contains(&op),
            "写操作身份 {op:?} 已在 signals_for 映射，却未被任何壳声明——\
             新写命令忘了经 write_entry 接线（或在例外白名单登记）"
        );
    }
}

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

#[test]
fn ipc_write_calls_must_go_through_write_entry() {
    let exceptions: HashSet<&str> = IPC_WRITE_ENTRY_EXCEPTIONS
        .iter()
        .map(|(n, _, _)| *n)
        .collect();
    for chunk in ipc_command_chunks() {
        let has_write_entry = write_entry_call_count(&chunk.masked) > 0;
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

#[test]
fn ipc_write_entry_span_matches_command_name() {
    for chunk in ipc_command_chunks() {
        if write_entry_call_count(&chunk.masked) == 0 {
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

#[test]
fn ipc_write_entry_exceptions_only_contain_registered_commands() {
    let registry: HashSet<&str> = IPC_COMMAND_MANIFEST.iter().copied().collect();
    for (name, _, _) in IPC_WRITE_ENTRY_EXCEPTIONS {
        assert!(
            registry.contains(name),
            "例外白名单命令 {name} 不在命令注册清单上——死条目（命令已改名或删除）"
        );
    }
}

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

pub(crate) const STANDARD_LOCK_LINE: &str = ".lock().map_err(|e| AppError::Db(e.to_string()))";

const IPC_READ_ENTRY_EXCEPTIONS: &[(&str, &str)] = &[
    (
        "create_backup",
        "文件级备份产物（库快照 → zip）：写侧白名单命令（ADR-0073），快照须在连接锁内取一致视图，非读闭包语义",
    ),
    (
        "restore_backup",
        "整库恢复：分支锁——主连接可选（issue #601 启动失败接管现场无连接不持锁），锁形态超出 read_entry 单锁形状",
    ),
    (
        "set_auto_backup_enabled",
        "设置 KV 写入经 settings 单点收口（写侧白名单，ADR-0032/0017）：写命令非读闭包",
    ),
    (
        "set_log_level",
        "设置 KV 写入经 settings 单点收口（写侧白名单，ADR-0032/0017）：写命令非读闭包",
    ),
    (
        "set_sync_channel_config",
        "通道配置写 app_settings（写侧白名单）：锁前凭据构库校验、锁后 settings::set，非读闭包形状",
    ),
    (
        "get_sync_status",
        "锁内文件探测（probe_file_kind 判库加密形态）：持锁窗口刻意含文件 IO，直连保持显式（ADR-0104 形状 C）",
    ),
    (
        "bootstrap_sync_from_channel",
        "段接缝实现（issue #1285）：整库换入需 &mut Connection，段1 前置守卫与段2 复验换手写短锁、\
         整库快照下载在锁外；配置读取已走 read_entry，非整段读闭包形状（ADR-0104 形状 C）",
    ),
];

fn has_handwritten_lock(masked: &str) -> bool {
    count_token(masked, STANDARD_LOCK_LINE) > 0
}

#[test]
fn ipc_lock_ritual_must_go_through_read_entry() {
    let exceptions: HashSet<&str> = IPC_READ_ENTRY_EXCEPTIONS.iter().map(|(n, _)| *n).collect();
    for chunk in ipc_command_chunks() {
        let through_entry = count_token(&chunk.masked, "read_entry(") > 0
            || write_entry_call_count(&chunk.masked) > 0;
        if through_entry || !has_handwritten_lock(&chunk.masked) {
            continue;
        }
        assert!(
            exceptions.contains(chunk.name.as_str()),
            "IPC 命令 {} 的函数体出现手写标准锁行却未走 read_entry——\
             锁仪式回潮（ADR-0104 决策 6）；如确属不经读入口的命令，\
             在 IPC_READ_ENTRY_EXCEPTIONS 豁免清单登记并附动机",
            chunk.name
        );
    }
}

#[test]
fn http_lock_ritual_must_go_through_read_entry() {
    for chunk in http_handler_chunks() {
        let through_entry = count_token(&chunk.masked, "read_entry(") > 0
            || write_entry_call_count(&chunk.masked) > 0;
        if through_entry || !has_handwritten_lock(&chunk.masked) {
            continue;
        }
        panic!(
            "HTTP handler {} 的函数体出现手写标准锁行却未走 read_entry——\
             锁仪式回潮（ADR-0104 决策 6）",
            chunk.name
        );
    }
}

#[test]
fn shell_never_constructs_db_connections_directly() {
    for (name, src) in read_sources("src") {
        let masked = mask_non_code(&src);
        for token in ["Connection::open(", "open_with_flags("] {
            assert_eq!(
                count_token(&masked, token),
                0,
                "壳层文件 {name} 出现连接构造器 {token}——建连必须经 db 收口函数，
                 不得手拷第二条连接绕开建连收尾单点（ADR-0117 决策 4）"
            );
        }
    }
}

#[test]
fn shell_connection_swaps_stay_in_whitelisted_files() {
    const SWAP_FILE_WHITELIST: [&str; 3] = ["boot.rs", "encryption.rs", "backup.rs"];
    const SWAP_PRIMITIVE_TOKENS: [&str; 4] = [
        "swap_pair(",
        "replace_read_conn(",
        "replace_read_conn_slot(",
        "placeholderize_read_conn(",
    ];
    for root in ["src", "src/commands"] {
        for (name, src) in read_sources(root) {
            let masked = mask_non_code(&src);
            let hits: Vec<&str> = SWAP_PRIMITIVE_TOKENS
                .iter()
                .filter(|token| count_token(&masked, token) > 0)
                .copied()
                .collect();
            if SWAP_FILE_WHITELIST.contains(&name.as_str()) {
                assert!(
                    !hits.is_empty(),
                    "换装白名单文件 {name} 已无任何换装原语调用——死条目
                     （文件已改名或换装点已迁走），请同步更新白名单"
                );
            } else {
                assert!(
                    hits.is_empty(),
                    "壳层文件 {name} 出现换装原语调用 {hits:?}——换入/换出必须收口
                     既有换连单点（boot.rs 引导换入、encryption.rs 解锁/重置编排、
                     backup.rs 恢复槽级换出豁免），不得手搓新副本（ADR-0117 决策 4）"
                );
            }
        }
    }
}

#[test]
fn ipc_read_entry_exceptions_only_contain_registered_commands() {
    let registry: HashSet<&str> = IPC_COMMAND_MANIFEST.iter().copied().collect();
    for (name, _) in IPC_READ_ENTRY_EXCEPTIONS {
        assert!(
            registry.contains(name),
            "读侧豁免清单命令 {name} 不在命令注册清单上——死条目（命令已改名或删除）"
        );
    }
}
