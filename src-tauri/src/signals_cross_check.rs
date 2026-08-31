//! 信号交叉核对测试（ADR-0044 决策 3 / issue #335）：信号映射单点 × 两壳声明表
//! 双向核对。仅测试可见（lib.rs 以 `#[cfg(test)]` 挂载），生产构建不含本模块。
//!
//! 兜底的两类静默漏发（编译期穷尽 match 抓不到的形状）：
//! - **新写命令忘了声明身份**：命令进了注册面（IPC 清单 / OpenAPI 端点集）但
//!   声明表无行——注册面 × 声明表双向比对即红；
//! - **映射有行但无壳接线**：`WriteOp` 变体在 `signals_for` 有映射行，却没有任何
//!   壳声明它（`WriteOp::ALL` 遍历 × 声明表反向核对即红）——特例条目
//!   `AutoBackupDeepPath` 除外（登记生产者清单、刻意不做命令键）。
//!
//! 「声明了但映射缺行」方向由 `signals_for` 的编译期穷尽 match 兜底（enum 新增
//! 变体漏改映射编译即错）；测试仍对每个声明身份复核映射返回值形状，把该方向
//! 的保障显式登记在测试面。

use std::collections::{HashMap, HashSet};

use crate::api_server::{ApiDoc, HTTP_ENDPOINT_WRITE_OPS};
use crate::commands::IPC_COMMAND_WRITE_OPS;
use crate::signals::{Signal, WriteEvidence, WriteOp, signals_for};
use utoipa::OpenApi;

include!(concat!(env!("OUT_DIR"), "/commands_manifest.rs"));

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

/// 两壳声明表的 `Some` 身份并集（跨壳共享同一 `WriteOp` 合法，如账户删除命令与
/// `DELETE /api/v1/accounts/{id}`）。
fn declared_ops() -> HashSet<WriteOp> {
    IPC_COMMAND_WRITE_OPS
        .iter()
        .chain(HTTP_ENDPOINT_WRITE_OPS.iter())
        .filter_map(|(_, op)| *op)
        .collect()
}

/// 「映射未声明」反向核对（#335 验收项）：`signals_for` 映射表中的每个写操作身份
/// （穷尽 match = 全部变体）都必须被至少一个壳的声明表声明；特例条目
/// `AutoBackupDeepPath` 豁免（生产者清单登记，发射走镜像句柄，不经壳层）。
#[test]
fn every_mapped_write_op_is_declared_by_some_shell() {
    let declared = declared_ops();
    for op in WriteOp::ALL {
        if matches!(op, WriteOp::AutoBackupDeepPath) {
            continue;
        }
        assert!(
            declared.contains(&op),
            "写操作身份 {op:?} 已在 signals_for 映射，却未被任何壳的声明表声明——\
             新写命令忘了接线（补 IPC_COMMAND_WRITE_OPS / HTTP_ENDPOINT_WRITE_OPS）\
             或漏登 WriteOp::ALL"
        );
    }
}

/// IPC 注册面 × 声明表双向比对（#335 验收项）：新命令漏声明、表键漂移
/// （命令改名未同步 / 手误）都使两侧集合不再互等。
#[test]
fn ipc_declaration_table_matches_registry_exactly() {
    let mut declared: Vec<&str> = IPC_COMMAND_WRITE_OPS
        .iter()
        .map(|(name, _)| *name)
        .collect();
    declared.sort_unstable();
    let mut registry: Vec<&str> = IPC_COMMAND_MANIFEST.to_vec();
    registry.sort_unstable();
    let missing: Vec<&&str> = registry.iter().filter(|n| !declared.contains(n)).collect();
    let stale: Vec<&&str> = declared.iter().filter(|n| !registry.contains(n)).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "IPC 声明表与注册清单不互等——漏声明（清单有表无）：{missing:?}；\
         表键漂移（表有清单无）：{stale:?}"
    );
}

/// HTTP 注册面 × 声明表双向比对（#335 验收项）：以 OpenAPI 契约自描述为端点集
/// 真源——「新写端点忘了声明」「表键漂移」「handler 有注解但契约漏记」都即红。
#[test]
fn http_declaration_table_matches_openapi_exactly() {
    let declared: HashSet<&str> = HTTP_ENDPOINT_WRITE_OPS
        .iter()
        .map(|(key, _)| *key)
        .collect();
    let endpoints = openapi_endpoints();
    let missing: Vec<&String> = endpoints
        .iter()
        .filter(|e| !declared.contains(e.as_str()))
        .collect();
    let stale: Vec<&&str> = declared
        .iter()
        .filter(|k| !endpoints.contains(**k))
        .collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "HTTP 声明表与 OpenAPI 端点集不互等——漏声明（契约有表无）：{missing:?}；\
         表键漂移（表有契约无）：{stale:?}"
    );
}

/// 「声明未映射」方向复核（#335 验收项；编译期穷尽 match 是第一道兜底，本测试
/// 把保障显式登记在测试面）：每个声明身份经 `signals_for` 的返回值必须落在四个
/// 已知信号集之内——映射行不缺、形状不漂。
#[test]
fn every_declared_write_op_is_mapped() {
    for (shell, (name, op)) in IPC_COMMAND_WRITE_OPS
        .iter()
        .map(|(n, o)| ("IPC", (n, o)))
        .chain(
            HTTP_ENDPOINT_WRITE_OPS
                .iter()
                .map(|(k, o)| ("HTTP", (k, o))),
        )
    {
        let Some(op) = op else { continue };
        let signals = signals_for(*op, WriteEvidence::None);
        let known = matches!(signals, &[])
            || matches!(signals, &[Signal::LedgerChanged])
            || matches!(signals, &[Signal::PricesChanged])
            || matches!(signals, &[Signal::BackupsChanged]);
        assert!(
            known,
            "{shell} 壳声明的身份 {op:?}（{name}）经 signals_for 返回未知信号集 {signals:?}——\
             映射行缺位或形状漂移"
        );
    }
}

/// 单壳内身份唯一：同一壳里两个命令/端点声明同一 `WriteOp` 视为复制粘贴错误
/// （跨壳共享合法——两壳写同一数据本就共享身份）。
#[test]
fn write_op_is_declared_at_most_once_per_shell() {
    for (shell, table) in [
        ("IPC", IPC_COMMAND_WRITE_OPS),
        ("HTTP", HTTP_ENDPOINT_WRITE_OPS),
    ] {
        let mut seen: HashMap<WriteOp, &str> = HashMap::new();
        for (name, op) in table {
            let Some(op) = op else { continue };
            let first = seen.insert(*op, name);
            assert!(
                first.is_none(),
                "{shell} 壳中 {op:?} 被重复声明：{} 与 {name}",
                first.unwrap_or("")
            );
        }
    }
}
