#!/usr/bin/env -S uv run --quiet
# 守门挂载登记（issue #1682 spec）：质量门挂载声明的单一事实源——「哪些门被质量
# 门槛跑到、挂哪」的唯一查询点。此前该事实由三本互不核对的登记簿维持（check.sh
# 手写行、CI workflow 手写步骤、各门头注释的「挂载于」声明），删任一行零测试变红；
# 本模块把挂载收成家族共用的数据结构，接线（wiring）成为登记条目的一环。
#
# 纯数据，无运行时副作用、无 CLI（grilling 定案：查询直接读导出常量，不加
# --list 报告命令，YAGNI）。核对，不生成：check.sh 与 CI workflow 保持手写，
# 守门接线测试（test_gate_mounts.py）遍历本登记断言双向全等——删任一宿主挂载行
# 或删任一登记条目至少一条断言变红（删除即变红，ADR-0087 断言强度）。
#
# 判定语义 = per-host 行首前缀：登记条目按宿主声明 {file, prefix, where}，宿主
# 文件的非注释行 trim 后行首匹配该前缀。echo 展示行行首是 `echo "▶`，永不可能
# 命中执行形态——echo 假绿免疫是判定语义的结构后果（#1112 第三轮审查实测教训）。
# 同一门在两宿主的命令文本实差（`( cd src-app && …)` 包裹 vs `working-directory`
# / 裸命令、`-- --check` vs `--check`）由 per-host prefix 显式表达。
#
# 新增守门的登记动作（story 5）：在 GATE_MOUNTS 追加一条——唯一名称、issue/ADR
# 出处（导航字段）、按宿主的执行行形态；check.sh 执行行与 CI 步骤同步挂载，
# 接线测试自动核对三侧全等。门的核心判定规则归各门自身，本登记只管挂载。
#
# Web 参照前端退役（#2171/#2172 / ADR-0148）：八个纯 Web 门随宿主删除、登记条目
# 一并移除；CI 前端 job 更名 gates（守门脚本 job，宿主语义不变）。
# 守门脚本运行时迁移（#2174 / ADR-0083 修订注记）：守门脚本门宿主行形态
# `bun scripts/x.ts` → `uv run scripts/x.py`，类型检查门 vue-tsc→原生 tsc→mypy，
# 守门脚本测试 bun test scripts/ → uv run scripts/run_tests.py；登记结构与
# 双向全等核对语义不变。

from __future__ import annotations

from dataclasses import dataclass
from typing import Final, Literal

# 宿主文件：本地质量门槛序列（唯一权威的全量执行面）。
CHECK_SH_FILE: Final = "scripts/check.sh"

# 宿主文件：CI workflow（挂载子集，按 job 分布）。
CI_WORKFLOW_FILE: Final = ".github/workflows/build.yml"

# 宿主标识：登记只认这两份宿主文件，挂到清单外的宿主是类型错误。
GateHostFile = Literal["scripts/check.sh", ".github/workflows/build.yml"]


# 按宿主的挂载声明。
@dataclass(frozen=True)
class GateHostFileMount:
    file: GateHostFile
    # 执行行形态：宿主非注释行 trim 后行首前缀匹配
    prefix: str
    # 宿主内定位说明（job / 序列，导航用）
    where: str


# 一条质量门的挂载登记。
@dataclass(frozen=True)
class GateMount:
    # 唯一名称（= 门的展示名，全表唯一）
    name: str
    # issue/ADR 出处（导航字段）：门本体的权威出处，供 all-entries 核对清单一次生成
    refs: str
    # 按宿主的挂载声明：check.sh 恒有一条；CI 挂载仅在该门实际入 CI 时声明
    hosts: tuple[GateHostFileMount, ...]


# CI 挂门 job 内的非门步骤豁免（政策显式，#1591：政策无源就显式）：
# 前端退役（#2171/#2172）后 gates job 内全部单行 run 步骤均为登记门——原豁免项
# `run: pnpm run build`（内含前端类型检查）随宿主删除，清单留空但机制保留。
CI_NON_GATE_RUNS: Final[tuple[str, ...]] = ()


def sh(prefix: str, where: str) -> GateHostFileMount:
    return GateHostFileMount(file="scripts/check.sh", prefix=prefix, where=where)


def ci(prefix: str, where: str) -> GateHostFileMount:
    return GateHostFileMount(file=".github/workflows/build.yml", prefix=prefix, where=where)


# 登记清单（check.sh 全部质量门步骤：uv Python 门、Rust 步骤与 shell 步骤同表；
# check-docs.sh 作单条，其内部九项校验不展开）。顺序与 check.sh 执行序列一致，
# 便于逐行核对；CI 挂载为子集（gates job / backend-lint job 分布见各条 where）。
# 实际集合以守门接线测试与 check.sh / CI 的双向全等断言为准，本注释不作计数依据。
GATE_MOUNTS: Final[tuple[GateMount, ...]] = (
    GateMount(
        name="守门脚本与测试类型检查（mypy）",
        refs="issue #734 / #740 / ADR-0083；运行时迁移 #2174（tsc → mypy）",
        hosts=(
            sh(
                "uv run scripts/check_scripts_types.py",
                "scripts/check.sh 质量门槛序列（首门）",
            ),
            ci(
                "run: uv run scripts/check_scripts_types.py",
                ".github/workflows/build.yml gates job（首步）",
            ),
        ),
    ),
    GateMount(
        name="Rust clippy",
        refs="issue #1087（--workspace 范围；门槛本体早于 issue 化）",
        hosts=(
            sh(
                "( cd src-app && cargo clippy --workspace --all-targets --all-features -- -D warnings )",
                "scripts/check.sh 质量门槛序列",
            ),
            ci(
                "run: cargo clippy --workspace --all-targets --all-features -- -D warnings",
                ".github/workflows/build.yml backend-lint job",
            ),
        ),
    ),
    GateMount(
        name="Rust gate-off 编译检查（ledger-infra 默认 feature）",
        refs="issue #1133 / #1473",
        hosts=(
            sh("( cd src-app && cargo check -p ledger-infra )", "scripts/check.sh 质量门槛序列"),
            ci(
                "run: cargo check -p ledger-infra",
                ".github/workflows/build.yml backend-lint job",
            ),
        ),
    ),
    GateMount(
        name="全仓无 tauri 守门（cargo tree，依赖图面）",
        refs="issue #2083 / ADR-0145 决策 4/7（#2074 零链接门升级）",
        hosts=(
            sh(
                "( cd src-app && cargo tree --workspace -i tauri",
                "scripts/check.sh 质量门槛序列（infra gate-off 门后）",
            ),
            ci(
                "run: cargo tree --workspace -i tauri",
                ".github/workflows/build.yml backend-lint job（grep 管道形态，见 backend-lint 步骤）",
            ),
        ),
    ),
    GateMount(
        name="Rust 格式检查（cargo fmt）",
        refs="issue #1087（--all 范围；门槛本体早于 issue 化）",
        hosts=(
            sh(
                "( cd src-app && cargo fmt --all -- --check )",
                "scripts/check.sh 质量门槛序列",
            ),
            ci("run: cargo fmt --all --check", ".github/workflows/build.yml backend-lint job"),
        ),
    ),
    GateMount(
        name="基础设施 rustdoc 门禁（cargo doc -D warnings）",
        refs="issue #1139",
        hosts=(
            sh(
                "( cd src-app && RUSTDOCFLAGS='-D warnings' cargo doc -p ledger-infra --no-deps )",
                "scripts/check.sh 质量门槛序列",
            ),
            ci(
                "run: RUSTDOCFLAGS='-D warnings' cargo doc -p ledger-infra --no-deps",
                ".github/workflows/build.yml backend-lint job",
            ),
        ),
    ),
    GateMount(
        name="文档一致性检查（check-docs.sh）",
        refs="issue #166 起（九项校验逐票增补）",
        hosts=(
            sh("./scripts/check-docs.sh", "scripts/check.sh 质量门槛序列"),
            ci("run: ./scripts/check-docs.sh", ".github/workflows/build.yml gates job"),
        ),
    ),
    GateMount(
        name="全仓无 tauri 守门（check-no-tauri，清单形状面）",
        refs="issue #2083 / ADR-0145 决策 4/7",
        hosts=(
            sh(
                "uv run scripts/check_no_tauri.py",
                "scripts/check.sh 质量门槛序列（文档一致性检查后）",
            ),
            ci(
                "run: uv run scripts/check_no_tauri.py",
                ".github/workflows/build.yml gates job",
            ),
        ),
    ),
    GateMount(
        name="版本元数据一致性检查（check-versions）",
        refs="issue #1957 / ADR-0022 修订注记",
        hosts=(
            sh("uv run scripts/check_versions.py", "scripts/check.sh 质量门槛序列"),
            ci(
                "run: uv run scripts/check_versions.py",
                ".github/workflows/build.yml gates job",
            ),
        ),
    ),
    GateMount(
        name="结构守门检查（check-structure）",
        refs="spec #1086 / ADR-0056",
        hosts=(
            sh("uv run scripts/check_structure.py", "scripts/check.sh 质量门槛序列"),
            ci(
                "run: uv run scripts/check_structure.py",
                ".github/workflows/build.yml gates job",
            ),
        ),
    ),
    GateMount(
        name="基础设施账本数据表 DML 禁令（check-infra-dml）",
        refs="issue #1135 / ADR-0111",
        hosts=(
            sh("uv run scripts/check_infra_dml.py", "scripts/check.sh 质量门槛序列"),
            ci(
                "run: uv run scripts/check_infra_dml.py",
                ".github/workflows/build.yml gates job（issue #1682 CI 补齐）",
            ),
        ),
    ),
    GateMount(
        name="东财行情面端点零残留源码扫描（check-eastmoney-residue）",
        refs="issue #1572 / ADR-0130 守门判据①",
        hosts=(
            sh("uv run scripts/check_eastmoney_residue.py", "scripts/check.sh 质量门槛序列"),
            ci(
                "run: uv run scripts/check_eastmoney_residue.py",
                ".github/workflows/build.yml gates job（issue #1682 CI 补齐）",
            ),
        ),
    ),
    GateMount(
        name="壳根包裸字号直调零残留扫描（check-bare-font-size）",
        refs="issue #2108 / spec #2099 / ADR-0147 决策 4",
        hosts=(
            sh(
                "uv run scripts/check_bare_font_size.py",
                "scripts/check.sh 质量门槛序列（东财残留扫描后）",
            ),
            ci(
                "run: uv run scripts/check_bare_font_size.py",
                ".github/workflows/build.yml gates job（东财残留扫描后）",
            ),
        ),
    ),
    GateMount(
        name="i18n key 全等检查（check-i18n-keys）",
        refs="issue #342 / ADR-0049 / #1188",
        hosts=(
            sh("uv run scripts/check_i18n_keys.py", "scripts/check.sh 质量门槛序列"),
            ci(
                "run: uv run scripts/check_i18n_keys.py",
                ".github/workflows/build.yml gates job（issue #1682 CI 补齐）",
            ),
        ),
    ),
    GateMount(
        name="Rust 测试守门（check-test-support）",
        refs="issue #752 / #758 / ADR-0084",
        hosts=(
            sh("uv run scripts/check_test_support.py", "scripts/check.sh 质量门槛序列"),
            ci(
                "run: uv run scripts/check_test_support.py",
                ".github/workflows/build.yml gates job（issue #1682 CI 补齐）",
            ),
        ),
    ),
    GateMount(
        name="测试执行覆盖守门（test-exec check）",
        refs="issue #1112 / spec #1086",
        hosts=(
            sh("uv run scripts/test_exec.py check", "scripts/check.sh 质量门槛序列"),
            ci(
                "run: uv run scripts/test_exec.py check",
                ".github/workflows/build.yml gates job",
            ),
        ),
    ),
    GateMount(
        name="守门脚本测试（uv run scripts/run_tests.py）",
        refs="issue #1991 / ADR-0083；运行时迁移 #2174",
        hosts=(
            sh("uv run scripts/run_tests.py", "scripts/check.sh 质量门槛序列"),
            ci(
                "run: uv run scripts/run_tests.py",
                ".github/workflows/build.yml gates job",
            ),
        ),
    ),
)
