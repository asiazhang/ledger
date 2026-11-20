#!/usr/bin/env -S uv run --quiet
# 结构守门（issue #396 / ADR-0056；模型域化禁令 issue #424 / ADR-0059 决策 6）。
#
# 数据分界（ADR-0056 决策 4 修订注记 / #1591 定案）：**事实有权威源就投影，政策
# 无源就显式**——模块面的 expected 由各 crate `lib.rs` 的 `mod` 声明投影、与磁盘
# 顶层模块双向全等（孤儿文件/目录照红）；政策核显式声明：`CRATES`（成员 / 包名 /
# dir / layer）、允许依赖方向与方向禁令、认许边台账。手写模块清单、登记表与
# #1593 的「手写 × 派生并行核对」期随之退役（#1595），#1589 的清单拆行/定序工件
# 一并清除——同域串行票（建模块文件 + `lib.rs` 加 `mod` + 写文件头）不再触碰本脚本。
#
# 分层规则：壳 → 域 → 基础设施，域永不依赖壳。壳 = 根包 nsh-app（#2083 切轨后
# 唯一壳，CRATES 以 dir "." 登记）；壳层模块面不整册登记（SHELL 层模块分层不
# 投影，域/基建/协议 crate 的模块面由各自 lib.rs 投影）。旧壳 tauri-app 条目、
# 根包 src 白名单与 tauri-emit / tauri-host 门已随切轨退役；「全仓无 tauri」
# 形状面（Cargo 清单；前端死路径面已随 Web 前端退役删除 #2171/#2172）由
# scripts/check_no_tauri.py 独立辖。
#
# 测试豁免（ADR-0056 决策 5）：外挂测试模块/目录（tests.rs 文件、tests/ 目录）
# 不参与守门——BDD/单元 fixture 合法引用壳层入口，不制造虚假违规；内联
# `#[cfg(test)]` 模块不豁免。条目路径缺失或扫不到非测试 Rust 文件即红（fail loud）。
#
# 扫描边界：文本级扫描，注释与字符串/char 字面量掩码后匹配 `commands::` 路径引用
# 与 `commands as` 别名引入；经别名改名的间接引用文本不可达，靠评审兜底。
# 报文形态（T2-2）：定位（文件:行）+ 分层/区 + 规则名，规则级 ADR/issue 指针住在
# 规则里（一处编号），不插值条目注记——注记的唯一作者位是 crate `lib.rs` 的 `//!`
# 与模块文件头（ADR-0113 决策 5）。
#
# `mod` 声明形状判定表（ADR-0056 决策 4 / ADR-0113 决策 7 / ADR-0111 决策 5 修订
# 注记）：`pub mod x;` / `mod x;` / `pub(crate) mod x;` 与可见性前置的声明，带
# lint 属性（allow/warn/deny/forbid/expect）、doc 属性或 `#[cfg]` / `#[cfg_attr]`
# 门控者一律计入 expected；`#[cfg(test)] mod tests;` 豁免（按目标模块名 tests 判，
# 与磁盘枚举 is_test_file 同规）；`#[path]`（含 `cfg_attr` 夹带 path）/ `include!` /
# 内联模块块 `mod x { … }` / 上述之外认不出的属性链 fail loud（报文给登记指引），
# 不静默跳过。crate 根 `lib.rs` 自身不是 `mod` 声明，不入 expected——它是声明与
# 再导出面。
#
# 跨 crate 依赖方向（#1596 / ADR-0056 决策 4 修订注记、ADR-0071 与 ADR-0101 修订
# 注记）：跨 crate 的越界方向只有两种违规形态，都不需要源码文本扫描——未声明依赖
# 即编译失败（cargo 强于文本扫描：`use crate::x as y` 别名改写对文本不可达、
# 编译期逃不掉）；已声明的越界方向由 crate 边界核对按 `Cargo.toml` 声明面判定
#（层秩 + 业务域→多端同步域禁令，见 CRATES）。据此退役的四族源码扫描：基础设施
# →域（ADR-0071 决策 6）、协议 crate→壳/域（#1089）、业务域→同步域（ADR-0101
# 决策 4b）、域间禁边（#1090，退役前已是空集——空集规则在活化前零测试覆盖，读者
# 以为在守，故代码路径整体删除）。cargo 唯一盲区是 dev-dependency 环（测试目标与
# 生产依赖图分离，cargo 放行）：基础设施对更高层 workspace crate 的
# `[dev-dependencies]` 逐条留痕于 INFRA_DOMAIN_ALLOWED_EDGES（目标 crate + 成因 +
# ADR 指针），清单之外的声明即红——方向禁令的判定载体自 #1596 起收成声明面。
# 同 crate 内的方向规则只剩基础设施块间禁边（原语 ← db ← boot 单向，db 不得引用
# boot / signals；ADR-0111 决策 4，认许边逐条留痕 INFRA_BLOCK_ALLOWED_EDGES）；
# 旧壳白名单面的壳层反向依赖扫描随白名单退役（#2083，壳 src 现由模块投影豁免
# 同款政策辖——SHELL 层不投影模块面）。
#
# 符号调用方闭集（ADR-0011 决策 3 + 2026-09-22 修订 ① / #1692）：当期入口
# `convert_to_native_current`（含 `try_` 软形态，issue #1797）的生产调用方白名单
# CONVERT_CURRENT_CALLERS 逐条留痕（9 个读路径消费面；压测工具注记已随
# #2082 ledger-perf 迁居 nsh-app 同包 bin 摘除——SHELL 层 crate 模块面不在
# 本扫描范围，与 nsh cross_book.rs 同规），白名单外调用即红；
# 测试代码按 `/tests/` 路径约定豁免（ADR-0056 决策 5 同规），`fn` 定义面与注释/
# 字符串提及经掩码与定义判定排除；在册成员文件缺失或零调用同样红（清单漂移
# fail loud、拒绝空集假绿）。别名改写等文本不可达形态靠评审兜底（同壳层扫描
# 边界）。
#
# 模型域化禁令（ADR-0059 决策 6/T7 / #424 收口）：① 全局模型模块路径残留即红；
# ② 域模型 glob 再导出（域接缝 `pub use model::*`、跨域拍平、模型文件/目录内的
# glob 聚合）即红，所有权必须逐类型可见；③ 产品代码手写
# `BEGIN`/`COMMIT`/`ROLLBACK` 即红——靶形态落在字符串里，扫描保留字符串、只掩码
# 注释；唯一合法住址 `db/tx_scope.rs`（issue #1014）。
#
# 交易域区级层序（ADR-0113 决策 2/3/7 / #1181 / #1597）：区归属按目录约定投影——
# zone = crate 顶层区目录（TRANSACTION_ZONE_BY_DIR 一表单点声明，per-module zone
# 字段退役）；crate 根顶层单文件必须进区目录、未登记区归属的顶层目录即红（fail
# loud，不允许「根级单文件默认为共享语义」——那会给写路径模块留静默错分类的
# 洞）。引用形态掩码后匹配 `super::` / `crate::` 前缀 + 目标首段（含花括号列举
# 逐条展开）；设计意图边逐条留痕 TRANSACTION_ZONE_ALLOWED_EDGES，清单之外即红。
#
# crate 边界核对（spec #1086 / issue #1087 门禁前置）：`CRATES` 是 workspace 成员、
# 分层与允许依赖方向的唯一政策核——成员目录（crates/*）与 CRATES 双向全等（新 crate
# 未登记即红）；每个成员须写 `[lints] workspace = true` 继承六件套门禁（漏写即红）；
# 依赖方向按壳 → 域 → 基础设施单向核对；业务域 crate 不得生产依赖多端同步域
# （ADR-0101 决策 4b / #1107）；scripts/check.sh、scripts/test.sh、scripts/lint-fix.sh
# 与 CI workflow 的 cargo clippy/test/fmt 命令须显式 `--workspace` 或 `--all`（非虚拟
# workspace 下默认只作用于根包，缺范围参数会静默漏检成员；`--all-targets` 等 `--all*`
# 旗标不算范围）；引号内的命令字样是说明文字不算命令面，宿主里一条命令都核不到即红
# （空集假绿）。另核 test_utils 生产编译门（ADR-0111 决策 5 / #1132）、投资五节锚点
# 门（#1185）与 http 投影 feature 门（ADR-0111 决策 5 / #1133）。tauri 面不在本门
# （清单形状归 scripts/check_no_tauri.py、依赖全图归 cargo tree 命令
# 门，#2083 切轨后 tauri-emit / tauri-host 门与 DOMAIN_TAURI_ALLOWED 整体退役）。
#
# Python 化 + uv 运行时（issue #734 / ADR-0083 修订注记（#2174））：类型经
# scripts/check_scripts_types.py 门槛检查；调用方式 `uv run scripts/check_structure.py`。
# 默认校验本仓库；测试可传位置参数指向夹具：
# uv run scripts/check_structure.py [src-dir] [src-app-dir]

import os
import re
import stat as stat_module
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Final, Sequence

# 家族共享原语住 scripts/gate_primitives.py（#1680 库归库、门归门）：本守门只保留
# 政策核与门专属机制；walk_text_files / line_at / mask_non_code / RUST_EXTENSIONS
# 由库导出，兄弟脚本直接 import 库、不再从本门「顺手」取件。
from gate_primitives import WalkTextOptions, WalkedFile, RUST_EXTENSIONS, has_extension, line_at, mask_non_code, read_dir_entries, walk_text_files


@dataclass(frozen=True)
class ModuleEntry:
    """模块扫描条目（crate 派生模块面共用，ADR-0056 决策 4）：路径 + 分层。"""

    path: str
    layer: str


class LAYER:
    """分层词汇（白名单 layer 字段取值）：比较侧单一来源，防字面量漂移。"""

    DOMAIN: Final[str] = "域目录"
    INFRA: Final[str] = "基础设施"
    PROTOCOL: Final[str] = "协议"


# 基础设施 crate 的模块根（相对 src-app）：`CRATES` 的 ledger-infra.dir + /src。
INFRA_SRC_REL: Final[str] = "crates/infra/src"


class TRANSACTION_ZONE:
    """交易域 crate 四区词汇（ADR-0113 决策 2/3）：区级层序判向共用，字面量单一来源。"""

    SHARED: Final[str] = "共享语义"
    SEAM: Final[str] = "跨域接缝"
    WRITE: Final[str] = "写路径"
    READ: Final[str] = "读路径"


TRANSACTION_ZONE_BY_DIR: Final[dict[str, str]] = {
    "amount": TRANSACTION_ZONE.SHARED,
    "command": TRANSACTION_ZONE.SHARED,
    "model": TRANSACTION_ZONE.SHARED,
    "read": TRANSACTION_ZONE.READ,
    "seams": TRANSACTION_ZONE.SEAM,
    "shared": TRANSACTION_ZONE.SHARED,
    "write": TRANSACTION_ZONE.WRITE,
}

# 交易域区级层序（ADR-0113 决策 3）：允许依赖方向唯一——写路径/读路径 → 跨域
# 接缝 → 共享语义。同区互依合法；跨区时秩大者方可依赖秩小者；写路径与读路径
# 同秩，互不依赖由「跨区且秩不大即红」承担。
TRANSACTION_ZONE_RANK: Final[dict[str, int]] = {
    TRANSACTION_ZONE.SHARED: 0,
    TRANSACTION_ZONE.SEAM: 1,
    TRANSACTION_ZONE.WRITE: 2,
    TRANSACTION_ZONE.READ: 2,
}


@dataclass(frozen=True)
class TransactionZoneEdge:
    """交易域区级认许边条目：拥有文件的区目录名 + 目标区目录名 + 成因留痕。"""

    dir: str
    target: str
    reason: str


# 交易域区级认许边（ADR-0113 决策 3 / #1181）：原形状上与区级层序冲突的既有
# 设计意图边逐条留痕于本脚本（与 INFRA_DOMAIN_ALLOWED_EDGES 同款纪律），精确到
# 区目录名 + 目标区目录名（目录约定 / #1597 后的判向单位），附 ADR 指针；清单
# 之外的区级反向引用一律红。两条原反边已由重排票（#1182）消除（本位币接缝归
# 共享语义区、model→writer 转换归写路径），故本清单归空——再出现区级反向引用
# 即红，不设认许。
TRANSACTION_ZONE_ALLOWED_EDGES: Final[tuple[TransactionZoneEdge, ...]] = ()


class CRATE_LAYER:
    """crate 分层词汇（crate 边界核对用）：壳 → 域 → 基础设施单向。
    与上面的 `LAYER`（单 crate 内的**模块路径**分层：域目录 / 基础设施）刻意分开——
    两者是不同粒度的事实源，同名值不合并（合并只会让任一侧语义被动漂移）。
    """

    SHELL: Final[str] = "壳"
    DOMAIN: Final[str] = "域"
    INFRA: Final[str] = "基础设施"
    PROTOCOL: Final[str] = "协议"


# crate 依赖方向优先级（数值大者可依赖数值小者）：壳 → 域 → 基础设施 → 协议
#（#1089：协议 crate 是全部业务域与同步域的共享底座，自身仍消费基础设施——
# 基础设施之下再无更底层）。
CRATE_LAYER_RANK: Final[dict[str, int]] = {
    CRATE_LAYER.SHELL: 3,
    CRATE_LAYER.DOMAIN: 2,
    CRATE_LAYER.PROTOCOL: 1,
    CRATE_LAYER.INFRA: 0,
}


@dataclass(frozen=True)
class CrateEntry:
    """crate 政策核条目（spec #1086 / issue #1087，ADR-0112 决策 4 修订）：dir 相对 src-app；根包（壳）以 dir "." 登记。"""

    name: str
    dir: str
    layer: str


# crate 政策核：workspace 成员、包名、dir、分层与允许依赖方向的唯一显式事实源
# （ADR-0112 决策 4 修订注记——模块面已改由各 crate `lib.rs` 投影，条目注记归
# crate `//!`）。结构守门据此核对成员登记、门禁继承与依赖方向；每拆一个域 crate
# 按 name 字节序插入一行。报文只给定位与规则级指针，不插值注记。
CRATES: Final[tuple[CrateEntry, ...]] = (
    CrateEntry(name="ledger-accounts", dir="crates/accounts", layer=CRATE_LAYER.DOMAIN),
    CrateEntry(name="ledger-backup", dir="crates/backup", layer=CRATE_LAYER.DOMAIN),
    CrateEntry(name="ledger-budget", dir="crates/budget", layer=CRATE_LAYER.DOMAIN),
    CrateEntry(name="ledger-categories", dir="crates/categories", layer=CRATE_LAYER.DOMAIN),
    CrateEntry(name="ledger-currencies", dir="crates/currencies", layer=CRATE_LAYER.DOMAIN),
    CrateEntry(name="ledger-dashboard", dir="crates/dashboard", layer=CRATE_LAYER.DOMAIN),
    CrateEntry(name="ledger-infra", dir="crates/infra", layer=CRATE_LAYER.INFRA),
    CrateEntry(name="ledger-investment", dir="crates/investment", layer=CRATE_LAYER.DOMAIN),
    CrateEntry(name="ledger-item", dir="crates/item", layer=CRATE_LAYER.DOMAIN),
    CrateEntry(name="ledger-market-sync", dir="crates/market-sync", layer=CRATE_LAYER.DOMAIN),
    CrateEntry(name="ledger-merchants", dir="crates/merchants", layer=CRATE_LAYER.DOMAIN),
    CrateEntry(
        name="ledger-physical-asset", dir="crates/physical-asset", layer=CRATE_LAYER.DOMAIN
    ),
    CrateEntry(name="ledger-policy", dir="crates/policy", layer=CRATE_LAYER.DOMAIN),
    CrateEntry(name="ledger-reports", dir="crates/reports", layer=CRATE_LAYER.DOMAIN),
    CrateEntry(
        name="ledger-savings-goal", dir="crates/savings-goal", layer=CRATE_LAYER.DOMAIN
    ),
    CrateEntry(name="ledger-scheduled", dir="crates/scheduled", layer=CRATE_LAYER.DOMAIN),
    CrateEntry(name="ledger-sync-engine", dir="crates/sync-engine", layer=CRATE_LAYER.DOMAIN),
    CrateEntry(
        name="ledger-sync-protocol", dir="crates/sync-protocol", layer=CRATE_LAYER.PROTOCOL
    ),
    CrateEntry(name="ledger-transaction", dir="crates/transaction", layer=CRATE_LAYER.DOMAIN),
    # NSH 原生壳 = workspace 根包（issue #2017 / ADR-0145；#2083 切轨后唯一壳）：
    # 依赖方向沿用壳→域→基础设施→协议，直调域栈、不经 IPC。dir "." 即 workspace
    # 根（src-app/Cargo.toml），[workspace] 段与根包同册（成员 glob crates/*）。
    CrateEntry(name="nsh-app", dir=".", layer=CRATE_LAYER.SHELL),
)

# 成员目录约定（workspace glob）：新增 crate 只需落在此目录下即自动入 workspace。
MEMBER_DIR_GLOB: Final[str] = "crates/*"


def module_layer_of(crate_layer: str) -> str | None:
    """crate 分层 → 模块路径分层（壳层返回 None：根包模块面不整册登记）。"""
    if crate_layer == CRATE_LAYER.DOMAIN:
        return LAYER.DOMAIN
    if crate_layer == CRATE_LAYER.INFRA:
        return LAYER.INFRA
    if crate_layer == CRATE_LAYER.PROTOCOL:
        return LAYER.PROTOCOL
    return None


@dataclass(frozen=True)
class CrateModuleTarget:
    """crate 模块扫描目标（#1595）：模块面的分层自 crate 政策核投影（`CRATES` 每
    crate 一条 layer），模块清单 expected 由各 crate `lib.rs` 的 `mod` 声明投影。
    壳层（根包 nsh-app，dir "."）不整册登记模块面（module_layer_of 对 SHELL 返回
    None）——壳层模块自由视图组住址，无模块级分层政策。
    """

    # crate 名（报文定位用；与 Cargo.toml 包名一致由 crate 边界核对保证）
    crate: str
    # 模块根（相对 src-app）
    src_rel: str
    # 模块路径分层（自 crate layer 投影：域 → 域目录，基础设施 → 基础设施，协议 → 协议）
    layer: str


# crate 模块扫描目标表（自 `CRATES` 政策核投影，无第二事实源）。
CRATE_MODULE_TARGETS: Final[tuple[CrateModuleTarget, ...]] = tuple(
    CrateModuleTarget(crate=crate.name, src_rel=f"{crate.dir}/src", layer=layer)
    for crate in CRATES
    if (layer := module_layer_of(crate.layer)) is not None
)

# 易 panic 构造六件套（ADR-0060）：workspace 级声明的键集（单一来源）。
PANIC_LINT_KEYS: Final[tuple[str, ...]] = (
    "unwrap_used",
    "expect_used",
    "panic",
    "todo",
    "unimplemented",
    "unreachable",
)

# 显式声明 workspace 范围的 cargo 命令宿主（静态检查与测试命令覆盖全成员）。
WORKSPACE_COMMAND_FILES: Final[tuple[str, ...]] = (
    "scripts/check.sh",
    "scripts/test.sh",
    "scripts/lint-fix.sh",
    # 执行器程序化拼装 cargo 命令（`run_child(cargo, ['test', '--workspace', …])`）：
    # 宿主形态是 .py 而非 shell。该宿主的真实命令面是**数组形态**（cargo 标识符 + 数组
    # 字面量），逐行字面量扫描只看得见说明文字（print 模板串），故另配数组形态核对
    # （check_py_cargo_arrays）真正约束命令面，见 #1112 第三轮审查 P2。
    "scripts/test_exec.py",
    ".github/workflows/build.yml",
)

# workspace 范围参数：`--workspace` 或 `--all`（精确词，防止 `--all-targets` 假绿）。
WORKSPACE_SCOPE_PATTERN: Final[re.Pattern[str]] = re.compile(r"(?:^|\s)--workspace(?:\s|$)")
ALL_SCOPE_PATTERN: Final[re.Pattern[str]] = re.compile(r"(?:^|\s)--all(?:\s|$)")

# cargo 命令词（workspace 范围核对的适用范围）。
CARGO_SUBCOMMANDS: Final[tuple[str, ...]] = ("clippy", "test", "fmt")

# 逐行命令扫描（shell / workflow 宿主共用）。
CARGO_COMMAND_PATTERN: Final[re.Pattern[str]] = re.compile(r"\bcargo\s+(clippy|test|fmt)\b")


# shell / YAML 引号与注释掩码（逐行，不跨行）：把解释性文字换成空格、保留列位置。
# 引号里的 `cargo test …` 是说明文字（`echo "( cd src-app && cargo test … )"`），
# `#` 之后是注释，都不构成命令面；不掩码会把 echo 字符串当成命令，把三条真命令全包成
# echo 后核对仍然全绿（#1112 第三轮审查 P1）。逐行做也保证一个未闭合引号不会吞掉
# 后续行。`.ts` 宿主不掩码——那里的命令面恰恰是字符串字面量。
def mask_shell_quoted(text: str) -> str:
    out_lines: list[str] = []
    for line in text.split("\n"):
        chars = list(line)
        n = len(chars)

        def blank(from_: int, to: int) -> None:
            for k in range(from_, min(to, n)):
                chars[k] = " "

        i = 0
        while i < n:
            c = chars[i]
            # `#` 起注释：行首或前面是空白（`foo#bar` 不是注释）
            if c == "#" and (i == 0 or re.search(r"\s", chars[i - 1]) is not None):
                blank(i, n)
                break
            if c != '"' and c != "'":
                i += 1
                continue
            j = i + 1
            while j < n:
                if c == '"' and chars[j] == "\\":
                    j += 2
                    continue
                if chars[j] == c:
                    break
                j += 1
            blank(i, min(j + 1, n))
            i = j + 1
        out_lines.append("".join(chars))
    return "\n".join(out_lines)


@dataclass(frozen=True)
class PyCargoArray:
    """`.py` 宿主里程序化拼装的 cargo 命令：数组起始行（1-based）+ 数组内的字符串元素。"""

    line: int
    elements: list[str]


# 注释行（`.py` 的行注释 `#`，与 shell 宿主同款由主循环先行剔除）：注释里的命令字样
# 不算命令面。
def is_py_comment_line(trimmed: str) -> bool:
    return trimmed == "" or trimmed.startswith("#")


# 找 `.py` 宿主的数组形态 cargo 命令 `run_child(cargo, ['test', '--workspace', …])`：
# `cargo` 标识符 + 逗号之后，数组要么同行，要么紧随的下一个非空行以 `[` 开头（仓内
# 多行调用形态）；再向后收括号，取出引号字符串元素。不这样限定就会把 `f(cargo, x)`
# 之后随便一个数组误认成命令面。
def py_cargo_arrays(source: str) -> list[PyCargoArray]:
    lines = source.split("\n")
    found: list[PyCargoArray] = []
    i = 0
    while i < len(lines):
        raw = lines[i]
        if is_py_comment_line(raw.strip()):
            i += 1
            continue
        marker = re.search(r"\bcargo\s*,", raw)
        if marker is None:
            i += 1
            continue
        text = raw[marker.end() :]
        start_line = i
        if "[" not in text:
            j = i + 1
            while j < len(lines) and lines[j].strip() == "":
                j += 1
            if j >= len(lines) or not lines[j].strip().startswith("["):
                i += 1
                continue
            start_line = j
            text = lines[j]
            i = j
        open_idx = text.index("[")
        j = i
        while text.find("]", open_idx + 1) == -1 and j + 1 < len(lines):
            j += 1
            text += "\n" + lines[j]
        close = text.find("]", open_idx + 1)
        if close == -1:
            i += 1
            continue
        elements: list[str] = []
        for m in re.finditer(r"'([^']*)'|\"([^\"]*)\"", text[open_idx : close + 1]):
            if m.group(1) is not None:
                elements.append(m.group(1))
            elif m.group(2) is not None:
                elements.append(m.group(2))
            else:
                elements.append("")
        found.append(PyCargoArray(line=start_line + 1, elements=elements))
        i += 1
    return found


# `.py` 宿主的数组形态 cargo 命令核对（#1112 第三轮审查 P2）：逐行字面量扫描只看得见
# 说明文字（print 模板串），真实命令面是数组参数——首元素是 cargo 子命令时必须
# 带 workspace 范围。返回命中条数。
def check_py_cargo_arrays(rel: str, source: str, problems: list[str]) -> int:
    hits = 0
    for arr in py_cargo_arrays(source):
        subcommand = arr.elements[0] if arr.elements else None
        if subcommand is None or subcommand not in CARGO_SUBCOMMANDS:
            continue
        hits += 1
        if "--workspace" in arr.elements or "--all" in arr.elements:
            continue
        problems.append(
            f"✗ workspace 命令覆盖：{rel}:{arr.line} cargo {subcommand} 数组形态缺 '--workspace'"
            "（非虚拟 workspace 下默认只作用于根包，会静默漏检成员 crate）\n"
            f"    {' '.join(arr.elements)}"
        )
    return hits


@dataclass(frozen=True)
class InfraDevDepEdge:
    """认许边条目（#1596 换载体）：目标 workspace crate 名 + 成因留痕。"""

    crate: str
    reason: str


# 基础设施 dev-dependency 认许边（ADR-0071 决策 6 修订注记 / #1596）：跨 crate
# 源码文本扫描退役后，基础设施→域方向的判定载体收成声明面——`[dependencies]`
# 的越界由 crate 层秩核对拦下、未声明依赖即编译失败，只有 `[dev-dependencies]`
# 是 cargo 盲区（测试目标与生产依赖图分离，cargo 对 dev-dep 环放行）。
#
# 故本台账逐条留痕基础设施对**更高层** workspace crate 的 dev-dependency：
# 目标是测试专用边（测试目标可见、生产依赖图不含），条目附 ADR 指针与成因；
# 清单之外的声明即红。台账形状自 #1596 起由「文件 + 目标域目录名」收成
# 「目标 crate 名」——源码形态由编译器判定，声明面才是唯一可核对处。
INFRA_DOMAIN_ALLOWED_EDGES: Final[tuple[InfraDevDepEdge, ...]] = (
    InfraDevDepEdge(
        crate="nsh-app",
        reason=(
            "ADR-0084 迁移状态段 + ADR-0071 决策 6：内联 cfg(test) 测试经根包统一测试工厂 "
            "test_support 建库/取常量与种子（#758 收口；#2083 切轨后根包 = nsh-app），"
            "测试专用边、非产品依赖"
        ),
    ),
    InfraDevDepEdge(
        crate="ledger-backup",
        reason=(
            "ADR-0111 决策 4 / #1134：db 测试注册备份域 after_commit 置脏钩子并读回 "
            "AutoBackupState 对拍——跨域接线只在测试目标验证，生产依赖图不含本边"
        ),
    ),
    InfraDevDepEdge(
        crate="ledger-transaction",
        reason=(
            "spec #1086 明文裁决的测试专用环：db 测试经核心交易域金额口径"
            "（convert_to_native_current）对拍，生产依赖图不含本边"
        ),
    ),
    InfraDevDepEdge(
        crate="ledger-accounts",
        reason=(
            "ADR-0067 / #1093：db 测试经账户域余额口径对拍缓存刷新，生产依赖图不含本边"
        ),
    ),
)

# crate 内块间禁边（ADR-0111 决策 4 / #1134；#1108 修订）：子目录级反向依赖
# 断言——原语 ← db/ ← boot/ 单向，events / signals / settings 是被各层引用的
# 共享接缝；db 不得引用 boot / signals。shell_support 靶随 #1108 迁出根包退役
#（块在 crate 内已不存在，残留引用归编译期拒绝）。键 = 拥有该文件的块
#（路径首段），值 = 禁止引用的目标块；顶层单文件（ids / error / fs_util 等
# 原语与共享接缝）不受块间禁边约束。
INFRA_BLOCK_FORBIDDEN: Final[dict[str, tuple[str, ...]]] = {
    "db": ("boot", "signals"),
}


# 块间依赖形态：crate 根前缀 + 目标块名（掩码后匹配），
#  再随 `::`（路径引用）、` as `（别名引入）或 `;`（模块自身导入）；\b 防前缀吞
#  匹配，`\{?\s*` 容纳花括号列举首段（use crate::{boot::x, …}）；花括号列举
#  非首段与 super:: 改写文本不可达，靠评审兜底。
def infra_block_dep_pattern(targets: Sequence[str]) -> re.Pattern[str]:
    return re.compile(
        r"\bcrate\s*::\s*\{?\s*(" + "|".join(targets) + r")\b(?:\s*::|\s+as\b|\s*;)"
    )


@dataclass(frozen=True)
class InfraBlockEdge:
    """crate 内块间认许边条目：基础设施文件相对路径 + 目标块名 + 成因留痕。"""

    file: str
    target: str
    reason: str


# crate 内块间认许边（ADR-0111 决策 4 / #1134）：块间反向依赖的既有设计意图
# 边逐条留痕于本脚本，与 INFRA_DOMAIN_ALLOWED_EDGES 同款留痕纪律——精确到
# 文件相对路径（相对 `crates/infra/src`）+ 目标块名，附 ADR 指针；清单之外的
# 块间反向引用一律红。
INFRA_BLOCK_ALLOWED_EDGES: Final[tuple[InfraBlockEdge, ...]] = (
    InfraBlockEdge(
        file="db/mod.rs",
        target="boot",
        reason=(
            "ADR-0111 决策 2 / #1131：引导层五模块升顶层 boot 后，既有 `crate::db::{boot,…}` "
            "调用点与协议 crate 的 `ledger_infra::db::…` 路径经本再导出保持零改动——"
            "路径兼容面，非机制依赖（#1128 ids 同款口径）"
        ),
    ),
)


@dataclass(frozen=True)
class SymbolCallerEntry:
    """当期入口闭集条目：生产调用方文件（相对 src-app）+ 成因留痕。"""

    file: str
    reason: str


# 当期入口符号（规则名与报文定位共用单一字面量）；`try_` 软形态（issue #1797）
# 同台账同一扫描，扫描正则以 `try_` 可选前缀并入。
CONVERT_CURRENT_CALL_SYMBOL: Final[str] = "convert_to_native_current"


# 当期入口生产调用方闭集（ADR-0011 决策 3 + 2026-09-22 修订 ① / #1692，成员清单
# 的唯一住址）：「写路径全部走交易日历史折算、当期表只服务读路径」的守卫载体——
# 文件级白名单，白名单外的生产调用即红，测试代码按 `/tests/` 路径约定豁免。
# 每条留痕成因（读路径消费面 / 压测工具注记），与
# INFRA_DOMAIN_ALLOWED_EDGES 同款留痕纪律；在册成员零调用或文件缺失同样红
#（清单漂移 fail loud、拒绝空集假绿）。
CONVERT_CURRENT_CALLERS: Final[tuple[SymbolCallerEntry, ...]] = (
    SymbolCallerEntry(
        file="crates/dashboard/src/lib.rs",
        reason="读路径：净资产与持仓市值卡片的当期折算（消费面见 amount convert.rs 当期入口 doc）",
    ),
    SymbolCallerEntry(
        file="crates/investment/src/overview.rs",
        reason="读路径：投资概览市值与累计盈亏的当期折算",
    ),
    SymbolCallerEntry(
        file="crates/investment/src/crud.rs",
        reason=(
            "读路径：持仓行逐行当期折算的软形态（try_convert_to_native_current，issue #1797）"
            "——持仓页签合计卡与首页投资概览卡的逐行本位币列；单行缺汇率降为行级 null，命令不失败"
        ),
    ),
    SymbolCallerEntry(
        file="crates/investment/src/reports.rs",
        reason=(
            "读路径：累计收益折本位币单值（query_cumulative_pnl_native_total，issue #1797）"
            "——三腿逐行当期折算后求和，缺汇率码化上抛不给半截数字"
        ),
    ),
    SymbolCallerEntry(
        file="crates/investment/src/financial_freedom.rs",
        reason="读路径：财务自由度可投资资产与现金合计的当期折算",
    ),
    SymbolCallerEntry(
        file="crates/physical-asset/src/crud.rs",
        reason="读路径：实物资产估值的当期折算",
    ),
    SymbolCallerEntry(
        file="crates/scheduled/src/spend.rs",
        reason="读路径：定时计划花费推算的当期折算",
    ),
    SymbolCallerEntry(
        file="crates/savings-goal/src/pace.rs",
        reason=(
            "读路径：储蓄目标关联计划节奏的当期折算（#1753，订阅花费 spend.rs 同款先例——"
            "计划金额折算本位币再乘周期系数；计划币种即专属账户币种=本位币，正常数据零汇率读取）"
        ),
    ),
    SymbolCallerEntry(
        file="src/cross_book.rs",
        reason=(
            "读路径：跨账本汇总的当期折算（活动本读数合并单点 read_active_book_and_merge，"
            "#1699 自壳层命令闭包抽出；ADR-0114 跨本唯一例外）——#2083 切轨后住 NSH 壳 src/cross_book.rs"
        ),
    ),
    SymbolCallerEntry(
        file="src/bin/ledger-perf/bench.rs",
        reason=(
            "读路径：性能基准的当期折算基线（ledger-perf bench 查询面；#2082 迁居 nsh-app "
            "同包 bin，#2083 切轨后随根包 src 入扫描范围）"
        ),
    ),
)


# 当期入口调用命中扫描（掩码注释与字面量后）：标识符 + 同行左括号才算调用；
# `fn convert_to_native_current(` / `fn try_convert_to_native_current(` 定义面、
# `use` 花括号列举与 doc 链提及均不命中。`try_` 软形态（issue #1797）经可选前缀
# 并入同一扫描——软形态同属当期入口，白名单纪律不分形态。
def convert_current_call_hits(source: str) -> list[ScanHit]:
    masked = mask_non_code(source)
    raw_lines = source.split("\n")
    hits: list[ScanHit] = []
    pattern = re.compile(r"\b(?:try_)?" + CONVERT_CURRENT_CALL_SYMBOL + r"[ \t]*\(")
    for m in pattern.finditer(masked):
        idx = m.start()
        # 定义面不是调用：紧邻前文以 `fn ` 收尾（`pub fn name(` 同形）。
        if re.search(r"(?:^|[^\w])fn\s+$", masked[max(0, idx - 4) : idx]) is not None:
            continue
        line = line_at(masked, idx)
        text = raw_lines[line - 1].strip() if line - 1 < len(raw_lines) else ""
        hits.append(ScanHit(line=line, text=text, match=m.group(0), captured=None))
    return hits


# 规则①形态：全局模型模块路径（全局目录已消亡，任何引用即残留）
GLOBAL_MODEL_PATH_PATTERN: Final[re.Pattern[str]] = re.compile(r"\bcrate\s*::\s*models\b")

# 规则②形态：模型模块的 glob 再导出——域接缝 `pub use model::*` 与
# 跨域/旧目录同名拍平 `pub use …::models::*`；逐类型花括号列举不命中
MODEL_GLOB_REEXPORT_PATTERN: Final[re.Pattern[str]] = re.compile(
    r"\bpub\s+use\s+[\w:]*\bmodels?\b\s*::\s*\*"
)

# 规则②形态：任意 glob 再导出（仅用于域模型文件内的聚合扫描）
MODEL_FILE_GLOB_PATTERN: Final[re.Pattern[str]] = re.compile(r"\bpub\s+use\s+[\w:]*\*")

# 规则③形态：产品代码原生事务语句（issue #1014 / #1003 grilling 定案 7）——
# 事务壳（无条件自持 `hold_transaction` / 嵌套感知 `ensure_transaction`）归
# 基础设施 `db::tx_scope`，其余位置手写 `BEGIN` / `COMMIT` / `ROLLBACK` 即红。
# 靶形态落在字符串字面量里，扫描须 `keep_literals=True`（只掩码注释）。
# #1469：API 形态覆盖 `execute` 与 `execute_batch` 两变体（仓内多处在用）——
# 手写事务边界不得借 API 变体漏检。
NATIVE_TX_STMT_PATTERN: Final[re.Pattern[str]] = re.compile(
    r"\bexecute(?:_batch)?\s*\(\s*\"(?:BEGIN|COMMIT|ROLLBACK)\b"
)

# 原生事务语句唯一合法住址（事务原语本体，issue #1014；#1088 起住基础设施 crate）
NATIVE_TX_STMT_ALLOWED: Final[str] = f"{INFRA_SRC_REL}/db/tx_scope.rs"


# 花括号列举内的违规条目头（深度 0 逐条切分后取首个标识符；#1089 零容忍，
# 全部条目违规）。返回违规条目头，供调用方构造命中。
def disallowed_brace_entries(body: str) -> list[str]:
    out: list[str] = []
    depth = 0
    current = ""

    def flush() -> None:
        nonlocal current
        head = re.split(r"[\s:{]", current.strip())[0]
        if head != "":
            out.append(head)
        current = ""

    for ch in body:
        if ch == "{" or ch == "(" or ch == "[":
            depth += 1
        elif ch == "}" or ch == ")" or ch == "]":
            depth -= 1
        if ch == "," and depth == 0:
            flush()
        else:
            current += ch
    flush()
    return out


# 域模型文件或模型目录成员（ADR-0059 目标形状：每域一个 model.rs，先例名
# models.rs；#1181 起判据扩到目录形态——模型目录 model/ / models/ 下的成员
# 文件同守「域模型禁止 glob 聚合」，模型目录化不再静默失靶）
def is_model_file(rel_path: str) -> bool:
    segments = rel_path.split("/")
    file = segments[-1]
    if file == "model.rs" or file == "models.rs":
        return True
    return any(s == "model" or s == "models" for s in segments[:-1])


# 测试豁免形态（ADR-0056 决策 5）：tests.rs 文件与 tests/ 目录
def is_test_file(rel_path: str) -> bool:
    segments = rel_path.split("/")
    file = segments[-1]
    return file == "tests.rs" or "tests" in segments[:-1]


@dataclass(frozen=True)
class ScanHit:
    """单条扫描命中：行号（1 起算）、原文行、匹配文本、捕获组 1
    （无捕获组时 None，基础设施→域形态为目标域名）。"""

    line: int
    text: str
    match: str
    captured: str | None


# 扫描单个 Rust 文本（掩码注释与字符串/char 字面量）：返回命中指定形态的
# 行号（1 起算）与原文。`keep_literals=True` 保留字符串/char 字面量内容、只掩码
# 注释——靶形态落在字符串里的扫描（原生事务语句，issue #1014）
def scan_rust_source(
    text: str, pattern: re.Pattern[str], keep_literals: bool = False
) -> list[ScanHit]:
    hits: list[ScanHit] = []
    masked = mask_non_code(text, keep_literals)
    masked_lines = masked.split("\n")
    raw_lines = text.split("\n")
    for i, masked_line in enumerate(masked_lines):
        m = pattern.search(masked_line)
        if m is not None:
            captured: str | None = m.group(1) if pattern.groups > 0 else None
            hits.append(
                ScanHit(
                    line=i + 1,
                    text=raw_lines[i].strip(),
                    match=m.group(0),
                    captured=captured,
                )
            )
    return hits


# 结构守门 Rust 面收集：家族共享单点 walk_text_files 的本守门消费侧（issue #1634，
# 与 check-infra-dml / check-eastmoney-residue 同源），文件结构复用 WalkedFile。
# 测试豁免（ADR-0056 决策 5）是本守门政策谓词，walk 后按 rel 过滤——
# is_test_file 只看 rel 形状，与遍历中剪枝输出全等（tests/ 目录成员无论多深
# 都按 rel 排除；唯一分歧形态是名为 tests.rs 的目录，非合法 Rust 模块布局，
# 出现时由模块面投影核对报红，此处多扫不致假绿）。
def collect_rust_files(dir_path: str, rel_base: str) -> list[WalkedFile]:
    return [
        f
        for f in walk_text_files(
            dir_path, rel_base, WalkTextOptions(extensions=RUST_EXTENSIONS)
        )
        if not is_test_file(f.rel)
    ]


# 取 TOML 段内容（段头到下一个段头之间，不含段头行）；段不存在返回 None。
def manifest_section(text: str, section: str) -> str | None:
    lines = text.split("\n")
    header = f"[{section}]"
    start = -1
    for i, line in enumerate(lines):
        trimmed = line.strip()
        if start == -1:
            if trimmed == header:
                start = i + 1
        elif trimmed.startswith("["):
            return "\n".join(lines[start:i])
    if start == -1:
        return None
    return "\n".join(lines[start:])


# 清单 [package] name（缺失返回 None）。
def manifest_package_name(manifest: str) -> str | None:
    section = manifest_section(manifest, "package")
    if section is None:
        return None
    m = re.search(r"(?:^|\n)\s*name\s*=\s*\"([^\"]+)\"", section)
    return m.group(1) if m is not None else None


# 清单声明的依赖 crate 名（`table` = dependencies / dev-dependencies；含
# `target.*` 变体与 `[dependencies.x]` 子表形态）。只取依赖表本身，段落其余内容
# 不参与——供 crate 依赖方向核对使用。
def declared_dependency_names(manifest: str, table: str) -> list[str]:
    names: dict[str, None] = {}
    table_re = re.compile(rf"^(?:target\..+\.)?{table}(?:\.([A-Za-z0-9_-]+))?$")
    current = ""
    for raw in manifest.split("\n"):
        header = re.fullmatch(r"\[([^\]]+)\]", raw.strip())
        if header is not None:
            current = header.group(1)
            sub = table_re.match(current)
            if sub is not None and sub.group(1) is not None:
                names[sub.group(1)] = None
            continue
        sub = table_re.match(current)
        if sub is None or sub.group(1) is not None:
            continue
        key = re.match(r"([A-Za-z0-9_-]+)\s*=", raw.strip())
        if key is not None:
            names[key.group(1)] = None
    return list(names)


# 清单声明的**生产**依赖 crate 名。刻意排除 `[dev-dependencies]`：spec #1086
# 明文裁决「数据库 ↔ 核心交易域的双向引用是测试专用边，用 dev-dependency 环
# 解决（Cargo 允许）」，生产依赖方向仍按「壳 → 域 → 基础设施」单向核对。
def declared_production_dependency_names(manifest: str) -> list[str]:
    return declared_dependency_names(manifest, "dependencies")


# 清单声明的 **dev-dependency** crate 名（#1596 基础设施方向核对用）。
def declared_dev_dependency_names(manifest: str) -> list[str]:
    return declared_dependency_names(manifest, "dev-dependencies")


# [lints] 段声明 workspace 继承（`workspace = true`）——六件套门禁的继承接线。
def inherits_workspace_lints(manifest: str) -> bool:
    section = manifest_section(manifest, "lints")
    return section is not None and re.search(
        r"(?:^|\n)\s*workspace\s*=\s*true\b", section
    ) is not None


# 声明（decl_index 行）前属性链中距声明最近的一条 `#[cfg(...)]` 属性全文——链
# 提取委托唯一原语 attribute_chain_before（#1602），提取规则（注释透明、多行闭合、
# 相邻终止）以其注记为准，此处只消费：自链上取最近 cfg（多 cfg 叠加取最内层），
# 无 cfg 即 None。属性判定在掩码注释后进行（keep_literals=True，保留字符串
# 字面量），属性内注释不参与门匹配。供生产编译 feature 门的各判定共用
#（test_utils / http 投影，ADR-0111 决策 5）。
def first_cfg_text_before(lines: Sequence[str], decl_index: int) -> str | None:
    keep = mask_non_code("\n".join(lines), True)
    # boundary = decl_index 行首字符位：属性链必须整条落在声明行之前（声明行内
    # 前缀的属性不入链，与既有行级扫描「自上一行起找」的边界一致）
    boundary = 0
    for k in range(decl_index):
        boundary += len(lines[k]) + 1
    attrs = attribute_chain_before(keep, boundary)
    for attr in reversed(attrs):
        if attr.startswith("#[cfg("):
            return attr
    return None


# 测试器具生产编译门的「放行测试」判定（ADR-0111 决策 5 / issue #1132）：声明
# （`pub mod test_utils;` 等）前的属性链中须有
# 一条 `#[cfg(...)]`（单行或 rustfmt 拆行的多行属性，#1469），且该 cfg 在
# `test` 或 `test-utils` feature 下放行——无门、
# `#[cfg(not(test))]` 等反向门、与测试无关的 cfg 一律不合格（判为生产会编译）。
# 声明前允许注释与其它属性（如 `#[doc(hidden)]`），属性顺序不敏感。
def has_test_allowing_cfg_gate(lines: Sequence[str], decl_index: int) -> bool:
    cfg = first_cfg_text_before(lines, decl_index)
    return (
        cfg is not None
        and re.search(r"\btest\b", cfg) is not None
        and "not(" not in cfg
    )


# HTTP 投影 impl 的 feature cfg 门判定（ADR-0111 决策 5 / issue #1133）：声明
# （`impl axum::response::IntoResponse for AppError`）前的属性链中须有一条
# `#[cfg(...)]`（单行或多行，#1469）含 `feature = "http"`；无门、`#[cfg(not(feature = "http"))]` 等
# 反向门一律不合格（feature 开启实现反而消失，等价于无门）。同 has_test_allowing_cfg_gate，
# 声明前允许注释与其它属性，属性顺序不敏感。
def has_http_feature_cfg_gate(lines: Sequence[str], decl_index: int) -> bool:
    cfg = first_cfg_text_before(lines, decl_index)
    return (
        cfg is not None
        and re.search(r"feature\s*=\s*\"http\"", cfg) is not None
        and "not(" not in cfg
    )


# 生产依赖表（`[dependencies]` 与 `target.*.dependencies`，inline 或子表形态，
# 刻意排除 `[dev-dependencies]`）中对 `ledger-infra` 启用指定 feature 的原文行；
# 命中的行即「生产构建会把该 feature 编入」的证据（ADR-0111 决策 5：#1132 用于
# test-utils、#1133 用于 http）。
def production_ledger_infra_enablement(manifest: str, feature: str) -> str | None:
    # 词边界匹配：'http' 不得误命中 https:// 等 URL 里的裸 'http' 子串。
    feature_re = re.compile(rf"\b{feature}\b")
    section = ""
    for raw in manifest.split("\n"):
        header = re.fullmatch(r"\[([^\]]+)\]", raw.strip())
        if header is not None:
            section = header.group(1)
            continue
        if re.fullmatch(r"(?:target\..+\.)?dependencies", section):
            if re.match(r"\s*ledger-infra\s*=", raw) and feature_re.search(raw):
                return raw.strip()
        elif re.fullmatch(r"(?:target\..+\.)?dependencies\.ledger-infra", section):
            if re.match(r"\s*features\s*=", raw) and feature_re.search(raw):
                return raw.strip()
    return None


# `[features] default` 是否（直接或经本清单内 feature 转发）触达指定 feature——
# 默认 feature 在生产构建启用，等价于无条件编入该 feature 的内容。转发链上的
# 边按 `includes(feature)` 判定，因而也覆盖 `ledger-infra/test-utils` 这类
# 跨 crate 引用形态（ADR-0111 决策 5：#1132 用于 test-utils、#1133 用于 http）。
# 跨清单的依赖 feature 图不在文本可辨范围，靠评审兜底。
def default_features_include(manifest: str, feature: str) -> bool:
    section = manifest_section(manifest, "features")
    if section is None:
        return False
    feature_edges: dict[str, list[str]] = {}
    for line in section.split("\n"):
        m = re.match(r"\s*([A-Za-z0-9_-]+)\s*=\s*\[([^\]]*)\]", line)
        if m is None:
            continue
        feature_edges[m.group(1)] = [
            x.group(1) for x in re.finditer(r"\"([^\"]+)\"", m.group(2))
        ]
    seen: set[str] = set()
    pending = list(feature_edges.get("default", []))
    while pending:
        current = pending.pop()
        if feature in current:
            return True
        if current in seen:
            continue
        seen.add(current)
        pending.extend(feature_edges.get(current, []))
    return False


# `crates/` 下含 Cargo.toml 的成员 crate 目录（相对 src-app，排序保证输出确定）。
def member_crate_dirs(src_tauri_dir: str) -> list[str]:
    crates_dir = os.path.join(src_tauri_dir, "crates")
    if not os.path.exists(crates_dir):
        return []
    # 列举机制归家族共享单点 read_dir_entries（#1742 收口）：排序归一为 localeCompare
    # （原 plain sort 实树逐目录对拍全等）；成员判定（Cargo.toml 存在）是 workspace
    # 政策，留本门。
    return [
        f"crates/{e.name}"
        for e in read_dir_entries(crates_dir)
        if e.is_directory and os.path.exists(os.path.join(crates_dir, e.name, "Cargo.toml"))
    ]


@dataclass(frozen=True)
class GatedDecl:
    """生产编译 cfg 门核对条目：文件 + 声明形状正则 + 报文文案。"""

    file: str
    pattern: re.Pattern[str]
    label: str
    gate: str
    src: str
    production_artifact: str


# crate 边界核对（spec #1086 / issue #1087 门禁前置）：workspace 成员登记、
# 六件套 deny 门禁继承、依赖方向（壳 → 域 → 基础设施）与静态检查/测试命令
# 的 workspace 覆盖，外加 test_utils 生产编译门（ADR-0111 决策 5 / #1132）与
# HTTP 错误响应投影 feature 门（ADR-0111 决策 5 / #1133），
# 全部 fail loud、删除即变红：
# - 成员漏写 `[lints] workspace = true` → 门禁静默消失，clippy 仍绿，本核对红；
# - `crates/` 下新增 crate 未登记 CRATES → 边界知识分裂，本核对红；
# - cargo 命令缺 `--workspace` → 默认只作用于根包，本核对红；
# - infra `test_utils` 模块摘掉 cfg 门、或根包生产依赖启用 `test-utils` → 测试
#   器具被编入生产构建，本核对红；
# - infra `http` 投影门被摘（axum 裸依赖 / impl 无 cfg / default 含 http / 域侧
#   成员启用 http）→ axum 无条件编入或域侧引入，本核对红。
def check_crate_boundaries(src_tauri_dir: str) -> list[str]:
    problems: list[str] = []
    repo_root = os.path.dirname(src_tauri_dir)
    root_manifest_path = os.path.join(src_tauri_dir, "Cargo.toml")
    if not os.path.exists(root_manifest_path):
        problems.append(f"✗ crate 边界：workspace 根清单不存在：{root_manifest_path}")
        return problems
    root_manifest = Path(root_manifest_path).read_text(encoding="utf-8")

    # ① workspace 骨架 + 成员目录 glob（新增 crate 自动成为 workspace 成员）
    workspace_section = manifest_section(root_manifest, "workspace")
    if workspace_section is None:
        problems.append(
            "✗ crate 边界：src-app/Cargo.toml 缺 [workspace] 段——Rust 根须为 workspace 根（spec #1086）"
        )
    elif f'"{MEMBER_DIR_GLOB}"' not in workspace_section:
        problems.append(
            f"✗ crate 边界：[workspace] members 未包含 \"{MEMBER_DIR_GLOB}\"——成员目录须用 glob 纳入，"
            "新增 crate 自动入 workspace，漏项即静默漏检"
        )

    # ② 六件套门禁的唯一声明处（workspace 级）
    clippy_lints = manifest_section(root_manifest, "workspace.lints.clippy")
    if clippy_lints is None:
        problems.append(
            "✗ 门禁继承：[workspace.lints.clippy] 缺失——六件套 deny 门禁的唯一声明处（ADR-0060 / spec #1086）"
        )
    else:
        for key in PANIC_LINT_KEYS:
            if re.search(rf"(?:^|\n)\s*{key}\s*=\s*\"deny\"", clippy_lints) is None:
                problems.append(
                    f"✗ 门禁继承：[workspace.lints.clippy] 缺 {key} = \"deny\"（ADR-0060 六件套）"
                )

    # ③ 根包同为 workspace 成员，须继承门禁
    if not inherits_workspace_lints(root_manifest):
        problems.append(
            "✗ 门禁继承：workspace 根包缺 [lints] workspace = true——根包同受六件套约束（ADR-0060）"
        )

    # ④ 成员登记：磁盘成员目录 ↔ CRATES 双向核对（新 crate 未登记即红）
    on_disk = member_crate_dirs(src_tauri_dir)
    registered_member_dirs = sorted(c.dir for c in CRATES if c.dir.startswith("crates/"))
    for dir_name in on_disk:
        if not any(c.dir == dir_name for c in CRATES):
            problems.append(
                f"✗ crate 边界：成员 crate 未登记 CRATES：{dir_name}\n"
                "    新增 crate 后须在 scripts/check_structure.py 的 CRATES 追加一行"
                "（name / dir / layer；依赖面注记归 crate lib.rs 的 //!），"
                "否则边界知识分裂成两份、依赖方向失守"
            )
    for dir_name in registered_member_dirs:
        if dir_name not in on_disk:
            problems.append(
                f"✗ crate 边界：CRATES 登记的成员目录不存在：{dir_name}（清单漂移 fail loud）"
            )

    # ⑤ 每个 crate：清单存在、包名一致、门禁继承、依赖方向单向
    for crate in CRATES:
        is_root = crate.dir == "."
        manifest_path = (
            root_manifest_path
            if is_root
            else os.path.join(src_tauri_dir, crate.dir, "Cargo.toml")
        )
        if not os.path.exists(manifest_path):
            problems.append(f"✗ crate 边界：crate 清单不存在：{crate.name}（{crate.dir}）")
            continue
        manifest = root_manifest if is_root else Path(manifest_path).read_text(encoding="utf-8")
        name = manifest_package_name(manifest)
        if name != crate.name:
            shown_name = name if name is not None else "（缺 name）"
            problems.append(
                f"✗ crate 边界：CRATES 登记名 {crate.name} 与清单包名 {shown_name} 不一致（{crate.dir}）"
            )
        if not is_root and not inherits_workspace_lints(manifest):
            problems.append(
                f"✗ 门禁继承：成员 crate {crate.name} 缺 [lints] workspace = true（{crate.dir}/Cargo.toml）\n"
                "    缺失即六件套 deny 门禁静默消失而 clippy 依然全绿——删除继承行即变红（ADR-0060 / spec #1086）"
            )
        # 依赖方向只看生产依赖（dev-dependency 环是 spec #1086 明文裁决的测试专用边，
        # 见 declared_production_dependency_names 注释）。
        for dep in declared_production_dependency_names(manifest):
            target = next((c for c in CRATES if c.name == dep), None)
            if target is not None and CRATE_LAYER_RANK[target.layer] > CRATE_LAYER_RANK[crate.layer]:
                problems.append(
                    f"✗ crate 依赖方向：{crate.name}（{crate.layer}）依赖 {target.name}（{target.layer}）\n"
                    "    分层规则：壳 → 域 → 基础设施单向；被依赖逻辑应下沉到更低层（spec #1086）"
                )
        # 同级域 crate 的同步禁边（ADR-0101 决策 4b / #1107）：多端同步域是全部业务域
        # 的消费方，业务域只可依赖同步协议 crate；业务域声明 `ledger-sync-engine`
        # 生产依赖即形成「业务域 ↔ 同步域」环，与分层秩无关，故在 crate 边界单点
        # 显式拒绝（源码文本扫描已随 #1596 退役，声明面是唯一判定处）。
        if (
            crate.layer == CRATE_LAYER.DOMAIN
            and crate.name != "ledger-sync-engine"
            and "ledger-sync-engine" in declared_production_dependency_names(manifest)
        ):
            problems.append(
                f"✗ crate 依赖方向：业务域 crate {crate.name} 生产依赖多端同步域 ledger-sync-engine\n"
                "    业务域只依赖同步协议 crate `ledger-sync-protocol`，不依赖多端同步域（ADR-0101 决策 4b / #1107）；"
                "重放分派住同步域单向消费各业务域，反边即环"
            )

        # 基础设施→域 dev-dependency（ADR-0071 决策 6 修订注记 / #1596 换载体）：
        # 生产依赖越界由上方层秩核对拦下、未声明依赖即编译失败；cargo 对
        # dev-dependency 环放行（测试目标与生产依赖图分离），是唯一盲区——基础设施
        # 对域层 crate 的 `[dev-dependencies]` 逐条
        # 留痕于 INFRA_DOMAIN_ALLOWED_EDGES，清单之外的声明即红。协议层不在本规则
        # 辖下（退役的原 infra→域文本扫描同样不辖），其生产方向仍归层秩核对。
        if crate.layer == CRATE_LAYER.INFRA:
            for dep in declared_dev_dependency_names(manifest):
                target = next((c for c in CRATES if c.name == dep), None)
                if target is None:
                    continue
                if CRATE_LAYER_RANK[target.layer] < CRATE_LAYER_RANK[CRATE_LAYER.DOMAIN]:
                    continue
                if any(e.crate == dep for e in INFRA_DOMAIN_ALLOWED_EDGES):
                    continue
                problems.append(
                    f"✗ 基础设施→域方向：{crate.name} 声明 dev-dependency {target.name}（{target.layer}）未留痕\n"
                    "    基础设施→域生产边归零（ADR-0071 决策 6 / #538）；dev-dependency 环 cargo 放行、"
                    "是编译期盲区——测试专用边须逐条留痕于本脚本 INFRA_DOMAIN_ALLOWED_EDGES（附 ADR 指针），"
                    "或把逻辑下沉域目录"
                )

    # ⑥ 静态检查与测试命令覆盖全成员（缺 --workspace 即静默漏检成员）
    for rel in WORKSPACE_COMMAND_FILES:
        abs_path = os.path.join(repo_root, rel)
        if not os.path.exists(abs_path):
            problems.append(f"✗ workspace 命令覆盖：宿主文件不存在：{rel}")
            continue
        source = Path(abs_path).read_text(encoding="utf-8")
        # `.ts` 宿主的命令面是数组字面量（另核对）；shell / YAML 宿主先掩掉引号内内容，
        # 否则 `echo "…cargo test…"` 这类说明文字会被当成命令（P1 假绿）。
        # #1742 留门：`.py` 是宿主类型分派政策（数组字面量 vs shell 掩码两条解析路径），
        # 非文件收集，不换扩展名闭集。
        is_py_host = rel.endswith(".py")
        text = source if is_py_host else mask_shell_quoted(source)
        hits = check_py_cargo_arrays(rel, source, problems) if is_py_host else 0
        for i, line in enumerate(text.split("\n")):
            # 注释行不算命令：shell / workflow 与 .py 宿主均用 `#`，注释里的
            # `cargo test` 只是说明文字，不构成命令面。
            trimmed = line.strip()
            if trimmed == "" or trimmed.startswith("#"):
                continue
            # 逐条命令核对（一行可有 `cargo fmt … && cargo clippy …` 多条：只看首个
            # 匹配会把未覆盖的 clippy 放过去）；命令段截到下一个 shell 控制符为止。
            for m in CARGO_COMMAND_PATTERN.finditer(line):
                hits += 1
                rest = line[m.start() :]
                end_match = re.search(r"&&|;|\|", rest)
                segment = rest if end_match is None else rest[: end_match.start()]
                # `--all` 按整词才算 workspace 别名：`--all-targets` / `--all-features`
                # 的 `\b` 落在 `-` 前，用 \b 会假绿（本核对要拦的正是这一形态）。
                if (
                    WORKSPACE_SCOPE_PATTERN.search(segment) is not None
                    or ALL_SCOPE_PATTERN.search(segment) is not None
                ):
                    continue
                problems.append(
                    f"✗ workspace 命令覆盖：{rel}:{i + 1} cargo {m.group(1)} 缺 --workspace"
                    "（非虚拟 workspace 下默认只作用于根包，会静默漏检成员 crate）\n"
                    f"    {line.strip()}"
                )
        # 空集拒绝（与覆盖守门的「拒绝以空集假绿」同口径）：宿主里一条命令位置上的
        # cargo 命令都没有——命令被 echo 成说明文字、被注释掉或整段删除时，逐条核对会
        # 变成空转全绿（#1112 第三轮审查 P1）。
        if hits == 0:
            problems.append(
                f"✗ workspace 命令覆盖：{rel} 未发现任何命令位置上的 cargo 命令"
                "（引号内的说明文字不算命令）——拒绝以空集假绿，命令被 echo/注释/删除即红"
            )

    # ⑦ 测试导出生产编译门（ADR-0111 决策 5）：测试目标专用导出默认不进生产
    # 编译，由构建形态保证，而非注释约定。删除即变红——clippy 走
    # `--all-features`、测试走 dev-dependency，都发现不了门被摘掉：
    #   ① infra `test_utils` 模块声明须带「放行测试」的 cfg 门（无门/反向门即生产编译）；
    #   ② 生产依赖（`[dependencies]` 与 target 变体）不得对 ledger-infra 启用 test-utils；
    #   ③ 根包与 infra 的 `[features] default` 不得包含 test-utils（默认 feature 即生产）；
    #   ④ 投资五节标题锚点常量（issue #1185，住 `handlers/import.rs`）须带同一形态的门；
    #   ⑤ 锚点再导出（`api_server/mod.rs`）须带同一形态的门。
    #   （api_server 随 #2081 迁居 NSH；#2083 切轨后 NSH 升根包，门路径 src/api_server。）
    # （根包 `test_utils` 再导出面已随 #1108 清除——测试器具经 dev-dependency 以
    # `ledger_infra::test_utils` 直达，根包侧门条目随之退役；再引入无门再导出会被
    # 生产构建解析失败拦下。）
    gated_decls: list[GatedDecl] = [
        GatedDecl(
            file=os.path.join(src_tauri_dir, INFRA_SRC_REL, "lib.rs"),
            pattern=re.compile(r"^\s*pub\s+mod\s+test_utils\s*;"),
            label="pub mod test_utils;",
            gate="test_utils 生产编译门",
            src="ADR-0111 决策 5 / issue #1132",
            production_artifact="测试器具",
        ),
        # 投资五节标题锚点（issue #1185）：#1121 常量结构锁与 #1123 API 集成锁的
        # 共享单一住处，仅测试构建编译——门摘掉即测试锚点静默进生产二进制。
        GatedDecl(
            file=os.path.join(src_tauri_dir, "src", "api_server", "handlers", "import.rs"),
            pattern=re.compile(r"^\s*pub\s+const\s+INVESTMENT_SECTION_HEADERS\b"),
            label="pub const INVESTMENT_SECTION_HEADERS",
            gate="投资五节锚点生产编译门",
            src="issue #1185",
            production_artifact="测试锚点",
        ),
        GatedDecl(
            file=os.path.join(src_tauri_dir, "src", "api_server", "mod.rs"),
            pattern=re.compile(
                r"^\s*pub\s+use\s+handlers::import::INVESTMENT_SECTION_HEADERS\s*;"
            ),
            label="pub use handlers::import::INVESTMENT_SECTION_HEADERS;",
            gate="投资五节锚点生产编译门",
            src="issue #1185",
            production_artifact="测试锚点",
        ),
    ]
    for decl in gated_decls:
        rel = decl.file[len(src_tauri_dir) + 1 :]
        if not os.path.exists(decl.file):
            problems.append(f"✗ {decl.gate}：{rel} 不存在，无法核对 cfg 门（{decl.src}）")
            continue
        lines = Path(decl.file).read_text(encoding="utf-8").split("\n")
        decl_index = next((i for i, l in enumerate(lines) if decl.pattern.match(l)), -1)
        if decl_index == -1:
            problems.append(f"✗ {decl.gate}：{rel} 找不到 `{decl.label}` 声明")
        elif not has_test_allowing_cfg_gate(lines, decl_index):
            problems.append(
                f"✗ {decl.gate}：{rel} `{decl.label}` 未加「放行测试」cfg 门\n"
                f"    {lines[decl_index].strip()}\n"
                '    门须为 `#[cfg(any(test, feature = "test-utils"))]`（或等价 cfg，支持 rustfmt 拆行的多行属性）；'
                f"无门 / `#[cfg(not(test))]` / 与测试无关的 cfg 都会让生产编译{decl.production_artifact}"
                f"（{decl.src}），删除或写反 cfg 门即变红"
            )

    prod_enable_line = production_ledger_infra_enablement(root_manifest, "test-utils")
    if prod_enable_line is not None:
        problems.append(
            "✗ test_utils 生产编译门：根包生产依赖 ledger-infra 启用了 test-utils\n"
            f"    {prod_enable_line}\n"
            "    test-utils 只许经测试目标（dev-dependency / cfg(test)）启用；"
            "生产依赖启用即把测试器具编入生产构建（ADR-0111 决策 5 / issue #1132），删除该 feature 即变红"
        )

    # infra 清单读一次：test-utils（⑦）与 http（⑧）两道 default 门共用同一份
    # [清单, 出处] 对与同一套核对（default_features_include）。
    infra_manifest_path = os.path.join(src_tauri_dir, os.path.dirname(INFRA_SRC_REL), "Cargo.toml")
    infra_manifest = (
        Path(infra_manifest_path).read_text(encoding="utf-8")
        if os.path.exists(infra_manifest_path)
        else None
    )
    if infra_manifest is None:
        problems.append(
            f"✗ 生产编译 feature 门：{os.path.dirname(INFRA_SRC_REL)}/Cargo.toml 不存在"
        )
    default_feature_manifests: list[tuple[str, str]] = [(root_manifest, "src-app/Cargo.toml")]
    if infra_manifest is not None:
        default_feature_manifests.append(
            (infra_manifest, f"{os.path.dirname(INFRA_SRC_REL)}/Cargo.toml")
        )
    for manifest, where in default_feature_manifests:
        if default_features_include(manifest, "test-utils"):
            problems.append(
                f"✗ test_utils 生产编译门：{where} [features] default 包含 test-utils\n"
                "    默认 feature 在生产构建启用，等价于把测试器具编入生产构建"
                "（ADR-0111 决策 5 / issue #1132），从 default 移除 test-utils 即变红"
            )

    # ⑧ HTTP 错误响应投影 feature 门（ADR-0111 决策 5 / issue #1133）：`impl
    # IntoResponse for AppError` 因孤儿规则必须住 infra，axum 改 optional、经
    # `http` feature 门控，仅壳侧根包启用——避免每出现一个域 crate 就无条件编入
    # axum 及其传递依赖。五处删除即变红——clippy 走 --all-features、壳侧生产依赖
    # 恒启用 http，都发现不了门被摘掉：
    #   ① infra 的 axum 依赖须声明 optional（裸依赖即门形同虚设）；
    #   ② infra [features] 须有 http 转发 dep:axum（门与依赖面绑死）；
    #   ③ error.rs 的 IntoResponse impl 须带 feature = "http" 的 cfg 门；
    #   ④ 根包与 infra 的 [features] default 不得包含 http（默认 feature 即生产）；
    #   ⑤ 域侧成员 crate 生产依赖不得对 ledger-infra 启用 http（域侧不引 axum）。
    http_gate_label = "http 投影 feature 门"
    if infra_manifest is not None:
        axum_line = next(
            (l for l in infra_manifest.split("\n") if re.match(r"\s*axum\s*=", l)), None
        )
        if axum_line is None:
            problems.append(f"✗ {http_gate_label}：infra Cargo.toml 找不到 axum 依赖声明")
        elif "optional = true" not in axum_line:
            problems.append(
                f"✗ {http_gate_label}：infra Cargo.toml 的 axum 依赖未声明 optional\n"
                f"    {axum_line.strip()}\n"
                "    非 optional 即无条件编入 axum 及其传递依赖（ADR-0111 决策 5 / issue #1133），"
                "加回 optional 即变绿"
            )
        features_section = manifest_section(infra_manifest, "features")
        http_feature_line = (
            next(
                (l for l in features_section.split("\n") if re.match(r"\s*http\s*=", l)),
                None,
            )
            if features_section is not None
            else None
        )
        if http_feature_line is None or "dep:axum" not in http_feature_line:
            problems.append(
                f"✗ {http_gate_label}：infra Cargo.toml [features] 缺 `http = [\"dep:axum\"]`\n"
                "    门与依赖面绑死——feature 不转发 dep:axum 即门形同虚设"
                "（ADR-0111 决策 5 / issue #1133）"
            )
    for manifest, where in default_feature_manifests:
        if default_features_include(manifest, "http"):
            problems.append(
                f"✗ {http_gate_label}：{where} [features] default 包含 http\n"
                "    默认 feature 在生产构建启用，等价于无条件编入 axum"
                "（ADR-0111 决策 5 / issue #1133），从 default 移除 http 即变红"
            )

    # ③' impl cfg 门：error.rs 的 IntoResponse impl 必须带 feature = "http" 的门。
    error_rs_path = os.path.join(src_tauri_dir, INFRA_SRC_REL, "error.rs")
    if not os.path.exists(error_rs_path):
        problems.append(
            f"✗ {http_gate_label}：{INFRA_SRC_REL}/error.rs 不存在，无法核对 impl cfg 门（issue #1133）"
        )
    else:
        lines = Path(error_rs_path).read_text(encoding="utf-8").split("\n")
        error_impl_pattern = re.compile(
            r"^\s*impl\s+axum::response::IntoResponse\s+for\s+AppError\b"
        )
        decl_index = next((i for i, l in enumerate(lines) if error_impl_pattern.match(l)), -1)
        if decl_index == -1:
            problems.append(
                f"✗ {http_gate_label}：error.rs 找不到 `impl axum::response::IntoResponse for AppError`"
            )
        elif not has_http_feature_cfg_gate(lines, decl_index):
            problems.append(
                f"✗ {http_gate_label}：error.rs IntoResponse impl 未加 feature cfg 门\n"
                f"    {lines[decl_index].strip()}\n"
                '    门须为 `#[cfg(feature = "http")]`（紧贴 impl 的属性链）；无门即无条件编译 axum 投影'
                "（ADR-0111 决策 5 / issue #1133），补回 cfg 门即变绿"
            )

    # ⑤' 域侧成员启用 http → 红：http 只许壳侧启用（根包 nsh-app——api_server
    # 随 #2081 迁居后壳是 axum 的唯一合法消费方，ADR-0112 决策 5），域侧启用即把
    # axum 编入域依赖图，违背「域侧依赖不引入 axum」口径（issue #1133）。
    for crate_dir in member_crate_dirs(src_tauri_dir):
        if crate_dir == os.path.dirname(INFRA_SRC_REL):
            continue  # infra 自身是门宿主，非消费方
        member_manifest_text = Path(
            os.path.join(src_tauri_dir, crate_dir, "Cargo.toml")
        ).read_text(encoding="utf-8")
        # 壳层成员（CRATES 登记为 SHELL）不在本门辖域：根包 nsh-app（唯一壳成员）
        # 是 http/axum 的合法消费方（api_server 迁居，#2081）。
        shell_entry = next((c for c in CRATES if c.dir == crate_dir), None)
        if shell_entry is not None and shell_entry.layer == CRATE_LAYER.SHELL:
            continue
        enable_line = production_ledger_infra_enablement(member_manifest_text, "http")
        if enable_line is not None:
            problems.append(
                f"✗ {http_gate_label}：域侧成员 {crate_dir} 生产依赖 ledger-infra 启用了 http\n"
                f"    {enable_line}\n"
                "    http 只许壳侧启用；域侧启用即把 axum 编入域依赖图"
                "（ADR-0111 决策 5 / issue #1133），移除该 feature 即变红"
            )
        # 域侧直接声明 axum 同样越界（域侧依赖不引入 axum，issue #1133）——不只拦
        # ledger-infra/http 转发一条路；[dev-dependencies] 不在核对范围（测试专用边）。
        if "axum" in declared_production_dependency_names(member_manifest_text):
            problems.append(
                f"✗ {http_gate_label}：域侧成员 {crate_dir} 生产依赖直接声明 axum\n"
                "    域侧依赖不引入 axum——axum 只有壳层需要（ADR-0111 决策 5 / issue #1133），"
                "移除该依赖即变红"
            )

    return problems


# 模块面投影核对的规则级指针（T2-2：报文只引一处编号，不插值注记）
MODULE_PROJECTION_RULE: Final[str] = (
    "模块面 expected 由 crate 根 lib.rs 的 mod 声明投影（ADR-0056 决策 4 修订注记 / #1595）"
)


# 磁盘顶层模块键：非测试 `.rs` 文件（去后缀）与扫得到非测试文件的目录名。
def disk_module_keys(src_dir: str) -> list[str]:
    keys: list[str] = []
    # 列举机制归家族共享单点 read_dir_entries（#1742 收口）：一层混合形态（文件/目录各自
    # 判定）正是其 dirent 投影面；模块键政策（lib.rs/is_test_file 豁免、目录递归判空）留本门；
    # 输出经末位 sort，列举序归一不影响结果。.rs 扩展名闭集换库 has_extension + RUST_EXTENSIONS
    # （#1742 收口：原内联 endsWith(".rs") 与点扩展名匹配全等，等价表在 test_gate_primitives.py）。
    for entry in read_dir_entries(src_dir):
        if (
            entry.is_file
            and has_extension(entry.name, RUST_EXTENSIONS)
            and entry.name != "lib.rs"
            and not is_test_file(entry.name)
        ):
            keys.append(re.sub(r"\.rs$", "", entry.name))
        elif entry.is_directory and (
            len(collect_rust_files(os.path.join(src_dir, entry.name), entry.name)) > 0
        ):
            keys.append(entry.name)
    return sorted(keys)


@dataclass(frozen=True)
class ModDeclScan:
    """lib.rs `mod` 声明扫描（#1593）：模块清单投影的权威源，形状判定表见文件头。
    注释与字符串掩码后匹配声明；`#[path]` / `include!` / 内联模块块 / 认不出的
    属性链记入 violations（调用方 fail loud），不静默跳过。
    """

    # 计入 expected 的模块名（去 .rs 后缀的模块键），按声明出现顺序
    modules: list[str]
    # 认不出的形状（行号 + 形状描述），调用方 fail loud
    violations: list[ModDeclViolation]


@dataclass(frozen=True)
class ModDeclViolation:
    line: int
    shape: str


# 属性链允许的形状：cfg 门、lint 属性与 doc 属性不改变模块↔文件映射。
# `#[path]` 改写映射（单列 fail loud），`cfg_attr` 夹带 path 等价。
def is_recognized_mod_attr(attr: str) -> bool:
    if re.match(r"^#\[\s*(?:cfg|cfg_attr|allow|warn|deny|forbid|expect|doc)\b", attr) is None:
        return False
    if (
        re.match(r"^#\[\s*cfg_attr\b", attr) is not None
        and re.search(r"\bpath\b", attr) is not None
    ):
        return False
    return True


# 声明前属性链提取的唯一原语（#1602：合并 #1469 的 cfg 门解析与 #1593 的形状
# 判定两套「取声明前属性链」实现，括号配对与相邻判定只此一份）：在掩码文本
#（mask_non_code keep_literals=True，注释已为空白、字符串字面量保留）中自
# boundary_idx（声明起点字符位）向前收集相邻的 `#[…]` 属性全文，返回按源码
# 顺序排列的列表。跳过空白（含换行——rustfmt 拆行的多行属性同样识别，读到
# 配对方括号闭合为止）；遇到首个非属性代码（非 `]` 起步，或 `[` 前非 `#`）
# 即止——属性与被饰声明必须相邻。括号配对为文本级：属性谓词不含带括号字符串
# 的常态下可靠，残余场景靠评审兜底。消费者：scan_mod_declarations（消费完整链
# 做形状判定）与 first_cfg_text_before（取最近 cfg 门，ADR-0111 决策 5）。
def attribute_chain_before(text: str, boundary_idx: int) -> list[str]:
    attrs: list[str] = []
    i = boundary_idx - 1
    while i >= 0:
        while i >= 0 and re.search(r"\s", text[i]) is not None:
            i -= 1
        if i < 0 or text[i] != "]":
            break
        depth = 0
        open_idx = -1
        for j in range(i, -1, -1):
            if text[j] == "]":
                depth += 1
            elif text[j] == "[":
                depth -= 1
                if depth == 0:
                    open_idx = j
                    break
        if open_idx <= 0 or text[open_idx - 1] != "#":
            break
        attrs.insert(0, text[open_idx - 1 : i + 1])
        i = open_idx - 2
    return attrs


# 去掉字符串开头连续的属性块（`#[…]`，含同行内联），返回其余文本。
def strip_leading_attrs(text: str) -> str:
    s = text
    while re.match(r"\s*#\[", s) is not None:
        open_idx = s.index("[")
        depth = 0
        end = -1
        for i in range(open_idx, len(s)):
            if s[i] == "[":
                depth += 1
            elif s[i] == "]":
                depth -= 1
                if depth == 0:
                    end = i
                    break
        if end == -1:
            break
        s = s[end + 1 :]
    return s


def scan_mod_declarations(source: str) -> ModDeclScan:
    keep = mask_non_code(source, True)
    code = mask_non_code(source, False)
    violations: list[ModDeclViolation] = []
    modules: list[str] = []

    # include! 展开出的模块面对文本扫描不可达：fail loud，不静默跳过。
    for m in re.finditer(r"\binclude\s*!", code):
        violations.append(ModDeclViolation(line=line_at(code, m.start()), shape="include!(…)"))

    decl_re = re.compile(r"\bmod\s+(?:r#)?([A-Za-z_][A-Za-z0-9_]*)\s*([;{])")
    for m in decl_re.finditer(code):
        idx = m.start()
        line = line_at(code, idx)
        name = m.group(1)
        line_start = code.rfind("\n", 0, idx) + 1
        prefix = code[line_start:idx]
        if re.fullmatch(r"\s*(?:pub(?:\s*\([^)]*\))?\s+)?", strip_leading_attrs(prefix)) is None:
            violations.append(
                ModDeclViolation(
                    line=line,
                    shape=f"认不出的声明前缀：{source[line_start : idx + len(m.group(0))].strip()}",
                )
            )
            continue
        if m.group(2) == "{":
            violations.append(ModDeclViolation(line=line, shape=f"内联模块块 mod {name} {{ … }}"))
            continue
        attrs = attribute_chain_before(keep, idx)
        path_attr = next((a for a in attrs if re.match(r"^#\[\s*path\b", a)), None)
        if path_attr is not None:
            violations.append(ModDeclViolation(line=line, shape=f"{path_attr} mod {name};"))
            continue
        unknown_attr = next((a for a in attrs if not is_recognized_mod_attr(a)), None)
        if unknown_attr is not None:
            violations.append(ModDeclViolation(line=line, shape=f"{unknown_attr} mod {name};"))
            continue
        if name == "tests":
            continue  # 测试豁免（ADR-0056 决策 5，与 is_test_file 同规）
        modules.append(name)
    return ModDeclScan(modules=modules, violations=violations)


# 交易域 crate 内区级依赖引用扫描（ADR-0113 决策 7 / #1181）：掩码注释与字面量
# 后匹配 `super::` / `crate::` 前缀 + 目标首段标识符（目录约定 / #1597 后顶层
# 只有区目录，`crate::` 首段即区目录；深层 `super::` 指向同区子模块，不命中
# 区目录表即被调用方跳过）；`::{…}` 花括号列举跨行取匹配闭括号后逐条目切分取
# 首段标识符。捕获 = 目标首段（区归属查表用）。表达式位裸路径（`writer::x`
# 无前缀形态，依赖边已由 use 语句承载）与别名改写文本不可达，靠评审兜底
#（与壳层/基础设施扫描同款边界）。
def scan_transaction_zone_refs(text: str) -> list[ScanHit]:
    masked = mask_non_code(text)
    raw_lines = text.split("\n")
    hits: list[ScanHit] = []

    def push(index: int, match: str, captured: str) -> None:
        line = line_at(masked, index)
        text_line = raw_lines[line - 1].strip() if line - 1 < len(raw_lines) else ""
        hits.append(ScanHit(line=line, text=text_line, match=match, captured=captured))

    for m in re.finditer(r"\b(?:super|crate)\s*::\s*(\{)?", masked):
        start = m.start()
        after = m.end()
        if m.group(1) is None:
            segment = re.match(r"([A-Za-z_][A-Za-z0-9_]*)", masked[after:])
            if segment is not None:
                push(start, m.group(0) + segment.group(1), segment.group(1))
            continue
        # 花括号列举：跨行取匹配闭括号，深度 0 逐条切分后取各条目首段标识符。
        # `{` 已被正则消费进 m[0]（after 在其后），深度从 1 起数——从 0 起数会使
        # 闭括号落到 -1、close 恒为 -1，body 吞到文件尾、后续枚举变体等被误报。
        depth = 1
        close = -1
        for i in range(after, len(masked)):
            if masked[i] == "{":
                depth += 1
            elif masked[i] == "}":
                depth -= 1
                if depth == 0:
                    close = i
                    break
        body = masked[after : close if close != -1 else len(masked)]
        for entry in disallowed_brace_entries(body):
            push(start, f"{{…{entry}…}}", entry)
    return hits


# 交易域 crate 内区级层序核对（ADR-0113 决策 2/3/7 / #1181 / #1597）：区归属按
# 目录约定投影——文件的区 = 其所在 crate 顶层区目录的区（TRANSACTION_ZONE_BY_DIR），
# 同区互依合法；跨区时秩大者方可依赖秩小者（写读 → 接缝 → 共享语义，写读两径
# 互不依赖）；认许边之外即红。fail loud 两类区登记面违例：① crate 根顶层单
# 文件（无区目录可投影，不允许「根级单文件默认为共享语义」——那会给写路径模块
# 留静默错分类的洞）；② 未登记区归属的顶层目录（新区目录漏登区表，判向静默
# 失靶）。crate 根 lib.rs（声明与再导出面）豁免判向。测试豁免形态由
# collect_rust_files 过滤（ADR-0056 决策 5）。
def check_transaction_zone_direction(src_tauri_dir: str) -> list[str]:
    problems: list[str] = []
    target = next((t for t in CRATE_MODULE_TARGETS if t.crate == "ledger-transaction"), None)
    if target is None:
        return [
            "✗ 交易域区级层序：CRATES 政策核缺 ledger-transaction 条目，无法定位模块根"
            "（规则见 ADR-0113 决策 3 / #1181）",
        ]
    src_dir = os.path.join(src_tauri_dir, target.src_rel)
    if not os.path.exists(src_dir):
        return problems  # 模块面投影核对逐条报「找不到声明文件」
    for f in collect_rust_files(src_dir, ""):
        rel = f.rel
        if rel == "lib.rs":
            continue  # crate 根声明与再导出面，免判向（#1181 同规）
        is_root_file = "/" not in rel
        zone_dir = rel if is_root_file else rel.split("/")[0]
        zone = TRANSACTION_ZONE_BY_DIR.get(zone_dir)
        if zone is None:
            if is_root_file:
                problems.append(
                    f"✗ 根级单文件必须进区目录：{rel}（{target.src_rel}）\n"
                    "    区归属的登记面是目录约定（ADR-0113 决策 2 / #1597）：zone = crate 顶层区目录，"
                    "根级单文件无区目录可投影，不允许「默认为共享语义」——那会给写路径模块留静默错分类的洞；"
                    "把文件移入区目录并同步 lib.rs 声明"
                )
            else:
                problems.append(
                    f"✗ 区目录未登记区归属：{zone_dir}/（{rel}）\n"
                    "    zone 靠路径投影（ADR-0113 决策 2 / #1597）：crate 顶层目录须在 "
                    "TRANSACTION_ZONE_BY_DIR 登记区归属，未登记即无法判向、区级层序静默失靶；"
                    "登记区归属，或把模块并入既有区目录"
                )
            continue
        source = Path(f.abs).read_text(encoding="utf-8")
        for hit in scan_transaction_zone_refs(source):
            captured = hit.captured
            # 目标首段非区目录（同区子模块 / 未登记孤儿）不判向：同区子模块随 owner
            # 同区，孤儿文件由模块面投影核对另行报红。
            if captured is None:
                continue
            target_zone = TRANSACTION_ZONE_BY_DIR.get(captured)
            if target_zone is None or target_zone == zone:
                continue
            if TRANSACTION_ZONE_RANK[zone] > TRANSACTION_ZONE_RANK[target_zone]:
                continue
            allowed = any(
                e.dir == zone_dir and e.target == captured
                for e in TRANSACTION_ZONE_ALLOWED_EDGES
            )
            if allowed:
                continue
            problems.append(
                f"✗ 区级反向依赖：{zone}「{zone_dir}/」引用 {target_zone}「{captured}/」 → "
                f"{rel}:{hit.line}（{hit.match}）\n"
                f"    {hit.text}\n"
                "    区级层序唯一：写路径/读路径 → 跨域接缝 → 共享语义（规则见 ADR-0113 决策 3）——"
                "共享语义不得依赖接缝与路径区，接缝不得依赖路径区，写读两径互不依赖；"
                "区归属按目录约定投影（TRANSACTION_ZONE_BY_DIR，ADR-0113 决策 2 / #1597）；"
                "设计意图边须逐条留痕于本脚本 TRANSACTION_ZONE_ALLOWED_EDGES（附 ADR 指针），"
                "或把依赖下沉到层序更低的区"
            )
    return problems


# 当期入口调用方闭集核对（ADR-0011 决策 3 + 2026-09-22 修订 ① / #1692）：扫描
# 全部非测试 Rust 生产面（根 src + 各 crate 模块面），`convert_to_native_current`
# （含 `try_` 软形态）的调用落在 CONVERT_CURRENT_CALLERS 之外即红；反向核对在册成员——文件缺失
#（清单漂移 fail loud）或零调用（拒绝空集假绿：符号改名/调用全删时白名单不静默
# 变成空转全绿）同样红。测试代码按 `/tests/` 路径约定豁免（ADR-0056 决策 5）。
def check_convert_current_callers(src_dir: str, src_tauri_dir: str) -> list[str]:
    problems: list[str] = []
    files: list[WalkedFile] = []
    try:
        files = collect_rust_files(src_dir, "src") + [
            f
            for target in CRATE_MODULE_TARGETS
            for f in collect_rust_files(
                os.path.join(src_tauri_dir, target.src_rel), target.src_rel
            )
        ]
    except OSError:
        # 目录缺失：下方在册成员存在性核对逐条报红（fail loud），不在此抛栈
        pass
    hits_by_file: dict[str, list[ScanHit]] = {}
    for f in files:
        hits = convert_current_call_hits(Path(f.abs).read_text(encoding="utf-8"))
        if len(hits) > 0:
            hits_by_file[f.rel] = hits
    for rel, hits in hits_by_file.items():
        if any(entry.file == rel for entry in CONVERT_CURRENT_CALLERS):
            continue
        for hit in hits:
            problems.append(
                f"✗ 当期入口调用方闭集：{rel}:{hit.line}（{CONVERT_CURRENT_CALL_SYMBOL} 调用）\n"
                f"    {hit.text}\n"
                "    写路径全部走交易日历史折算、当期表只服务读路径（规则见 ADR-0011 决策 3 / 2026-09-22 修订 ①，#1692）——"
                "当期入口生产调用方白名单外即红；读路径消费面须逐条留痕于本脚本 CONVERT_CURRENT_CALLERS（附成因），"
                "写路径改接 convert_to_native_on_trade_date / convert_to_native_on_edit，零腿例外见 NativeConversion::zero_cash_leg"
            )
    for entry in CONVERT_CURRENT_CALLERS:
        if not any(f.rel == entry.file for f in files):
            problems.append(
                f"✗ 当期入口调用方闭集：在册成员文件不存在：{entry.file}（相对 src-app）\n"
                "    清单漂移 fail loud（规则见 ADR-0011 决策 3 / 2026-09-22 修订 ①，#1692）："
                "文件移动 / 删除后须同步本脚本 CONVERT_CURRENT_CALLERS"
            )
            continue
        if entry.file not in hits_by_file:
            problems.append(
                f"✗ 当期入口调用方闭集：在册成员零调用：{entry.file}（相对 src-app）\n"
                f"    文件存在但扫不到 {CONVERT_CURRENT_CALL_SYMBOL} 生产调用——拒绝空集假绿"
                "（规则见 ADR-0011 决策 3 / 2026-09-22 修订 ①，#1692）："
                "调用已移除即从 CONVERT_CURRENT_CALLERS 摘除该成员"
            )
    return problems


# 单个非测试 Rust 文件的分层与方向扫描（crate 模块面）：基础设施条目核 crate 内
# 块间反向依赖（ADR-0111 决策 4 / #1134）。跨 crate 方向不在本扫描面（#1596：
# 声明面 + 编译期；壳层白名单面已随 #2083 切轨退役）。报文只给定位 + 规则指针（T2-2）。
def scan_module_file(file: WalkedFile, layer: str, problems: list[str]) -> None:
    source = Path(file.abs).read_text(encoding="utf-8")
    if layer == LAYER.INFRA:
        # crate 内块间反向依赖（ADR-0111 决策 4 / #1134）：db 不得引用 boot / signals
        #（shell_support 已随 #1108 迁出根包）；认许边逐条留痕，清单之外即红。
        # 块名 = 模块键（路径首段去 `.rs`）——目录模块（db/…）与单文件模块（db.rs）
        # 同键，块降为单文件时不静默失靶。
        block = re.sub(r"\.rs$", "", file.rel.split("/")[0])
        forbidden_targets = INFRA_BLOCK_FORBIDDEN.get(block)
        if forbidden_targets is not None:
            for hit in scan_rust_source(source, infra_block_dep_pattern(forbidden_targets)):
                allowed = any(
                    e.file == file.rel and e.target == hit.captured
                    for e in INFRA_BLOCK_ALLOWED_EDGES
                )
                if allowed:
                    continue
                problems.append(
                    f"✗ crate 内反向依赖：{block} 引用 {hit.captured} → {file.rel}:{hit.line}（{hit.match}）\n"
                    f"    {hit.text}\n"
                    "    crate 内分层（规则见 ADR-0111 决策 4）：原语 ← db ← boot 单向，db 不得引用 boot/signals——"
                    "设计意图边须逐条留痕于本脚本 INFRA_BLOCK_ALLOWED_EDGES（附 ADR 指针），或把逻辑下沉到更低的块"
                )


# 条目化扫描（ADR-0056 决策 4）：crate 派生模块面共用——条目必须存在且扫得到
# 非测试 Rust 文件（漂移 fail loud），逐文件交 scan_module_file。返回扫到的非测试
# 文件数；返回 0 由调用方统一拒绝（拒绝以空集假绿通过）。
def scan_module_entries(
    entries: Sequence[ModuleEntry],
    base_dir: str,
    problems: list[str],
    context: str,
) -> int:
    scanned = 0
    for w in entries:
        abs_path = os.path.join(base_dir, w.path)
        try:
            st = os.stat(abs_path)
        except OSError:
            st = None
        if st is None:
            problems.append(
                f"✗ 条目路径不存在：{w.path}（{context}）——"
                "目录改名/迁移后未同步政策项或声明面（规则见 ADR-0056 决策 4 修订注记 / #1595）"
            )
            continue
        files: list[WalkedFile] = (
            collect_rust_files(abs_path, w.path)
            if stat_module.S_ISDIR(st.st_mode)
            else [WalkedFile(abs=abs_path, rel=w.path)]
        )
        if len(files) == 0:
            problems.append(
                f"✗ 条目扫不到非测试 Rust 文件：{w.path}（{context}）——"
                "全部是测试豁免形态或已空（规则见 ADR-0056 决策 5）"
            )
            continue
        scanned += len(files)
        for f in files:
            scan_module_file(f, w.layer, problems)
    return scanned


# crate 模块面扫描（#1595）：条目由 `derive_crate_modules` 的声明投影派生，
# 逐条交 scan_module_entries——模块级分层扫描与声明↔磁盘双向全等共用同一 target。
def scan_crate_modules(
    target: CrateModuleTarget,
    derived: Sequence[str],
    src_tauri_dir: str,
    problems: list[str],
) -> int:
    entries = [ModuleEntry(path=path, layer=target.layer) for path in derived]
    return scan_module_entries(
        entries,
        os.path.join(src_tauri_dir, target.src_rel),
        problems,
        # crate 模块面不做源码方向扫描（#1596：跨 crate 方向归声明面 + 编译期）；
        # 条目存在性与非测试文件核对、基础设施块间禁边照旧。
        context=f"{target.crate} / {target.layer}",
    )


def main() -> None:
    repo_root = str(Path(__file__).resolve().parent.parent)
    src_dir = sys.argv[1] if len(sys.argv) > 1 else os.path.join(repo_root, "src-app", "src")
    src_tauri_dir = sys.argv[2] if len(sys.argv) > 2 else os.path.join(repo_root, "src-app")
    problems: list[str] = []
    scanned_files = 0

    # 模型域化禁令（规则①/②）+ 原生事务语句禁令（规则③）：全树扫描（壳、域、
    # 基础设施、顶层文件），残留引用可出现在任何层；collect_rust_files 自带测试
    # 豁免（ADR-0056 决策 5）——外挂测试目录的直置事务边界合法。
    # srcDir 整体不可达时静默交由白名单循环报「路径不存在」，不在此抛栈。
    all_files: list[WalkedFile] = []
    try:
        all_files = collect_rust_files(src_dir, "") + [
            f
            for target in CRATE_MODULE_TARGETS
            for f in collect_rust_files(
                os.path.join(src_tauri_dir, target.src_rel), target.src_rel
            )
        ]
    except OSError:
        # 目录缺失：白名单循环会逐条报错并 fail loud
        pass
    for f in all_files:
        source = Path(f.abs).read_text(encoding="utf-8")
        for hit in scan_rust_source(source, GLOBAL_MODEL_PATH_PATTERN):
            problems.append(
                f"✗ 全局模型路径残留：{f.rel}:{hit.line}（{hit.match}）\n"
                f"    {hit.text}\n"
                "    全局模型目录已随 ADR-0059 模型域化消亡（T7 / #424），"
                "模型类型一律走域路径显式 import（如 crate::transaction::model::Transaction）"
                "——防扁平命名空间复活"
            )
        for hit in scan_rust_source(source, MODEL_GLOB_REEXPORT_PATTERN):
            problems.append(
                f"✗ 域模型 glob 再导出：{f.rel}:{hit.line}（{hit.match}）\n"
                f"    {hit.text}\n"
                "    域 model 只许逐类型再导出，所有权必须逐类型可见 "
                "（ADR-0059 决策 3/6，#424）：改为 pub use model::{TypeA, TypeB} 形态"
            )
        if is_model_file(f.rel):
            for hit in scan_rust_source(source, MODEL_FILE_GLOB_PATTERN):
                problems.append(
                    f"✗ 域模型文件内 glob 聚合：{f.rel}:{hit.line}（{hit.match}）\n"
                    f"    {hit.text}\n"
                    "    域模型文件只承载本域类型定义与逐类型再导出，禁止 glob 聚合 "
                    "（ADR-0059 决策 3/6，#424）"
                )
        for hit in scan_rust_source(source, NATIVE_TX_STMT_PATTERN, True):
            if f.rel == NATIVE_TX_STMT_ALLOWED:
                continue
            problems.append(
                f"✗ 原生事务语句：{f.rel}:{hit.line}（{hit.match}）\n"
                f"    {hit.text}\n"
                "    事务壳归基础设施 db::tx_scope（无条件自持 hold_transaction / "
                "嵌套感知 ensure_transaction，ADR-0056 / ADR-0105；#1013/#1014）——"
                f"产品代码不得手写 BEGIN/COMMIT/ROLLBACK，唯一合法住址 {NATIVE_TX_STMT_ALLOWED}"
            )

    # 模块面扫描（#1595）：crate 模块面 expected 由各 crate `lib.rs` 的
    # `mod` 声明投影（含声明↔磁盘双向全等），逐条做条目核对与基础设施块间禁边，
    # 源码方向扫描随 #1596 退役给声明面 + 编译期（壳层白名单面随 #2083 切轨退役）。
    for target in CRATE_MODULE_TARGETS:
        derived = derive_crate_modules(target, src_tauri_dir, problems)
        scanned_files += scan_crate_modules(target, derived, src_tauri_dir, problems)

    if scanned_files == 0:
        problems.append(
            "✗ crate 模块面扫不到任何非测试 Rust 文件——src-app 目录指错或 CRATES 整体漂移，拒绝以空集假绿通过"
        )

    # crate 边界核对（spec #1086 / issue #1087）：成员登记、门禁继承、依赖方向、
    # 静态检查/测试命令的 workspace 覆盖——与模块路径白名单并列，同为删除即变红。
    problems.extend(check_crate_boundaries(src_tauri_dir))

    # 交易域区级层序（ADR-0113 决策 2/3/7 / #1181 / #1597）：区归属据
    # TRANSACTION_ZONE_BY_DIR 按目录约定投影判向（根级单文件与未登记区目录 fail
    # loud），认许边（决策 3 原形状反边）之外即红。
    problems.extend(check_transaction_zone_direction(src_tauri_dir))

    # 当期入口调用方闭集（ADR-0011 决策 3 + 2026-09-22 修订 ① / #1692）：白名单
    # 外调用即红，测试代码按 /tests/ 路径豁免；在册成员缺失/零调用同样红。
    problems.extend(check_convert_current_callers(src_dir, src_tauri_dir))

    if len(problems) > 0:
        for p in problems:
            print(p, file=sys.stderr)
        print(
            "❌ 结构守门失败：" f"{len(problems)} 处问题"
            "（分层规则：壳 → 域 → 基础设施，域永不依赖壳，白名单即规格，见 ADR-0056）",
            file=sys.stderr,
        )
        sys.exit(1)
    print(
        f"✓ 结构守门：壳 = 根包 nsh-app（dir \".\"，#2083 切轨后唯一壳）"
        f"· crate 模块面非测试文件 {scanned_files} 个 · 对壳层零依赖"
        f"· crate 模块面 {len(CRATE_MODULE_TARGETS)} 份（expected 由 crate 根 lib.rs 的 mod 声明投影 + 磁盘双向全等，#1595）"
        "· 跨 crate 依赖方向：声明面判定（层秩 + 业务域→多端同步域禁令），未声明依赖归编译期（#1596 / ADR-0071·ADR-0101 修订注记）"
        f"· 基础设施 dev-dependency 方向零未留痕声明（认许边 {len(INFRA_DOMAIN_ALLOWED_EDGES)} 条，ADR-0071 决策 6）"
        f"· 模型域化禁令全树扫描 {len(all_files)} 个文件零残留（ADR-0059）"
        f"· 原生事务语句全树扫描 {len(all_files)} 个文件仅 {NATIVE_TX_STMT_ALLOWED} 一处（#1014）"
        f"· crate 边界 {len(CRATES)} 个（成员登记 / 门禁继承 / 依赖方向 / workspace 命令覆盖，#1087）"
        f"· crate 内块间反向依赖零未认许引用（认许边 {len(INFRA_BLOCK_ALLOWED_EDGES)} 条，ADR-0111 决策 4 / #1134）"
        "· 交易域区级层序零未认许反向引用（写读 → 接缝 → 共享语义，"
        f"认许边 {len(TRANSACTION_ZONE_ALLOWED_EDGES)} 条，ADR-0113 决策 3 / #1181）"
        f"· 当期入口调用方闭集 {len(CONVERT_CURRENT_CALLERS)} 成员全在册（白名单外即红，ADR-0011 / #1692）"
        "· test_utils 生产编译门（cfg 门 + 生产依赖不启用 test-utils，#1132）"
        "· 投资五节锚点生产编译门（cfg 门，#1185）"
        "· http 投影 feature 门（axum optional + impl cfg 门 + default 不含 http + 域侧不启用，#1133）"
    )


# 派生单个 crate 的模块面并双向核对（#1595）：返回磁盘实际形态的扫描条目；
# `#[path]` / `include!` / 内联模块块 / 认不出的属性链在此 fail loud。
def derive_crate_modules(
    target: CrateModuleTarget,
    src_tauri_dir: str,
    problems: list[str],
) -> list[str]:
    src_dir = os.path.join(src_tauri_dir, target.src_rel)
    lib_rs_rel = f"{target.src_rel}/lib.rs"
    if not os.path.exists(src_dir) or not os.path.exists(os.path.join(src_tauri_dir, lib_rs_rel)):
        problems.append(
            f"✗ 找不到 crate 根声明文件：{lib_rs_rel}（{target.crate}）\n"
            f"    {MODULE_PROJECTION_RULE}：声明文件缺失即无法派生 expected，fail loud"
        )
        return []
    scan = scan_mod_declarations(
        Path(os.path.join(src_tauri_dir, lib_rs_rel)).read_text(encoding="utf-8")
    )
    for v in scan.violations:
        problems.append(
            f"✗ 认不出的 lib.rs 声明形状：{lib_rs_rel}:{v.line}（{v.shape}）\n"
            "    mod 扫描形状判定表（ADR-0056 决策 4 修订注记 / ADR-0113 决策 7 / ADR-0111 决策 5）："
            "lint 属性（allow/warn/deny/forbid/expect）、doc 属性与 #[cfg] / #[cfg_attr] 门控 mod 计入 expected；"
            "#[cfg(test)] mod tests; 豁免；#[path]（含 cfg_attr 夹带 path）/ include! / 内联模块块 / "
            "上述之外认不出的属性链 fail loud——按上表改写声明，或把新形态登记进形状判定表"
        )
    keys = scan.modules
    entries: list[str] = []
    for key in dict.fromkeys(keys):
        file = f"{key}.rs"
        has_file = os.path.exists(os.path.join(src_dir, file))
        # 目录形态只在含非测试 Rust 文件时才是模块面的一员（只挂 tests.rs 的目录是
        # 外挂测试豁免形态，ADR-0056 决策 5）。
        has_dir_code = os.path.exists(os.path.join(src_dir, key)) and (
            len(collect_rust_files(os.path.join(src_dir, key), key)) > 0
        )
        if has_file:
            entries.append(file)
        if has_dir_code:
            entries.append(key)
        if not has_file and not has_dir_code:
            problems.append(
                f"✗ 声明缺失：{lib_rs_rel} 声明 `mod {key};`，"
                f"磁盘上既无 {file} 也无含非测试文件的 {key}/（{target.src_rel}）\n"
                f"    模块面与声明双向核对（{MODULE_PROJECTION_RULE}）：声明即 rustc 契约，"
                "补齐模块文件/目录或删除声明"
            )
    for key in disk_module_keys(src_dir):
        if key not in keys:
            problems.append(
                f"✗ 未声明模块：{key}（{target.src_rel}）\n"
                f"    {MODULE_PROJECTION_RULE}："
                f"新增模块须先在 {lib_rs_rel} 声明 `mod {key};`——磁盘上多出的生产模块即红（孤儿文件/目录）"
            )
    return entries


# 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的扫描函数。
if __name__ == "__main__":
    main()
