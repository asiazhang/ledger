#!/usr/bin/env -S uv run --quiet
# 测试执行器（issue #1112 / 父 spec #1086）：workspace 拆成多 crate 之后，
# `cargo test --workspace` 把全部测试二进制**顺序**启动（当前清单与数量以
# `uv run scripts/test_exec.py plan` 的输出为唯一权威，本文件不复述），每个二进制
# 各自按 CPU 数开满 libtest 线程——进程启动开销与「最后一个二进制独占整机」的
# 尾部空转是纯浪费。执行面按两条入口重划：
#
# ① 并发入口（本脚本默认命令 run，scripts/test.sh 第一条命令）：一条
#    `cargo test --workspace --lib --test '*' --no-run --message-format=json-render-diagnostics`
#    （与 CI build.yml 的 PR 执行面同口径）构建一次，解析 cargo 报告的测试二进制
#    清单，再由本脚本统一调度——全局
#    并行度 = min(--jobs 或 CPU 数, 待跑二进制数)，每个二进制固定
#    `RUST_TEST_THREADS=1`（只约束 libtest 线程），libtest 线程数 = 并行度，
#    不出现「各二进制各自开满 libtest 线程」的 CPU 超订（测试自起的 tokio 等
#    运行时不在控制面，属经验证据）；失败聚合报告，跑完全部才退出。
# ② 非并发入口（scripts/test.sh 第二、三条命令）：e2e（cucumber，
#    `harness = false`，自有 runner 与 CLI）与 doc-test（测试二进制由 rustdoc
#    生成、不进 cargo 的 test artifact 报告）仍走 cargo 自有入口，不纳入并发
#    调度——e2e 的既有入口形态本票不动。
#
# 覆盖守门（check 命令；挂载登记归守门挂载登记 scripts/gate_mounts.py，issue #1682）：
# 目标清单口径与 `cargo test` 默认执行面全等——lib 单测 + bin 单测 + 集成测试 +
# doc-test；example / bench 不在默认执行面（cargo 只构建 example、不跑 bench），
# 故不入清单，但发现声明即提示，防口径漂移。bin 单测不入并发入口（与 CI PR 口径
# 对齐，issue #1787 热点 2）：零单测 bin 在 manifest 声明 test = false（源码扫描
# 守门防新增测试静默漏跑），有单测的 bin 登记 EXEMPT_BINS 并写明承接入口。
# 守门四条（任一处漂移即红，fail loud）：
#   ① 工作区全部测试目标 = 并发入口承接集合 ⊎ 非并发入口承接集合 ⊎ bin 豁免集合
#      （互斥且无遗漏；目标清单自 manifest + 目录自动发现派生，新增 target /
#      新成员 crate 自动入列；豁免登记与发现漂移即红）；
#   ② `[[test]] harness = false` 的声明集合 ⇔ scripts/test.sh 非并发入口的
#      `--test <name>` 名单（双向全等——新增自定义 harness 目标未登记即红，登记
#      失效或拼写漂移同样红）；
#   ③ 存在 doc-test 目标 ⇔ scripts/test.sh 有 `cargo test --workspace --doc`
#      （doc 命令必须带 workspace 范围——退化成 `cargo test --doc` 会让成员 crate
#      的 doctest 静默漏跑而守门仍绿）；且 scripts/test.sh 必须调用并发入口
#      的**运行**命令 `uv run scripts/test_exec.py [run]`（删除即红；`… check` 自检行
#      不算运行接线）；
#     ②③ 同口径：只认**命令位置**的命令——非注释行里的说明文字（引号内的
#      `echo "…uv run scripts/test_exec.py…"`）不算接线，否则把三条真命令全包成
#      echo 字符串就能让两个入口一起假绿（#1112 第三轮审查实测）。
#   ④ 执行器等价性：测试运行期不得依赖 cargo 注入的环境变量（现状零命中，见
#      cargo_runtime_env_problems 注）。
#（原守门规则④「门禁自身接线：check.sh 与 CI frontend job 必须调用 check」已退役
# 并入守门挂载登记——接线成为守门家族 interface 的一环，住可测试的登记而非各门
# 自证，issue #1682。）
# run 命令另有第六道交叉核对：`cargo test --no-run` 实际构建出的测试二进制集合
# 必须与①的目标清单全等——发现逻辑与 cargo 真实行为漂移即红，不给「清单看着对、
# 实际漏跑」留口子。unsupported manifest 形态（auto* 开关、lib harness=false、
# 非尾随 `*` 的成员 glob）一律拒绝而非猜测。
#
# Python 化 + uv 运行时（issue #2174 / ADR-0083 修订注记）：类型经
# scripts/check_scripts_types.py（mypy）门槛检查；调用方式
# `uv run scripts/test_exec.py [run|check|plan] [--jobs N]`，默认 run；测试可传
# `--root <夹具根>` 指向夹具（夹具只需 manifest + 目录形态，不调用 cargo）。
# 本脚本经 scripts/test.sh 调起（本地测试入口的并发入口与入口自检）；
# 质量门槛挂载声明归守门挂载登记 scripts/gate_mounts.py（issue #1682）。
# 包装测试 scripts/test_test_exec.py。

from __future__ import annotations

import asyncio
import json
import os
import re
import sys
import time
from collections.abc import Awaitable, Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Final, Literal, NoReturn

# 复用守门家族共享原语库的 Rust 词法掩码（注释掩去、字面量保留）——不新建第二份词法器。
from gate_primitives import (
    RUST_EXTENSIONS,
    WalkTextOptions,
    has_extension,
    line_at,
    locale_sort_key,
    mask_non_code,
    walk_text_files,
)

# pytest 收集豁免（issue #2174）：本文件是守门门脚本，文件名 test_exec.py 恰好
# 匹配 pytest 的 test_*.py 收集模式——模块级 __test__ = False 防止 pytest 把
# 门脚本当测试模块收集（真正的测试住 scripts/test_test_exec.py）。
__test__ = False

# Rust workspace 根（= NSH 壳根包，#2083 切轨后住 src-app/）目录名，相对仓库根。
RUST_ROOT_DIR_NAME: Final = "src-app"

# 两个测试入口的宿主脚本（覆盖守门读取其命令行）。
TEST_SH_REL: Final = os.path.join("scripts", "test.sh")

# 并发入口在 scripts/test.sh 里的调用标记（删除即红）。
PARALLEL_ENTRY_MARKER: Final = "scripts/test_exec.py"

# 非并发入口逐目标参数与 doc-test 开关（scripts/test.sh 命令行判据）。
DELEGATED_TARGET_FLAG: Final = "--test"
DOC_FLAG: Final = "--doc"

# workspace 范围参数：`--workspace` 或 `--all`（精确 token 匹配，`--all-targets`
# / `--all-features` 不算；与 check_structure.py 的 WORKSPACE_COMMAND_FILES 同口径）。
WORKSPACE_FLAG: Final = "--workspace"
ALL_FLAG: Final = "--all"

# cargo `cargo test` 默认执行面的目标种类；`doc` 由 cargo 自有入口承接。
TargetKind = Literal["lib", "bin", "test", "doc"]


# 一个测试目标（doc 目标的 target 名 = lib 目标名）。
@dataclass(frozen=True)
class DiscoveredTarget:
    package: str
    target: str
    kind: TargetKind
    # 仅集成测试目标有意义：`[[test]] harness = false`（自定义 runner）。
    harness_false: bool
    # 目标所属包根（绝对路径）——测试进程的 cwd 与 cargo 口径一致（见 run_key 注）。
    cwd: str


# 展示用键（包名 + 目标名）：守门报错与 plan 输出用，不参与集合相等核对。
def target_key(t: DiscoveredTarget) -> str:
    return f"{t.package}::{t.target}"


# 执行面核对键（包名 + 目标名 + 种类）：cargo 允许同一包内 lib 与 bin / 集成
# target 同名（`src/lib.rs` + `src/main.rs` 的常见形态），只用「包 + 名」会撞键
# ——静态清单与 run 的交叉核对都会假绿，实际只跑掉一个二进制（issue #1112
# Spec 审查发现）。故一切集合运算都用带 kind 的键。
def run_key(t: DiscoveredTarget) -> str:
    return f"{t.package}::{t.target}::{t.kind}"


# bin 单测豁免登记条目：并发入口不跑的 bin 单测，必须写明承接入口。
@dataclass(frozen=True)
class ExemptBinDecl:
    package: str
    target: str
    # 单测的承接入口（守门报告与 summary 展示用）。
    owner: str


# bin 单测豁免登记（issue #1787 热点 2）：并发入口的执行面与 CI build.yml 的
# PR 口径（`cargo test --workspace --lib --test '*'`，不含 bin 单测）对齐，
# 有单测的 bin 在此登记承接入口。当前仅 ledger-perf：生成器正确性单测属
# 性能工具链，随每日基准在 perf-bench.yml 运行（`cargo test --bin ledger-perf`），
# 不拖进 PR CI。守门方向：发现的每个 test = true bin 都必须登记（删登记或
# 新增未声明 bin 即红）；bin 删除后残留的死登记无副作用，不红。
EXEMPT_BINS: Final[tuple[ExemptBinDecl, ...]] = (
    ExemptBinDecl(
        # #2082 迁居：ledger-perf 自旧壳根包迁入 nsh-app 同包 bin（自动发现）。
        package="nsh-app",
        target="ledger-perf",
        # owner 文案勿写成完整命令形状（如 cargo test --bin …）：workspace 命令覆盖
        # 守门会把它当真命令扫（缺 --workspace 即红，ADR-0056）。
        owner="perf-bench.yml 每日基准专属入口（--bin ledger-perf 显式选择）",
    ),
)


# `test = false` 的 bin 目标：不产生测试目标，但源码扫描守门防新增测试静默漏跑。
@dataclass(frozen=True)
class TestFalseBin:
    package: str
    target: str
    # bin 源文件绝对路径（cargo 口径解析结果；解析失败为 None，守门红）。
    source: str | None


# 目标发现结果：目标清单 + 形态问题（不支持形态一律显式拒绝而非猜测）。
@dataclass
class Discovery:
    targets: list[DiscoveredTarget]
    problems: list[str]
    # example / bench 目标（不在 `cargo test` 默认执行面，仅提示）。
    out_of_face: list[str]
    # `test = false` 的 bin 目标（不入执行面，源码扫描守门用）。
    test_false_bins: list[TestFalseBin]


# ── manifest 解析（窄形态文本扫描，形态同 check_structure.py 家族） ──────────
# 只认本仓实际存在的键形态：字符串 / 布尔 / 字符串数组 / `[[table]]` 条目。
# 需要正则与真 TOML 解析器才能覆盖的形态（多行字符串、内联表）不出现即不支持，
# 出现时发现阶段会因目标数对不上而在 run 命令的交叉核对里变红。

@dataclass
class TargetDecl:
    name: str | None = None
    path: str | None = None
    test: bool | None = None
    harness: bool | None = None
    doctest: bool | None = None


@dataclass
class ManifestTables:
    package: dict[str, str]
    lib: TargetDecl | None
    bins: list[TargetDecl]
    tests: list[TargetDecl]
    benches: list[TargetDecl]
    examples: list[TargetDecl]
    workspace: dict[str, list[str]]
    # auto* 开关：发现逻辑不辖，出现即拒绝。
    unsupported_keys: list[str]


ARRAY_DECL_TABLES: Final[dict[str, str]] = {
    "bin": "bins",
    "test": "tests",
    "bench": "benches",
    "example": "examples",
}

UNSUPPORTED_KEYS: Final[tuple[str, ...]] = ("autobins", "autotests", "autobenches", "autoexamples")


# 去掉行内注释：`#` 在引号外才算注释起点（字符串里的 `#` 保留）。
def strip_comment(line: str) -> str:
    quote: str | None = None
    for i, ch in enumerate(line):
        if quote is not None:
            if ch == quote:
                quote = None
            continue
        if ch == '"' or ch == "'":
            quote = ch
            continue
        if ch == "#":
            return line[:i]
    return line


# 方括号配平数（多行数组续行判定）。
def bracket_balance(line: str) -> int:
    depth = 0
    quote: str | None = None
    for ch in line:
        if quote is not None:
            if ch == quote:
                quote = None
            continue
        if ch == '"' or ch == "'":
            quote = ch
        elif ch == "[":
            depth += 1
        elif ch == "]":
            depth -= 1
    return depth


_QUOTED_VALUE: Final = re.compile(r'^(["\'])(.*)\1$')


# 解析基本值：字符串 / 布尔 / 字符串数组 / 其余原样返回。
def parse_value(raw: str) -> object:
    text = raw.strip()
    quoted = _QUOTED_VALUE.match(text)
    if quoted is not None:
        return quoted.group(2)
    if text == "true":
        return True
    if text == "false":
        return False
    if text.startswith("[") and text.endswith("]"):
        return [parse_value(item) for item in split_array_items(text[1:-1])]
    return text


# 按顶层逗号切分数组项（引号内的逗号不算分隔）。
def split_array_items(body: str) -> list[str]:
    items: list[str] = []
    current = ""
    quote: str | None = None
    for ch in body:
        if quote is not None:
            current += ch
            if ch == quote:
                quote = None
            continue
        if ch == '"' or ch == "'":
            quote = ch
            current += ch
            continue
        if ch == ",":
            items.append(current)
            current = ""
            continue
        current += ch
    items.append(current)
    return [item.strip() for item in items if item.strip() != ""]


_ARRAY_HEADER: Final = re.compile(r"^\[\[\s*([A-Za-z0-9_-]+)\s*\]\]$")
_TABLE_HEADER: Final = re.compile(r"^\[\s*([A-Za-z0-9_-]+)\s*\]$")
_KV_LINE: Final = re.compile(r"^([A-Za-z0-9_-]+)\s*=\s*(.+)$")


def parse_manifest(text: str) -> ManifestTables:
    out = ManifestTables(
        package={},
        lib=None,
        bins=[],
        tests=[],
        benches=[],
        examples=[],
        workspace={},
        unsupported_keys=[],
    )
    table: str | None = None
    decl: TargetDecl | None = None
    lines = text.split("\n")
    i = 0
    while i < len(lines):
        line = strip_comment(lines[i]).strip()
        i += 1
        if line == "":
            continue
        while bracket_balance(line) > 0 and i < len(lines):
            line += " " + strip_comment(lines[i]).strip()
            i += 1

        array_header = _ARRAY_HEADER.match(line)
        if array_header is not None:
            key = ARRAY_DECL_TABLES.get(array_header.group(1))
            if key is None:
                table = None
                decl = None
            else:
                decl = TargetDecl()
                table = key
                if key == "bins":
                    out.bins.append(decl)
                elif key == "tests":
                    out.tests.append(decl)
                elif key == "benches":
                    out.benches.append(decl)
                else:
                    out.examples.append(decl)
            continue
        table_header = _TABLE_HEADER.match(line)
        if table_header is not None:
            name = table_header.group(1)
            decl = None
            table = name
            if name == "lib":
                out.lib = TargetDecl()
            continue
        if line.startswith("["):
            # 点分表头（`[workspace.lints.clippy]` 等）与不认识的表：本段无受关注键。
            decl = None
            table = None
            continue
        kv = _KV_LINE.match(line)
        if kv is None:
            continue
        key = kv.group(1)
        value = parse_value(kv.group(2))
        if key in UNSUPPORTED_KEYS:
            out.unsupported_keys.append(key)
        target = decl if decl is not None else (out.lib if table == "lib" else None)
        if target is not None:
            apply_target_key(target, key, value)
            continue
        if table == "package" and key == "name" and isinstance(value, str):
            out.package["name"] = value
        if table == "workspace" and key == "members" and isinstance(value, list):
            out.workspace["members"] = [v for v in value if isinstance(v, str)]
    return out


# 目标声明键（字符串 / 布尔）逐条落位；非受关注键忽略。
def apply_target_key(target: TargetDecl, key: str, value: object) -> None:
    if key == "name" and isinstance(value, str):
        target.name = value
    elif key == "path" and isinstance(value, str):
        target.path = value
    elif key == "test" and isinstance(value, bool):
        target.test = value
    elif key == "harness" and isinstance(value, bool):
        target.harness = value
    elif key == "doctest" and isinstance(value, bool):
        target.doctest = value


# ── 目标发现（manifest 声明 + 目录自动发现，口径 = cargo test 默认执行面） ────

def read_manifest(dir_path: str) -> ManifestTables | None:
    path = os.path.join(dir_path, "Cargo.toml")
    if not os.path.exists(path):
        return None
    return parse_manifest(Path(path).read_text(encoding="utf-8"))


# #1742 留门：list_dir 形状与家族一层列举原语不同构——names-only 契约（文件与目录
# 同列）、缺目录容错（返回 []）、调用侧按路径 stat 重判类型（跟随符号链接，与
# dirent 不跟随语义不同）；已是单点命名助手，收口只会改语义或纯写法替换。
def list_dir(dir_path: str) -> list[str]:
    if not os.path.exists(dir_path):
        return []
    return sorted(os.listdir(dir_path))


def is_directory(path: str) -> bool:
    return os.path.isdir(path)


# 目标名归一：cargo 对未显式命名的 target 取文件/目录名。
def stem_name(file: str) -> str:
    return file.removesuffix(".rs")


# 展开 `[workspace] members`：只支持「前缀 + 尾随 `*`」的单层 glob 与裸目录。
def expand_member_globs(rust_root_dir: str, patterns: list[str]) -> tuple[list[str], list[str]]:
    dirs: list[str] = []
    problems: list[str] = []
    for pattern in patterns:
        if pattern.endswith("/*"):
            base = pattern[:-2]
            base_dir = os.path.join(rust_root_dir, base)
            if not is_directory(base_dir):
                problems.append(f"✗ 目标发现：members glob 基目录不存在：{base}（{pattern}）")
                continue
            for entry in list_dir(base_dir):
                entry_dir = os.path.join(base_dir, entry)
                if is_directory(entry_dir) and os.path.exists(os.path.join(entry_dir, "Cargo.toml")):
                    dirs.append(entry_dir)
            continue
        if "*" in pattern:
            problems.append(
                f"✗ 目标发现：未支持的 members glob 形态：{pattern}（只支持「前缀 + 尾随 *」单层 glob）"
            )
            continue
        dir_path = os.path.join(rust_root_dir, pattern)
        if not os.path.exists(os.path.join(dir_path, "Cargo.toml")):
            problems.append(f"✗ 目标发现：members 目录缺 Cargo.toml：{pattern}")
            continue
        dirs.append(dir_path)
    return dirs, problems


@dataclass
class PackageTargets:
    targets: list[DiscoveredTarget]
    problems: list[str]
    out_of_face: list[str]
    test_false_bins: list[TestFalseBin]


# bin 源文件解析（cargo 口径）：显式 path 优先；无 path 时按 cargo 默认——
# target 名 = 包名 → src/main.rs，否则 src/bin/<name>.rs 或 src/bin/<name>/main.rs。
# 找不到返回 None（守门红，不猜测）。
def resolve_bin_source(dir_path: str, package_name: str, decl: TargetDecl, name: str) -> str | None:
    if decl.path is not None:
        explicit = os.path.join(dir_path, decl.path)
        return explicit if os.path.exists(explicit) else None
    candidates: list[str] = []
    if name == package_name:
        candidates.append(os.path.join("src", "main.rs"))
    candidates.append(os.path.join("src", "bin", f"{name}.rs"))
    candidates.append(os.path.join("src", "bin", name, "main.rs"))
    for rel in candidates:
        abs_path = os.path.join(dir_path, rel)
        if os.path.exists(abs_path):
            return abs_path
    return None


def discover_package(dir_path: str, rel: str, tables: ManifestTables) -> PackageTargets:
    targets: list[DiscoveredTarget] = []
    problems: list[str] = []
    out_of_face: list[str] = []
    test_false_bins: list[TestFalseBin] = []
    package_name = tables.package.get("name")
    if package_name is None:
        problems.append(f"✗ 目标发现：{rel}/Cargo.toml 缺 [package] name——目标无法归属包，拒绝继续")
        return PackageTargets(targets, problems, out_of_face, test_false_bins)
    for key in tables.unsupported_keys:
        problems.append(
            f"✗ 目标发现：{rel}/Cargo.toml 出现 auto* 开关 `{key}`——自动发现面被改写，"
            f"发现逻辑不辖该形态（拒绝猜测）。请扩展 scripts/test_exec.py 的目标发现后重跑"
        )

    # lib：显式 [lib] 表或 src/lib.rs 自动发现；`test = false` 关单测、`doctest = false` 关 doc-test。
    lib_declared = tables.lib is not None
    lib_present = lib_declared or os.path.exists(os.path.join(dir_path, "src", "lib.rs"))
    if lib_present:
        lib = tables.lib
        lib_name = (
            lib.name
            if lib is not None and lib.name is not None
            else package_name.replace("-", "_")
        )
        if lib is not None and lib.harness is False:
            problems.append(
                f"✗ 目标发现：{rel} 的 [lib] 声明 harness = false——自定义 harness 的 lib 单测"
                f"不属 libtest 面，需显式归入非并发入口（当前发现逻辑不支持）"
            )
        elif lib is None or lib.test is not False:
            targets.append(
                DiscoveredTarget(package=package_name, target=lib_name, kind="lib", harness_false=False, cwd=dir_path)
            )
        if lib is None or lib.doctest is not False:
            targets.append(
                DiscoveredTarget(package=package_name, target=lib_name, kind="doc", harness_false=False, cwd=dir_path)
            )

    # bin：显式 [[bin]] 或 src/main.rs、src/bin/*.rs、src/bin/*/main.rs 自动发现。
    bin_names: dict[str, TargetDecl] = {}
    for decl in tables.bins:
        bin_names[decl.name if decl.name is not None else stem_name(decl.path if decl.path is not None else "")] = decl
    bin_candidates: list[tuple[str, str]] = [
        (package_name, os.path.join("src", "main.rs")),
    ]
    src_bin_dir = os.path.join(dir_path, "src", "bin")
    for entry in list_dir(src_bin_dir):
        entry_path = os.path.join(src_bin_dir, entry)
        # .rs 扩展名闭集换库 hasExtension + RUST_EXTENSIONS（#1742 收口，全等表在 test_gate_primitives.py）
        if has_extension(entry, RUST_EXTENSIONS):
            bin_candidates.append((stem_name(entry), os.path.join("src", "bin", entry)))
        elif is_directory(entry_path) and os.path.exists(os.path.join(entry_path, "main.rs")):
            bin_candidates.append((entry, os.path.join("src", "bin", entry, "main.rs")))
    for candidate_name, candidate_path in bin_candidates:
        if not os.path.exists(os.path.join(dir_path, candidate_path)):
            continue
        if candidate_name not in bin_names:
            bin_names[candidate_name] = TargetDecl(name=candidate_name, path=candidate_path)
    for name, decl in bin_names.items():
        if decl.harness is False:
            problems.append(
                f"✗ 目标发现：{rel} 的 [[bin]] {name} 声明 harness = false——自定义 harness 不属并发面，需显式归入非并发入口"
            )
            continue
        if decl.test is False:
            # test = false 的 bin 不产生测试目标（cargo 也不构建其测试二进制），但
            # 登记下来供源码扫描守门：bin 内新增 #[test] 会静默漏跑，见 testFalseBinProblems。
            test_false_bins.append(
                TestFalseBin(
                    package=package_name,
                    target=name,
                    source=resolve_bin_source(dir_path, package_name, decl, name),
                )
            )
            continue
        targets.append(
            DiscoveredTarget(package=package_name, target=name, kind="bin", harness_false=False, cwd=dir_path)
        )

    # 集成测试：显式 [[test]] 或 tests/*.rs、tests/*/main.rs 自动发现。
    test_decls: dict[str, TargetDecl] = {}
    for decl in tables.tests:
        test_decls[decl.name if decl.name is not None else stem_name(decl.path if decl.path is not None else "")] = decl
    tests_dir = os.path.join(dir_path, "tests")
    for entry in list_dir(tests_dir):
        entry_path = os.path.join(tests_dir, entry)
        # .rs 闭集换库，同 src/bin 站点（#1742 收口）
        if has_extension(entry, RUST_EXTENSIONS):
            candidate_name = stem_name(entry)
        elif is_directory(entry_path) and os.path.exists(os.path.join(entry_path, "main.rs")):
            candidate_name = entry
        else:
            continue
        if candidate_name not in test_decls:
            test_decls[candidate_name] = TargetDecl(name=candidate_name)
    for name, decl in test_decls.items():
        if decl.test is False:
            continue
        targets.append(
            DiscoveredTarget(
                package=package_name,
                target=name,
                kind="test",
                harness_false=decl.harness is False,
                cwd=dir_path,
            )
        )

    # example / bench 不在 `cargo test` 默认执行面（只构建 example、不跑 bench），仅提示。
    for key in ("benches", "examples"):
        label = "bench" if key == "benches" else "example"
        for decl in getattr(tables, key):
            name = decl.name if decl.name is not None else stem_name(decl.path if decl.path is not None else "")
            if name != "":
                out_of_face.append(f"{rel}/{label}:{name}")
        for entry in list_dir(os.path.join(dir_path, key)):
            # .rs 闭集换库，同 src/bin 站点（#1742 收口）
            if has_extension(entry, RUST_EXTENSIONS):
                out_of_face.append(f"{rel}/{label}:{stem_name(entry)}")

    return PackageTargets(targets, problems, out_of_face, test_false_bins)


# 发现工作区全部测试目标（口径 = `cargo test` 默认执行面）。
def discover_targets(root_dir: str) -> Discovery:
    rust_root_dir = os.path.join(root_dir, RUST_ROOT_DIR_NAME)
    problems: list[str] = []
    targets: list[DiscoveredTarget] = []
    out_of_face: list[str] = []
    test_false_bins: list[TestFalseBin] = []

    root_tables = read_manifest(rust_root_dir)
    if root_tables is None:
        return Discovery(
            targets=targets,
            problems=[f"✗ 目标发现：Rust 根 manifest 不存在：{RUST_ROOT_DIR_NAME}/Cargo.toml"],
            out_of_face=out_of_face,
            test_false_bins=test_false_bins,
        )
    package_dirs: list[tuple[str, str]] = []
    if "name" in root_tables.package:
        package_dirs.append((rust_root_dir, RUST_ROOT_DIR_NAME))
    member_patterns = root_tables.workspace.get("members")
    if member_patterns is None:
        if not package_dirs:
            problems.append(
                f"✗ 目标发现：{RUST_ROOT_DIR_NAME}/Cargo.toml 既无 [package] 也无 [workspace] members"
            )
    else:
        expanded_dirs, expand_problems = expand_member_globs(rust_root_dir, member_patterns)
        problems.extend(expand_problems)
        for dir_path in expanded_dirs:
            package_dirs.append((dir_path, os.path.relpath(dir_path, root_dir)))

    seen: set[str] = set()
    for dir_path, rel in package_dirs:
        tables = read_manifest(dir_path)
        if tables is None:
            problems.append(f"✗ 目标发现：成员目录缺 Cargo.toml：{rel}")
            continue
        found = discover_package(dir_path, rel, tables)
        problems.extend(found.problems)
        out_of_face.extend(found.out_of_face)
        test_false_bins.extend(found.test_false_bins)
        for target in found.targets:
            key = run_key(target)
            if key in seen:
                continue
            seen.add(key)
            targets.append(target)
    return Discovery(
        targets=targets,
        problems=problems,
        out_of_face=out_of_face,
        test_false_bins=test_false_bins,
    )


# ── 两个入口的覆盖守门 ───────────────────────────────────────────────────

# shell 控制符：每个字符都是新命令的起点（`&&` / `||` / `;` / 管道 / 子 shell 括号）。
SHELL_CONTROL_CHARS: Final = frozenset({"&", "|", ";", "(", ")"})

# 命令名前可出现的 shell 前缀词（保留字 / 内建）：`if uv …` / `command uv …` /
# `env VAR=1 uv …` 都是合法调用形态，剥掉后仍算命令位置（避免偏严假红）。
COMMAND_PREFIX_WORDS: Final = frozenset(
    {
        "command",
        "exec",
        "env",
        "nohup",
        "time",
        "if",
        "then",
        "else",
        "elif",
        "do",
        "while",
        "until",
        "!",
    }
)

# `VAR=value` 赋值前缀（`VAR=x uv …`）同样不改变命令位置。
ENV_ASSIGNMENT_PREFIX: Final = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")

# 命令行首的书写装饰（YAML 列表项 / `run:` 键），不含命令语义。
COMMAND_LINE_DECORATIONS: Final = frozenset({"-", "run:"})


# 把一行 shell/YAML 文本切成词：引号只脱壳，引号内的空格与控制符并入同一个词——
# `echo "uv run scripts/test_exec.py"` 因此是「命令词 echo + 一个说明文字词」，不会被
# 误当成 `uv` 接线；而 `uv "scripts/test_exec.py" check` 脱壳后仍是 `uv` 接线。
# 引号外的 `& | ; ( )` 一律切成独立控制符 token（`check;` 也能归到 `check`）。
def tokenize_shell_line(line: str) -> list[str]:
    tokens: list[str] = []
    current = ""
    started = False

    def flush() -> None:
        nonlocal current, started
        if started:
            tokens.append(current)
        current = ""
        started = False

    i = 0
    n = len(line)
    while i < n:
        c = line[i]
        if c == "'" or c == '"':
            j = i + 1
            while j < n:
                inner = line[j]
                if c == '"' and inner == "\\":
                    current += line[j : j + 2]
                    j += 2
                    continue
                if inner == c:
                    break
                current += inner
                j += 1
            started = True
            i = j + 1
            continue
        if c == " " or c == "\t":
            flush()
            i += 1
            continue
        if c in SHELL_CONTROL_CHARS:
            flush()
            tokens.append(c)
            i += 1
            continue
        current += c
        started = True
        i += 1
    flush()
    return tokens


# 剥掉行首装饰、前缀词与环境赋值，返回命令位置起的 argv；全被剥光则为空。
def strip_command_prefixes(tokens: list[str]) -> list[str]:
    for start, token in enumerate(tokens):
        if (
            token in COMMAND_LINE_DECORATIONS
            or token in COMMAND_PREFIX_WORDS
            or ENV_ASSIGNMENT_PREFIX.match(token) is not None
        ):
            continue
        return tokens[start:]
    return []


# 把脚本内容切成**命令位置**的 argv 列表：只看非注释行，行内按 shell 分隔符切段，
# 逐段剥前缀。判据必须落在命令位置——`echo "uv run scripts/test_exec.py"` 这类说明文字
# 虽然含命令字样，命令词却是 `echo`，不构成接线。上一版只判「非注释行含子串」，把
# scripts/test.sh 三条真命令全包成 `echo "…"` 后三个入口一起假绿（#1112 第三轮审查）。
def shell_commands(content: str) -> list[list[str]]:
    commands: list[list[str]] = []
    for raw_line in content.split("\n"):
        line = raw_line.strip()
        if line == "" or line.startswith("#"):
            continue
        segment: list[str] = []

        def flush() -> None:
            nonlocal segment
            argv = strip_command_prefixes(segment)
            if argv:
                commands.append(argv)
            segment = []

        for token in tokenize_shell_line(line):
            if token in SHELL_CONTROL_CHARS:
                flush()
            else:
                segment.append(token)
        flush()
    return commands


# scripts/test.sh 命令行里的非并发入口判据。
@dataclass
class EntryWiring:
    # `cargo test … --test <name>` 的 name 名单。
    tests: set[str]
    # 是否有 `cargo test … --doc`（doc 入口存在性）。
    doc: bool
    # doc 入口的命令行是否带 workspace 范围（`--workspace` / `--all`）。
    # 只查 `--doc` 存在不够：退化成 `cargo test --doc`（非虚拟 workspace 下默认
    # 只作用根包）会让成员 crate 的 doctest 静默漏跑而守门仍然绿。
    doc_workspace: bool
    # 是否**运行**并发入口（`uv run scripts/test_exec.py [run]`）。入口自检行
    # （`… check`）与 `… plan` 不算运行接线——否则把运行行删掉、只留自检行也能假绿。
    parallel: bool


# 解析 scripts/test.sh：只看**命令位置**的命令——注释、说明文字（引号里的命令字样）
# 与 echo 参数都不算接线。每条命令按命令词匹配：`cargo test …` 才贡献
# `--test <name>` / `--doc` 判据，`uv run … scripts/test_exec.py` 才贡献并发入口判据。
def parse_entry_wiring(content: str) -> EntryWiring:
    tests: set[str] = set()
    doc = False
    doc_workspace = False
    parallel = False
    for argv in shell_commands(content):
        if argv[0] == "uv":
            # 并发入口：`uv run scripts/test_exec.py [run]`（`uv run` 的 run 子词在
            # 标记之前，不影响子命令判定；引号形态由词切分归一）；
            # `check` / `plan` 是守门与清单命令，不跑测试，不算运行接线。
            marker = -1
            for index in range(1, len(argv)):
                if argv[index] == PARALLEL_ENTRY_MARKER:
                    marker = index
                    break
            if marker != -1:
                subcommand = next((t for t in argv[marker + 1 :] if not t.startswith("-")), None)
                if subcommand is None or subcommand == "run":
                    parallel = True
            continue
        if len(argv) < 2 or argv[0] != "cargo" or argv[1] != "test":
            continue
        args = argv[2:]
        if DOC_FLAG in args:
            doc = True
            # workspace 范围必须落在同一条 `cargo test` 命令里；`--all` 是别名，精确 token
            # 匹配（`--all-targets` / `--all-features` 不算，与 check_structure.py 同口径）。
            if WORKSPACE_FLAG in args or ALL_FLAG in args:
                doc_workspace = True
        for i in range(len(args)):
            if args[i] == DELEGATED_TARGET_FLAG:
                name = args[i + 1] if i + 1 < len(args) else ""
                if name != "":
                    tests.add(name)
    return EntryWiring(tests=tests, doc=doc, doc_workspace=doc_workspace, parallel=parallel)


@dataclass
class CoverageResult:
    problems: list[str]
    targets: list[DiscoveredTarget]
    # 并发入口承接（lib 单测 + 未豁免 bin 单测 + harness=true 集成测试）。
    parallel: list[DiscoveredTarget]
    # 非并发入口承接的自定义 harness 集成测试（`--test <name>`）。
    delegated: list[DiscoveredTarget]
    # cargo 自有入口承接的 doc-test。
    doc: list[DiscoveredTarget]
    # bin 单测豁免（EXEMPT_BINS 命中的 bin 目标，专属入口承接）。
    exempt: list[DiscoveredTarget]
    out_of_face: list[str]


# 执行器等价性守门（守门规则④）：执行器直接 spawn 测试二进制，**不**复制 cargo
# 运行期注入的环境（`CARGO_MANIFEST_DIR` / `CARGO_PKG_*` / `OUT_DIR` / 动态库搜索
# 路径）。只对齐其中一部分会给「已等价」的假信心，故取 fail loud 处置：测试运行期
# （`env::var` / `env::var_os`；编译期 `env!` 不受影响）不得读这些变量，命中即红，
# 由引入者显式扩展执行器并更新本守门。构建脚本的构建期读取不属测试运行期，跳过。
# 扫描前经守门共享原语库的 Rust 词法掩码（`mask_non_code(…, keep_literals=True)`）掩去
# 注释、保留字面量——注释里提到这些变量名不误报，字面量本身才是判据。
CARGO_RUNTIME_ENV_PATTERN: Final = re.compile(r'\benv::var(?:_os)?\s*\(\s*"(CARGO_[A-Z0-9_]+|OUT_DIR)"')


def cargo_runtime_env_problems(root_dir: str) -> list[str]:
    problems: list[str] = []
    scan_root = os.path.join(root_dir, RUST_ROOT_DIR_NAME)
    if not is_directory(scan_root):
        return problems
    # 遍历机制归守门家族共享单点 walk_text_files（#1680 收口）：target/ 剪枝经 options
    # 注入，build.rs 豁免是本守门扫描边界政策、walk 后按文件名过滤。
    files = [
        f
        for f in walk_text_files(
            scan_root,
            "",
            WalkTextOptions(extensions=RUST_EXTENSIONS, skip_dirs=frozenset({"target"})),
        )
        if f.rel.rsplit("/", 1)[-1] != "build.rs"
    ]
    for file in files:
        masked = mask_non_code(Path(file.abs).read_text(encoding="utf-8"), True)
        for match in CARGO_RUNTIME_ENV_PATTERN.finditer(masked):
            line = line_at(masked, match.start())
            rel = os.path.relpath(file.abs, root_dir)
            problems.append(
                f"✗ 执行器等价性：{rel}:{line} 运行期读 cargo 注入环境变量 `{match.group(1)}`——"
                f"执行器只对齐 cwd 与 RUST_TEST_THREADS，不复制该环境，测试将读到未定义值。"
                f"请改走 cwd 相对定位，或先扩展 scripts/test_exec.py 的 run_child 并更新本守门（issue #1112）"
            )
    return problems


# bin test = false 源码扫描守门：声明 test = false 的 bin 不构建测试二进制，
# 源码里新增 #[test] / #[cfg(test)] / #[tokio::test] 会静默漏跑。扫描前掩注释
# （mask_non_code）——注释里提到测试属性不误报，代码里的真属性才命中（issue #1787）。
TEST_ATTR_PATTERN: Final = re.compile(r"#\[\s*(?:tokio\s*::\s*)?test\b|#\[\s*cfg\s*\(\s*test\b")


def test_false_bin_problems(root_dir: str, bins: list[TestFalseBin]) -> list[str]:
    problems: list[str] = []
    for bin_decl in bins:
        if bin_decl.source is None:
            problems.append(
                f"✗ bin test=false 守门：{bin_decl.package}::{bin_decl.target} 声明 test = false，但无法定位源文件"
                f"（显式 [[bin]] 缺 path 时按 cargo 默认 src/main.rs 或 src/bin/<name>.rs 解析失败）——拒绝猜测"
            )
            continue
        masked = mask_non_code(Path(bin_decl.source).read_text(encoding="utf-8"), True)
        for match in TEST_ATTR_PATTERN.finditer(masked):
            line = line_at(masked, match.start())
            rel = os.path.relpath(bin_decl.source, root_dir)
            problems.append(
                f"✗ bin test=false 守门：{rel}:{line} 出现测试属性 `{match.group(0)}`——"
                f"该 bin 已声明 test = false（不构建测试二进制），新增测试会静默漏跑。"
                f"请把测试移到 lib 或集成测试，或改回 test = true 并重新评估执行面（issue #1787）"
            )
    return problems


# 覆盖守门：目标清单 ⇔ 两个入口 + bin 豁免的并集（互斥且无遗漏）。
def check_coverage(root_dir: str) -> CoverageResult:
    discovery = discover_targets(root_dir)
    problems = list(discovery.problems)
    delegated = [t for t in discovery.targets if t.kind == "test" and t.harness_false]
    doc = [t for t in discovery.targets if t.kind == "doc"]
    exempt = [
        t
        for t in discovery.targets
        if t.kind == "bin"
        and any(e.package == t.package and e.target == t.target for e in EXEMPT_BINS)
    ]
    # bin 单测不入并发入口（与 CI PR 口径对齐）：发现的每个 test = true bin 都必须
    # 登记豁免（test = false 的 bin 根本不产生目标，无需登记）。删登记或新增未声明
    # 的 bin 即红；bin 删除后残留的死登记无副作用，不红。
    for t in discovery.targets:
        if t.kind != "bin":
            continue
        if any(e.package == t.package and e.target == t.target for e in EXEMPT_BINS):
            continue
        problems.append(
            f"✗ 覆盖守门：bin 单测未登记豁免：{target_key(t)}::bin（test = true）会进入并发入口——"
            f"bin 单测不入 PR 执行面（issue #1787 热点 2）：零单测请在 [[bin]] 声明 test = false，"
            f"有单测且有专属承接入口请在 scripts/test_exec.py 的 EXEMPT_BINS 登记"
        )
    exempt_keys = {run_key(t) for t in exempt}
    parallel = [
        t
        for t in discovery.targets
        if t.kind != "doc"
        and not (t.kind == "test" and t.harness_false)
        and run_key(t) not in exempt_keys
    ]

    if not discovery.targets and not problems:
        problems.append("✗ 覆盖守门：工作区测试目标清单为空——拒绝以空集假绿通过")

    wiring = parse_entry_wiring(
        Path(os.path.join(root_dir, TEST_SH_REL)).read_text(encoding="utf-8")
    )
    for target in delegated:
        if target.target not in wiring.tests:
            problems.append(
                f"✗ 覆盖守门：{target_key(target)} 声明 harness = false（自定义 runner），"
                f"但 {TEST_SH_REL} 的非并发入口没有 `{DELEGATED_TARGET_FLAG} {target.target}`——"
                f"两个入口的覆盖范围都不含它，新增自定义 harness 目标被静默漏跑"
            )
    delegated_names = {t.target for t in delegated}
    for name in sorted(wiring.tests):
        if name not in delegated_names:
            problems.append(
                f"✗ 覆盖守门：{TEST_SH_REL} 的 `{DELEGATED_TARGET_FLAG} {name}` 不对应任何"
                f" harness = false 目标（登记失效或拼写漂移：非并发入口的每一项都必须是自定义 harness 目标）"
            )
    if doc and not wiring.doc:
        problems.append(
            f"✗ 覆盖守门：工作区有 {len(doc)} 个 doc-test 目标，但 {TEST_SH_REL} 缺"
            f" `cargo test --workspace {DOC_FLAG}`——doc-test 静默漏跑"
        )
    if doc and wiring.doc and not wiring.doc_workspace:
        problems.append(
            f"✗ 覆盖守门：{TEST_SH_REL} 的 `{DOC_FLAG}` 缺 workspace 范围"
            f"（`{WORKSPACE_FLAG}` 或 `{ALL_FLAG}`）——非虚拟 workspace 下只跑根包的"
            f" doctest，其余成员 crate 的 doc-test 静默漏跑（守门仍绿）"
        )
    if not doc and wiring.doc:
        problems.append(
            f"✗ 覆盖守门：{TEST_SH_REL} 有 `{DOC_FLAG}`，但工作区无 doc-test 目标（登记失效）"
        )
    if not wiring.parallel:
        problems.append(
            f"✗ 覆盖守门：{TEST_SH_REL} 未调用并发入口（{PARALLEL_ENTRY_MARKER}）——"
            f"单元测试与集成测试未由统一执行器调度（删除接线即红）"
        )
    problems.extend(cargo_runtime_env_problems(root_dir))
    problems.extend(test_false_bin_problems(root_dir, discovery.test_false_bins))

    return CoverageResult(
        problems=problems,
        targets=discovery.targets,
        parallel=parallel,
        delegated=delegated,
        doc=doc,
        exempt=exempt,
        out_of_face=discovery.out_of_face,
    )


def summarize_coverage(result: CoverageResult) -> str:
    def count(kind: TargetKind) -> int:
        return len([t for t in result.parallel if t.kind == kind])

    doc_pkgs = len({t.package for t in result.doc})

    def exempt_owners(t: DiscoveredTarget) -> str:
        for e in EXEMPT_BINS:
            if e.package == t.package and e.target == t.target:
                return e.owner
        return "?"

    delegated_runners = " / ".join(t.target for t in result.delegated) or "（无）"
    summary = (
        f"✓ 测试执行覆盖守门：目标 {len(result.targets)} 个 = "
        f"并发入口 {len(result.parallel)}（lib 单测 {count('lib')} + bin 单测 {count('bin')} + 集成测试 {count('test')}）"
        f" ⊎ 非并发入口 {len(result.delegated) + len(result.doc)}"
        f"（cargo 自有 runner：{delegated_runners}"
        f" + doc-test {len(result.doc)} 个，覆盖 {doc_pkgs} 个包）"
    )
    if result.exempt:
        summary += (
            f" ⊎ bin 单测豁免 {len(result.exempt)} 个"
            f"（{' / '.join(f'{t.target} → {exempt_owners(t)}' for t in result.exempt)}）"
        )
    if result.out_of_face:
        summary += f" · 默认执行面外 {len(result.out_of_face)} 个（example/bench，不跑）"
    return summary


# ── 并发执行 ─────────────────────────────────────────────────────────────

@dataclass
class ChildResult:
    code: int
    stdout: str
    stderr: str


async def _drain(stream: asyncio.StreamReader, sink: list[str], stream_to_stderr: bool) -> None:
    while True:
        chunk = await stream.read(65536)
        if not chunk:
            return
        text = chunk.decode("utf-8", errors="replace")
        sink.append(text)
        if stream_to_stderr:
            sys.stderr.write(text)
            sys.stderr.flush()


async def run_child(
    command: str,
    args: list[str],
    *,
    cwd: str,
    env: dict[str, str] | None = None,
    stream_stderr: bool = False,
) -> ChildResult:
    try:
        proc = await asyncio.create_subprocess_exec(
            command,
            *args,
            cwd=cwd,
            env=env if env is not None else dict(os.environ),
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
    except OSError as error:
        return ChildResult(code=-1, stdout="", stderr=f"{error}\n")
    stdout_parts: list[str] = []
    stderr_parts: list[str] = []
    if proc.stdout is None or proc.stderr is None:
        code = await proc.wait()
        return ChildResult(code=code if code is not None else -1, stdout="", stderr="")
    stdout_task = asyncio.create_task(_drain(proc.stdout, stdout_parts, False))
    stderr_task = asyncio.create_task(_drain(proc.stderr, stderr_parts, stream_stderr))
    code = await proc.wait()
    await asyncio.gather(stdout_task, stderr_task)
    return ChildResult(code=code, stdout="".join(stdout_parts), stderr="".join(stderr_parts))


# 构建一次（cargo test --no-run），返回 cargo 报告的可执行测试二进制键集合。
def parse_cargo_message(line: str) -> dict[str, Any] | None:
    if not line.startswith("{"):
        return None
    try:
        loaded = json.loads(line)
    except ValueError:
        return None
    return loaded if isinstance(loaded, dict) else None


async def build_test_binaries(
    cargo: str,
    rust_root_dir: str,
) -> tuple[dict[str, str], list[str], int]:
    start = time.monotonic()
    # CI PR 口径（build.yml）：--lib --test '*' 排除 bin 单测（issue #1787 热点 2）。
    # 零单测 bin 声明 test = false；有单测的 bin（当前 ledger-perf）登记 EXEMPT_BINS。
    result = await run_child(
        cargo,
        [
            "test",
            "--workspace",
            "--lib",
            "--test",
            "*",
            "--no-run",
            "--message-format=json-render-diagnostics",
        ],
        cwd=rust_root_dir,
        stream_stderr=True,
    )
    ms = round((time.monotonic() - start) * 1000)
    problems: list[str] = []
    executables: dict[str, str] = {}
    build_succeeded = result.code == 0
    for line in result.stdout.split("\n"):
        message = parse_cargo_message(line)
        if message is None:
            continue
        reason = message.get("reason")
        if reason == "build-finished":
            build_succeeded = message.get("success") is True
        if reason != "compiler-artifact":
            continue
        profile = message.get("profile")
        if not isinstance(profile, dict) or profile.get("test") is not True:
            continue
        executable = message.get("executable")
        if not isinstance(executable, str):
            continue
        package_name = package_name_from_id(message.get("package_id") or "")
        target = message.get("target")
        target_name = target.get("name") if isinstance(target, dict) else None
        if not package_name or not isinstance(target_name, str) or target_name == "":
            continue
        kinds = target.get("kind") if isinstance(target, dict) else None
        kind_list = [k for k in kinds if isinstance(k, str)] if isinstance(kinds, list) else []
        kind = artifact_target_kind(kind_list)
        if kind is None:
            continue
        executables[f"{package_name}::{target_name}::{kind}"] = executable
    if not build_succeeded:
        problems.append(
            "✗ 构建失败：`cargo test --workspace --lib --test '*' --no-run` 非零退出（编译错误见上方 cargo 输出）"
        )
    return executables, problems, ms


# cargo package_id → 包名（`path+file:///…#<name>@<version>` 形态）。
def package_name_from_id(id_text: str) -> str:
    hash_index = id_text.find("#")
    tail = id_text if hash_index == -1 else id_text[hash_index + 1 :]
    at_index = tail.find("@")
    return tail if at_index == -1 else tail[:at_index]


# cargo artifact 的 target.kind → 目标种类（与发现侧同一口径，核对键才可比）。
def artifact_target_kind(kinds: list[str]) -> TargetKind | None:
    if "test" in kinds:
        return "test"
    if "bin" in kinds:
        return "bin"
    if "lib" in kinds or "rlib" in kinds or "cdylib" in kinds:
        return "lib"
    if "staticlib" in kinds or "dylib" in kinds or "proc-macro" in kinds:
        return "lib"
    return None


# 固定并发度的任务池：全局并行度由执行器统一控制（不依赖各二进制的 libtest 线程数）。
async def run_pool(items: list[str], jobs: int, worker: Callable[[str], Awaitable[None]]) -> None:
    cursor = 0
    width = max(1, min(jobs, len(items)))

    async def runner() -> None:
        nonlocal cursor
        while True:
            index = cursor
            cursor += 1
            if index >= len(items):
                return
            await worker(items[index])

    await asyncio.gather(*(runner() for _ in range(width)))


@dataclass
class TargetOutcome:
    key: str
    ms: int
    code: int
    output: str
    passed: int
    failed: int
    ignored: int


TEST_RESULT_RE: Final = re.compile(
    r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;"
    r" \d+ measured; \d+ filtered out; finished in ([\d.]+)s$",
    re.MULTILINE,
)


def parse_test_result(output: str) -> tuple[int, int, int]:
    passed = 0
    failed = 0
    ignored = 0
    for match in TEST_RESULT_RE.finditer(output):
        passed = int(match.group(1))
        failed = int(match.group(2))
        ignored = int(match.group(3))
    return passed, failed, ignored


def format_seconds(ms: int) -> str:
    return f"{ms / 1000:.2f}s"


# 构建阶段超过该时长 = 测试二进制被重新链接（缓存全命中时构建秒级返回），
# 每个二进制的首次执行会撞上 macOS 一次性安全扫描/页入开销——per-binary 耗时
# 因此虚高，不代表测试或夹具的真实耗时（issue #1787 的误诊源头，提示防再犯）。
FRESH_RELINK_HINT_MS: Final = 3_000


@dataclass(frozen=True)
class RunOptions:
    root_dir: str
    jobs: int


async def run_all(options: RunOptions) -> int:
    root_dir = options.root_dir
    jobs = options.jobs
    coverage = check_coverage(root_dir)
    if coverage.problems:
        print_problems(coverage.problems, "❌ 测试执行覆盖守门失败")
    else:
        print(summarize_coverage(coverage))

    rust_root_dir = os.path.join(root_dir, RUST_ROOT_DIR_NAME)
    cargo = os.environ.get("CARGO") or "cargo"
    print(
        "▶ 构建测试二进制（一次构建全部 target）：cargo test --workspace --lib --test '*' --no-run"
        "（CI PR 口径同款，bin 单测不入面；缓存命中时秒级返回；冷构建的编译耗时见 cargo 输出）"
    )
    executables, build_problems, build_ms = await build_test_binaries(cargo, rust_root_dir)
    print(f"  · 构建阶段 {format_seconds(build_ms)}")
    if build_ms > FRESH_RELINK_HINT_MS:
        print(
            "  · 提示：构建阶段超过 3s = 测试二进制被重新链接——macOS 对新链接的二进制"
            "首次执行有一次性安全扫描/页入开销（大体量二进制可达秒级），下方 per-binary"
            " 耗时含该一次性成本，不代表测试或夹具的真实耗时（issue #1787 的误诊源头）"
        )
    problems = [*coverage.problems, *build_problems]

    # 第四道交叉核对：cargo 实际构建出的测试二进制集合 ⇔ 目标发现清单（lib/集成测试 +
    # 未豁免 bin）。构建命令 --lib --test '*' 不含 bin 单测，豁免 bin 同步剔除。
    exempt_keys = {run_key(t) for t in coverage.exempt}
    expected: dict[str, DiscoveredTarget] = {
        run_key(t): t
        for t in coverage.targets
        if t.kind != "doc" and run_key(t) not in exempt_keys
    }
    for key in expected:
        if key not in executables:
            problems.append(
                f"✗ 执行面漂移：目标清单里的 {key} 未被 cargo 构建"
                f"（cargo test --workspace --lib --test '*' --no-run 无对应可执行文件）"
            )
    for key in executables:
        if key not in expected:
            problems.append(
                f"✗ 执行面漂移：cargo 构建出清单外的测试二进制 {key}——目标发现逻辑漏项，"
                f"该目标会被静默漏跑（请扩展 scripts/test_exec.py 的目标发现）"
            )
    if problems:
        print_problems(problems, "❌ 测试执行前检查失败")

    delegated_keys = {run_key(t) for t in coverage.delegated}
    queue = sorted(key for key in expected if key not in delegated_keys)
    cpu_count = os.cpu_count() or 1
    width = max(1, min(jobs, len(queue)))
    print(
        f"▶ 并发执行 {len(queue)} 个测试二进制（全局并行度 {width} = "
        f"min(并行度上限 {jobs}（默认 CPU 数 {cpu_count}）, 待跑二进制数 {len(queue)})；"
        f"每个二进制 RUST_TEST_THREADS=1（只约束 libtest 线程），libtest 线程数 = {width}）"
    )
    if jobs > cpu_count:
        print(
            f"  · 提示：--jobs {jobs} 超过 CPU 数 {cpu_count}——这是显式超订（默认取 CPU 数），"
            f"刻意压测/限流时才这样传"
        )
    if coverage.delegated:
        print(
            f"  · 非并发入口承接 {len(coverage.delegated)} 个"
            f"（{' / '.join(t.target for t in coverage.delegated)}）+ "
            f"doc-test {len(coverage.doc)} 个，由 {TEST_SH_REL} 的 cargo 自有入口执行"
        )

    start = time.monotonic()
    outcomes: list[TargetOutcome] = []

    async def worker(key: str) -> None:
        target = expected.get(key)
        executable = executables.get(key)
        if target is None or executable is None:
            return
        item_start = time.monotonic()
        result = await run_child(
            executable,
            [],
            # cwd 与 cargo 口径一致（各包根，不是 workspace 根）：成员 crate 的测试若
            # 用相对路径，直接以 workspace 根为 cwd 会读到不同位置（Spec 审查发现）。
            # cargo 运行期注入的环境（CARGO_MANIFEST_DIR / CARGO_PKG_* / OUT_DIR /
            # 动态库搜索路径）**不复制**——部分复制会制造「已等价」的假信心；现状等价
            # 由守门规则④兜底（测试运行期读这些变量即红，见 cargoRuntimeEnvProblems）。
            cwd=target.cwd,
            env={**os.environ, "RUST_TEST_THREADS": "1"},
        )
        ms = round((time.monotonic() - item_start) * 1000)
        passed, failed, ignored = parse_test_result(f"{result.stdout}\n{result.stderr}")
        outcomes.append(
            TargetOutcome(
                key=key,
                ms=ms,
                code=result.code,
                output=f"{result.stdout}{result.stderr}",
                passed=passed,
                failed=failed,
                ignored=ignored,
            )
        )
        mark = "✓" if result.code == 0 else "✗"
        detail = f"{passed} passed" if result.code == 0 else f"exit {result.code}"
        ignored_note = f", {ignored} ignored" if ignored > 0 else ""
        print(f"  {mark} {key} {format_seconds(ms)}（{detail}{ignored_note}）")

    await run_pool(queue, jobs, worker)
    wall_ms = round((time.monotonic() - start) * 1000)

    failed = sorted((o for o in outcomes if o.code != 0), key=lambda o: locale_sort_key(o.key))
    if failed:
        for outcome in failed:
            print(
                f"\n──── ✗ {outcome.key}（exit {outcome.code}，{format_seconds(outcome.ms)}）────",
                file=sys.stderr,
            )
            print(outcome.output.rstrip(), file=sys.stderr)

    sum_ms = sum(o.ms for o in outcomes)
    slowest: TargetOutcome | None = None
    for outcome in outcomes:
        if slowest is None or outcome.ms > slowest.ms:
            slowest = outcome
    totals_passed = sum(o.passed for o in outcomes)
    totals_failed = sum(o.failed for o in outcomes)
    totals_ignored = sum(o.ignored for o in outcomes)
    pass_count = len(outcomes) - len(failed)
    print(
        f"\n{'✅' if not failed else '❌'} 并发入口结果：{pass_count}/{len(outcomes)} 个二进制通过"
        f"（{totals_passed} passed / {totals_failed} failed / {totals_ignored} ignored）"
    )
    slowest_note = (
        f"；最长 {slowest.key} {format_seconds(slowest.ms)}" if slowest is not None else ""
    )
    print(
        f"  · 墙钟 {format_seconds(wall_ms)}（并行度 {width}）；各二进制耗时之和 {format_seconds(sum_ms)}；"
        f"串行等价 ≈ {format_seconds(sum_ms + build_ms)}；加速 {(sum_ms / max(wall_ms, 1)):.2f}x"
        f"{slowest_note}"
    )
    return 0 if not failed else 1


def print_problems(problems: list[str], title: str) -> NoReturn:
    for problem in problems:
        print(problem, file=sys.stderr)
    print(f"{title}：{len(problems)} 处问题（issue #1112）", file=sys.stderr)
    sys.exit(1)


# ── CLI ─────────────────────────────────────────────────────────────────

@dataclass(frozen=True)
class CliOptions:
    command: str
    root_dir: str
    jobs: int


def parse_args(argv: list[str], default_root: str) -> CliOptions:
    command = "run"
    root_dir = default_root
    env_jobs = os.environ.get("LEDGER_TEST_JOBS", "").strip()
    try:
        jobs = int(env_jobs) if env_jobs != "" else 0
    except ValueError:
        jobs = 0
    if jobs == 0:
        jobs = os.cpu_count() or 1
    i = 0
    while i < len(argv):
        arg = argv[i]
        if arg == "run" or arg == "check" or arg == "plan":
            command = arg
        elif arg == "--jobs":
            i += 1
            raw = argv[i] if i < len(argv) else None
            try:
                value = int(raw.strip()) if raw is not None else 0
            except ValueError:
                value = 0
            if value < 1:
                print(
                    f"✗ --jobs 需要一个正整数，收到：{raw if raw is not None else '（缺参数）'}",
                    file=sys.stderr,
                )
                sys.exit(2)
            jobs = value
        elif arg == "--root":
            i += 1
            root_dir = os.path.abspath(argv[i] if i < len(argv) else "")
        else:
            print(
                f"✗ 未知参数：{arg}（用法：uv run scripts/test_exec.py [run|check|plan] [--jobs N] [--root DIR]）",
                file=sys.stderr,
            )
            sys.exit(2)
        i += 1
    return CliOptions(command=command, root_dir=root_dir, jobs=jobs)


def main() -> None:
    default_root = str(Path(__file__).resolve().parent.parent)
    options = parse_args(sys.argv[1:], default_root)
    if options.command == "plan":
        discovery = discover_targets(options.root_dir)
        if discovery.problems:
            print_problems(discovery.problems, "❌ 目标发现失败")
        exempt_keys = {
            run_key(t)
            for t in discovery.targets
            if t.kind == "bin"
            and any(e.package == t.package and e.target == t.target for e in EXEMPT_BINS)
        }
        for target in discovery.targets:
            if target.harness_false:
                note = "  harness=false（非并发入口）"
            elif run_key(target) in exempt_keys:
                note = "  bin 单测豁免（EXEMPT_BINS，不入并发入口）"
            else:
                note = ""
            print(f"{target.kind:<5} {target_key(target)}{note}")
        return
    if options.command == "check":
        coverage = check_coverage(options.root_dir)
        if coverage.problems:
            print_problems(coverage.problems, "❌ 测试执行覆盖守门失败")
        print(summarize_coverage(coverage))
        return
    sys.exit(asyncio.run(run_all(RunOptions(root_dir=options.root_dir, jobs=options.jobs))))


# 仅直接运行时执行 main；被其他工具 import 时只取导出的发现/守门逻辑。
if __name__ == "__main__":
    main()
