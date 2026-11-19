#!/usr/bin/env -S uv run --quiet
# 基础设施账本数据表 DML 禁令（issue #1135 / ADR-0111 决策 1、决策 5；承接
# ADR-0071「余额与净资产口径迁出基础设施」的守门意图）：
# 基础设施（ledger-infra crate）只承诺「不定义账本数据的口径与规则」——交易、
# 账户余额、净资产、预算、投资等账本数据的语义与算术一律归域，基础设施不得对
# 账本数据表执行 DML（INSERT / UPDATE / DELETE）。
#
# 账本数据表判定（默认拒绝，防新表绕过）：表清单从迁移链提取（migrations/*.sql
# 与 db/migrate.rs 的 CREATE TABLE 全量），除「机制表」（MECHANISM_TABLES，逐条
# 附理由显式登记——设置 KV 归口与多端同步协议表，非账本数据）外全部视为账本
# 数据表。迁移链新表进场未登记机制表即按账本数据对待，基础设施对它执行 DML
# 必然变红——新表的归类被迫显式化，不靠沉默放行。
#
# 免扫范围（明示理由，不靠沉默放行；ADR-0111 决策 5「迁移与 schema 守卫免扫」）：
# - 迁移文件本体（migrations/*.sql，不在扫描根）与迁移链 db/migrate.rs——
#   schema 管理是迁移职责；
# - schema 守卫 db/schema_guard.rs——启动期 schema 漂移校验职责（ADR-0100）。
# 免扫条目 fail loud：文件消失/改名即红（清单漂移），不放任守门空转。
#
# 测试豁免（ADR-0056 决策 5，与守门家族同款）：外挂测试模块/目录（tests.rs
# 文件、tests/ 目录）整文件豁免——测试侧业务表直置归公开写入口纪律辖域
# （check-test-support.ts）。内联 #[cfg(test)] 块不豁免（与家族一致，更严）：
# 存量内联夹具的命中经 REGISTERED_EXCEPTIONS 逐条登记（文件 + 预期命中数 +
# 动机），实际命中与登记数严格相等校验——多出（未登记新直置）、少掉（例外已
# 收敛）都红，与「例外显式登记在案，防止被无意复制」同哲学（ADR-0073 例外
# 白名单纪律）。
#
# 扫描边界（文本级）：SQL 就住在字符串字面量里，掩码只抹注释、保留字符串
# （mask_non_code(…, True)，与 check-structure 原生事务语句禁令同款）；
# 注释里的形态不计数。命中形态覆盖 `INSERT [OR x] INTO` / `REPLACE INTO` /
# `UPDATE` / `DELETE FROM`，含 `main.` 库名前缀限定（`main.transactions` 同样
# 命中）。已知文本不可达逃逸形态（靠评审兜底）：经变量拼接的动态表名、
# `UPDATE … SET … FROM <表>` 的 FROM 侧（只读不属 DML 靶）、宏拼接的语句形态。
# 扫描根 = `crates/infra/src`（INFRA_SRC_REL 单一来源），拆 crate 后路径随清单走。
#
# Python 化 + uv 运行时（issue #2174 / ADR-0083 修订注记）：类型经
# scripts/check_scripts_types.py（mypy --strict）门槛检查；调用方式
# `uv run scripts/check_infra_dml.py`。
# 默认校验本仓库；测试可传位置参数指向夹具：uv run scripts/check_infra_dml.py [src-app-dir]
# 包装测试 scripts/test_check_infra_dml.py（#1158 归位）。
# 行号定位与目录遍历消费 gate_primitives.py（#1680 库归库、门归门）导出的家族共享原语
# （line_at / walk_text_files，issue #1625）；禁令形态与豁免面属本守门政策，自持。

from __future__ import annotations

import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import NoReturn

from check_structure import INFRA_SRC_REL
from gate_primitives import (
    RUST_EXTENSIONS,
    WalkTextOptions,
    WalkedFile,
    line_at,
    mask_non_code,
    read_dir_entries,
    walk_text_files,
)

# 扫描根相对 src-app 的路径：单一事实源 = check_structure 的 INFRA_SRC_REL
# （TS 源同款导入收敛，Python 移植保持同形）。

DEFAULT_SRC_APP = str(Path(__file__).resolve().parent.parent / "src-app")


def fail(message: str) -> NoReturn:
    print(f"✗ 基础设施账本数据表 DML 禁令：{message}", file=sys.stderr)
    sys.exit(1)


# 机制表条目：非账本数据的基础设施机制表，逐条附理由（显式登记，不靠沉默放行）
@dataclass(frozen=True)
class MechanismTable:
    table: str
    why: str


# 机制表（非账本数据）：迁移链全量表清单扣除本清单即禁 DML 账本数据表集合
# （默认拒绝——新表未登记机制表即按账本数据对待）。每条附理由留痕。
MECHANISM_TABLES: tuple[MechanismTable, ...] = (
    MechanismTable(
        table="app_settings",
        why="设置 KV 归口（crate::settings 单点收口，ADR-0111 共享接缝）——设置非账本数据",
    ),
    MechanismTable(
        table="sync_device",
        why="多端同步协议表（协议 crate 唯一 SQL 收口，#1089 / ADR-0091）——同步机制非账本数据",
    ),
    MechanismTable(
        table="sync_ops",
        why="多端同步协议表（协议 crate 唯一 SQL 收口，#1089 / ADR-0091）——同步机制非账本数据",
    ),
    MechanismTable(
        table="sync_parked_ops",
        why="多端同步协议表（协议 crate 唯一 SQL 收口，#1089 / ADR-0091）——同步机制非账本数据",
    ),
    MechanismTable(
        table="sync_stream_positions",
        why="多端同步协议表（协议 crate 唯一 SQL 收口，#1089 / issue #857）——同步机制非账本数据",
    ),
)


# 免扫文件条目：按职责免扫，逐条附理由（明示，不靠沉默放行）
@dataclass(frozen=True)
class ExemptFile:
    file: str
    why: str


# 免扫文件（相对 `crates/infra/src`，ADR-0111 决策 5）：迁移与 schema 守卫按其
# 职责免扫。fail loud：登记文件消失即红（清单漂移），不放任守门空转。
EXEMPT_FILES: tuple[ExemptFile, ...] = (
    ExemptFile(
        file="db/migrate.rs",
        why="迁移链本体：schema 管理是迁移职责（ADR-0111 决策 5「迁移文件免扫」）",
    ),
    ExemptFile(
        file="db/schema_guard.rs",
        why="schema 漂移守卫本体：启动期 schema 校验职责（ADR-0100 / ADR-0111 决策 5）",
    ),
)


# 已登记例外条目：内联 cfg(test) 测试夹具的存量账本表 DML 命中
@dataclass(frozen=True)
class RegisteredException:
    file: str
    count: int
    why: str


# 已登记例外（严格相等校验）：内联 #[cfg(test)] 不豁免（家族一致），存量内联
# 夹具命中逐条登记——实际命中 ≠ 登记数即红（漂移），命中清零也红（例外已
# 收敛，登记条目应删除）；登记文件消失（改名/搬迁）同样红（清单漂移，与
# EXEMPT_FILES 同纪律）。
#
# #1108 起当前为空：唯一例外 shell_support/write_entry.rs 随壳机制迁出根包
# 退役——本禁令辖基础设施 crate，壳层统一写入口写账本表是其本职，不在辖域。
REGISTERED_EXCEPTIONS: tuple[RegisteredException, ...] = ()


# 迁移链 CREATE TABLE 提取形态（含 VIRTUAL / IF NOT EXISTS / 引号包裹 /
# 库名前缀限定——`CREATE TABLE main.foo` 取末段标识符 foo 入清单）
CREATE_TABLE_RE = re.compile(
    r"\bCREATE\s+(?:VIRTUAL\s+)?TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?"
    r'(?:["\'`]?\w+["\'`]?\s*\.\s*)?["\'`]?(\w+)',
    re.IGNORECASE,
)


# 账本数据表判定单一来源：从迁移链（migrations/*.sql + 迁移链定义
# db/migrate.rs）提取 CREATE TABLE 全量清单，扣除机制表即禁 DML 集合。
# 提不出任何表名即红（拒绝以空集假绿通过）。
@dataclass(frozen=True)
class TableInventory:
    inventory: list[str]
    banned: list[str]


def derive_banned_tables(rust_root_dir: str) -> TableInventory:
    sources: list[str] = []
    migrations_dir = os.path.join(rust_root_dir, "migrations")
    if os.path.exists(migrations_dir):
        # 列举机制归家族共享单点 read_dir_entries（#1742 收口）：排序归一为 locale_compare
        # （原 plain sort 实树对拍全等）；.sql 是迁移链扫描边界政策（单消费、无副本可
        # 漂移），留门不换扩展名闭集（#1742 留门）。
        for entry in read_dir_entries(migrations_dir):
            if entry.name.endswith(".sql"):
                sources.append(
                    Path(os.path.join(migrations_dir, entry.name)).read_text(encoding="utf-8")
                )
    migrate_rs = os.path.join(rust_root_dir, INFRA_SRC_REL, "db", "migrate.rs")
    if os.path.exists(migrate_rs):
        sources.append(Path(migrate_rs).read_text(encoding="utf-8"))

    inventory: set[str] = set()
    for source in sources:
        for m in CREATE_TABLE_RE.finditer(source):
            inventory.add(m.group(1))
    if len(inventory) == 0:
        fail(
            f"迁移链提不出任何 CREATE TABLE 表名（{migrations_dir} 无 .sql 或清单漂移）"
            "——拒绝以空集假绿通过"
        )

    missing = [t for t in MECHANISM_TABLES if t.table not in inventory]
    if len(missing) > 0:
        fail(
            f"机制表清单漂移：{'、'.join(t.table for t in missing)} 不在迁移链 CREATE TABLE 清单中"
            "——表已改名/删除，请同步 MECHANISM_TABLES（清单漂移 fail loud）"
        )

    mechanism_names = {t.table for t in MECHANISM_TABLES}
    banned = sorted(t for t in inventory if t not in mechanism_names)
    if len(banned) == 0:
        fail("机制表清单覆盖了迁移链全部表——禁 DML 账本数据表集合为空，拒绝以空集假绿通过")
    return TableInventory(inventory=sorted(inventory), banned=banned)


# 单条 DML 命中：行号（1 起算）、原文行、匹配文本
@dataclass(frozen=True)
class DmlHit:
    line: int
    text: str
    match: str


# DML 禁令形态（针对禁 DML 表集合，大小写不敏感）：
# `INSERT [OR x] INTO <table>` / `REPLACE INTO <table>` / `UPDATE <table>` /
# `DELETE FROM <table>`，含 `main.` 库名前缀限定（可选 schema 段跳过后取表名）。
# SQL 就住在字符串里，调用方须以 keep_literals=True 掩码（只抹注释）。
def dml_pattern(banned_tables: list[str]) -> re.Pattern[str]:
    return re.compile(
        r"\b(?:INSERT\s+(?:OR\s+\w+\s+)?INTO|REPLACE\s+INTO|UPDATE|DELETE\s+FROM)\s+"
        r"(?:[\"'`]?\w+[\"'`]?\s*\.\s*)?[\"'`]?"
        f"(?:{'|'.join(banned_tables)})\\b",
        re.IGNORECASE,
    )


# 扫描单个 Rust 源文本（调用方传入 keep_literals 掩码后的文本）：返回全部命中
def scan_dml(masked: str, banned_tables: list[str]) -> list[DmlHit]:
    raw_lines = masked.split("\n")
    hits: list[DmlHit] = []
    for m in dml_pattern(banned_tables).finditer(masked):
        line = line_at(masked, m.start())
        raw_line = raw_lines[line - 1] if line - 1 < len(raw_lines) else ""
        hits.append(DmlHit(line=line, text=raw_line.strip(), match=m.group(0)))
    return hits


# 测试豁免形态（ADR-0056 决策 5，与守门家族同款）：tests.rs 文件与 tests/ 目录
def is_test_path(rel_segments: list[str]) -> bool:
    return "tests" in rel_segments[:-1] or rel_segments[-1] == "tests.rs"


def main() -> None:
    src_app_dir = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_SRC_APP
    infra_src_dir = os.path.join(src_app_dir, INFRA_SRC_REL)
    if not os.path.exists(infra_src_dir):
        fail(f"基础设施 crate 源目录不存在：{infra_src_dir}")

    derived = derive_banned_tables(src_app_dir)
    inventory, banned = derived.inventory, derived.banned

    # 免扫清单自证：登记文件必须存在（清单漂移 fail loud，不放任守门空转）
    problems: list[str] = []
    for e in EXEMPT_FILES:
        if not os.path.exists(os.path.join(infra_src_dir, e.file)):
            problems.append(
                f"✗ 免扫清单漂移：{INFRA_SRC_REL}/{e.file} 不存在（{e.why}）——"
                "文件改名/搬迁后未同步 EXEMPT_FILES，请复核后更新登记"
            )

    # 全量收集（家族共享单点 walk_text_files）→ 免扫/外挂测试分流 → 生产文本扫描（掩码只抹注释、保留 SQL 字符串）
    all_files = walk_text_files(
        infra_src_dir, INFRA_SRC_REL, WalkTextOptions(extensions=RUST_EXTENSIONS)
    )
    scanned_files: list[WalkedFile] = []
    hits_by_file: dict[str, list[DmlHit]] = {}
    exempt_rel = {f"{INFRA_SRC_REL}/{e.file}" for e in EXEMPT_FILES}
    for f in all_files:
        segments = f.rel.split("/")
        if is_test_path(segments):
            continue  # 外挂测试整文件豁免（ADR-0056 决策 5）
        if f.rel in exempt_rel:
            continue  # 免扫职责（明示登记于 EXEMPT_FILES）
        scanned_files.append(f)
        source = Path(f.abs).read_text(encoding="utf-8")
        masked = mask_non_code(source, True)
        hits = scan_dml(masked, banned)
        if len(hits) > 0:
            hits_by_file[f.rel] = hits

    # 生产命中即红（纯禁令，定位到 文件:行）；已登记例外文件的命中数由下方严格
    # 相等校验裁决（漂移/收敛才红），不在此逐条报。
    for rel in sorted(hits_by_file):
        if any(f"{INFRA_SRC_REL}/{e.file}" == rel for e in REGISTERED_EXCEPTIONS):
            continue
        for hit in hits_by_file[rel]:
            problems.append(
                f"✗ 账本数据表 DML：{rel}:{hit.line}（{hit.match}）\n"
                f"    {hit.text}\n"
                f"    基础设施不得对账本数据表执行 DML（ADR-0111 决策 1、决策 5 / issue #1135；"
                f"承接 ADR-0071 守门意图）：账本数据的语义与算术归域——\n"
                f"    写路径收敛到域层公开写入口；迁移与 schema 守卫按职责免扫"
                f"（明示登记于本脚本 EXEMPT_FILES），其余位置命中即红"
            )

    # 已登记例外严格相等校验：漂移（≠登记数）即红、收敛（清零）即红；
    # 登记文件消失（改名/搬迁）也红（清单漂移 fail loud，与 EXEMPT_FILES 同纪律）。
    for exc in REGISTERED_EXCEPTIONS:
        rel = f"{INFRA_SRC_REL}/{exc.file}"
        if not any(f.rel == rel for f in scanned_files):
            problems.append(
                f"✗ 例外清单漂移：{rel} 不在扫描范围（不存在或已改为测试豁免形态）——"
                f"文件改名/搬迁后未同步 REGISTERED_EXCEPTIONS（{exc.why}），请复核后更新登记"
            )
            continue
        actual = len(hits_by_file.get(rel, []))
        if actual != exc.count:
            problems.append(
                f"✗ 例外已收敛：{rel} 实际命中 0 处——登记条目应删除（失效登记是第二份事实，"
                "请同步清理 REGISTERED_EXCEPTIONS）"
                if actual == 0
                else f"✗ 例外漂移：{rel} 实际命中 {actual} 处 ≠ 登记的 {exc.count} 处（{exc.why}）——"
                "多出的命中未登记新直置即红，复核后同步更新代码与登记"
            )

    if len(problems) > 0:
        for p in problems:
            print(p, file=sys.stderr)
        print(
            f"❌ 基础设施账本数据表 DML 禁令失败：{len(problems)} 处问题"
            f"（禁 DML 账本数据表 {len(banned)} 张，迁移链全量 {len(inventory)} 张；"
            f"ADR-0111 决策 1、决策 5 / issue #1135）",
            file=sys.stderr,
        )
        sys.exit(1)

    print(
        f"✓ 基础设施账本数据表 DML 禁令通过：禁 DML 账本数据表 {len(banned)} 张"
        f"（迁移链全量 {len(inventory)} 张 − 机制表 {len(MECHANISM_TABLES)} 张："
        f"{' / '.join(t.table for t in MECHANISM_TABLES)}）\n"
        f"  · 免扫文件（明示理由）：{'；'.join(f'{e.file}（{e.why}）' for e in EXEMPT_FILES)}\n"
        f"  · 已登记例外 {len(REGISTERED_EXCEPTIONS)} 条（严格相等校验）："
        + (
            "、".join(e.file for e in REGISTERED_EXCEPTIONS)
            if len(REGISTERED_EXCEPTIONS) > 0
            else "无——#1108 起壳机制迁出根包，例外随迁退役"
        )
        + f"\n  · 扫描 {INFRA_SRC_REL} 非测试文件 {len(scanned_files)} 个零生产命中（issue #1135 / ADR-0111）"
    )


# 仅直接运行时执行 main；包装测试以子进程调用（行为等价判据：只测外部可观察结果）。
if __name__ == "__main__":
    main()
