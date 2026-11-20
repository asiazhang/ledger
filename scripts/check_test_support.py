#!/usr/bin/env -S uv run --quiet
# Rust 测试守门（issue #752 落地 / #758 收口 / #956 追加规则 4 / ADR-0084 决策 8），
# 四条规则：
#
# 统一测试数据库工厂（src-app/src/test_support/，#751 落地）是建库与种子知识的
# 唯一入口；本守门防回潮——测试代码绕开工厂直连建库、直写种子表、自抄默认时刻，
# 一律红。白名单起步（「白名单即规格」，ADR-0056 哲学）、随按域迁移票逐组清零
# （API 集成 #753、叶子域+db+sync #754、investment #755、scheduled_transactions
# #756、transaction #757、backup/内联簿记 #758），#758 收口移除白名单机制转
# 纯禁令：扫描范围内命中即红，无基线可维护。前端同构先例：
# check-test-stubs.ts（#725/#726）已验证「深模块收敛 + 文本级守门 + 白名单防回潮」
# 在本仓有效。
#
# 规则 1（禁直连建库）：`open_in_memory` / `init_db` 出现在 test_support 与
# src/db 产品代码之外的 Rust 测试代码即红（建库两行序的唯一入口是
# test_support::open()）。src/db/tests 同样在禁用范围——产品代码合法、测试不豁免；
# src/db 产品代码内的命中不在扫描范围（产品开库合法）。匹配按裸标识符：任意
# 限定路径（db::/crate::db::/nsh_app::db:: 等）、裸调用与 use 引入、以及
# 测试侧平行建库入口（如 `DbState::open_in_memory`——内部即两行序）与方法形态
# 全部命中；`init_db`/`open_in_memory` 作为更长标识符的子串不匹配（\b 边界）。
# 一律非零退出并逐条列出（fail loud，不静默跳过）。BDD e2e 层已随 #2083 切轨
# 退役，原「规则 1 不辖 tests/e2e」的范围边界与例外表随之消失（纯禁令化）。
#
# 规则 2（禁夹具裸 SQL）：工厂种子表的 `INSERT INTO` 出现在 test_support 与
# 各域薄皮之外即红。禁用表集合单一来源 = test_support/seed.rs 的 `INSERT INTO`
# 登记处（提取其表名；工厂未来新种子自动扩展禁令，与 check-test-stubs.ts 从
# REFERENCE_DEFAULTS 提取命令清单同款，无双源漂移）；登记处提不出任何表名即红。
# 薄皮豁免按文件名形状：父目录恰为 tests 目录段且文件名为 common.rs /
# batch_common.rs——域薄皮按准入规则长期保留单域特有种子（ADR-0084 决策 1）。
# 顶屋 tests/ 下的子目录共享层（tests/api_server/common.rs 等）不豁免——文件名
# common.rs 不构成薄皮（薄皮以父目录恰为 tests 判定），命中照常计入。
# 业务表（transactions/categories/budgets 等）的 INSERT 不在本守门范围——那是
# 公开写入口纪律（测试层显式例外逐个登记）的辖域，不是建库/种子工厂的。
#
# 规则 3（禁默认时刻字面量）：固定时刻登记处（FIXED_NOW 住址，ADR-0084 决策 5）
# 提取的现值出现在 test_support 之外的测试代码即红——夹具簿记戳由工厂种子内部发
# 放，调用点零字面量。时刻值单一来源 = 登记处现值提取（先例：同脚本规则 2 的禁
# 用种子表清单自 seed.rs 提取，#1468 归一双份事实源）：登记处改值禁令自动跟随，
# 脚本内不留硬编码时刻快照；登记处缺失或提不出任何现值即 fail loud，禁令不静默
# 失效。域时刻字面量合法（时间推进是测试的行为输入，ADR-0084 决策 5），不在扫描
# 范围；文本级无法分辨意图，恰为现值的字面量一律计入（如实际作域时刻用，改写为
# 其他值或引用常量）；禁令覆盖全部测试代码，无目录例外。
#
# 规则 4（禁自建通道线格式，issue #956）：测试代码不得手工构造通道段/清单
#（`SegmentEntry {` / `ChannelManifest {` 字面构造），也不得在**引用了通道面**
# 的文件里自建摘要实现（`Sha256` / `sha2::`）。通道线格式的成帧单点是
# `test_support::channel::publish_raw_segment`（内部消费产品侧 `channel::sha256_hex`
# 唯一实现）——测试侧自建会让段命名规则、清单版本与「尺寸/摘要算在封包后字节
# 上」的口径各自漂移一份（#956 成因：两处夹具都错把尺寸/摘要算在 payload 上，
# 明文模式下恰好相等而掩盖）。
# 「引用了通道面」的判定：同文件出现 `ChannelManifest` / `SegmentEntry` /
# `EnvelopeMode` / `publish_raw_segment` 任一标识。此作用域限定是为了不误伤
# 与通道无关的摘要用法（如 `src-app/src/bin/ledger-perf/tests.rs` 的生成器确定性 DB
# 摘要——它自带 sha2 但是另一件事）。
#
# 扫描边界（文本级，注释掩码后匹配，注释里的形态不计数）：
# - 「测试代码」= ① 路径含 tests 目录段或文件名为 tests.rs 的文件（域外挂测试
#   模块/目录与顶层集成测试，check_structure.py 同款约定）整文件；
#   ② 其余产品文件内的 `#[cfg(test)] mod <name> { … }` 内联块（括号配对，词法
#   跳过字符串与注释）。产品代码本体不扫——产品开库、产品写表、产品默认时刻
#   均合法，四条规则只辖测试代码。
# - 字符串字面量不掩码（SQL 就住在字符串里）；注释（行/块，含嵌套块注释）掩码
#   为等长空白。裸字符串 "…" 处理转义，r"…" / r#"…"# 等原始字符串按 hash 数配对
#   终止；'…' 仅按合法 char 字面量吞掉，其余（生命周期 'a、标签 'outer:）跳过
#   单字符。词法掩码实现 = 守门家族共享原语库 mask_non_code(…, True)（#1680 收口：
#   本门的第三份实现已删除，与 check-structure 同源同规）。
#
# 已知文本不可达逃逸形态（靠评审兜底）：经 use 别名（use … as x; 调 x()）或局部
# 函数包装的间接建库/写表；宏拼接出的命中形态；`db :: f`（空格分隔限定符）；
# 属性内部含不平衡括号的非常规写法。char 字面量形态枚举之外的怪异转义可能致
# 词法错位——失衡区段跳过不计（宁漏不误）。
#
# 纯禁令语义（#758 收口）：扫描范围内任一命中即红，逐文件逐规则计数输出；无
# 白名单、无基线——需要旁路的形态先收敛进工厂/薄皮，或按显式例外纪律登记动机
# （先例：壳层写仪式例外白名单，ADR-0073）后再恢复扫描覆盖。
#
# TypeScript/Bun 运行时已随 #2174 移植为 Python/uv（issue #734 / ADR-0083 修订注记）：
# 类型经 scripts/check_scripts_types.py（mypy --strict）门槛检查；调用方式
# `uv run scripts/check_test_support.py`。
# 默认校验本仓库；测试可传位置参数指向夹具：uv run scripts/check_test_support.py [src-app-dir]
# workspace 成员（crates/*/src 与 crates/*/tests）同在该扫描范围内（spec #1086 /
# issue #1087）——拆 crate 后测试守门不得因目录随迁而静默漏扫。
# 包装测试 scripts/test_check_test_support.py（#1158 归位）。

from __future__ import annotations

import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Final, NoReturn

from gate_primitives import (
    RUST_EXTENSIONS,
    WalkTextOptions,
    line_at,
    mask_non_code,
    read_dir_entries,
    walk_text_files,
)

# 规则 3 的固定时刻登记处住址（相对根包 src-app 根，单一来源）：main 从该住址提取
# 固定时刻常量现值作禁令清单（先例：同脚本规则 2 禁用种子表清单自 seed.rs 登记处
# 提取），登记处改值禁令自动跟随；配套测试夹具自此导出常量派生登记处文件位置，
# 无双源漂移（#1468）。
FIXED_NOW_REGISTRY_REL: Final[str] = "src/test_support/mod.rs"


def fail(message: str) -> NoReturn:
    print(f"✗ Rust 测试守门：{message}", file=sys.stderr)
    sys.exit(1)


# ——— Rust 词法助手（掩码与 cfg(test) 块提取共享） ———

CHAR_LITERAL_RE: Final[re.Pattern[str]] = re.compile(
    r"^'(\\(?:.|x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]{1,6}\})|[^\\'])'"
)


def string_end_at(source: str, i: int) -> int:
    """若 source[i] 起是字符串字面量起点（含 b/r/br 前缀与原始字符串 r#"…"#），返回终点下标；否则 -1"""
    n = len(source)
    is_raw = source[i] == "r" and i + 1 < n and source[i + 1] in ('"', "#")
    is_byte_raw = (
        source[i] == "b"
        and i + 1 < n
        and source[i + 1] == "r"
        and i + 2 < n
        and source[i + 2] in ('"', "#")
    )
    if source[i] != '"' and source[i] != "b" and not is_raw and not is_byte_raw:
        return -1
    j = i
    raw_hashes = 0
    if source[j] == "b":
        j += 1
    if j < n and source[j] == "r":
        j += 1
        while j < n and source[j] == "#":
            raw_hashes += 1
            j += 1
    if j >= n or source[j] != '"':
        return -1  # 形态不符（如裸 r 后接标识符）
    j += 1
    terminator = '"' + "#" * raw_hashes
    while j < n:
        if raw_hashes == 0 and source[j] == "\\":
            j += 2
            continue
        if source.startswith(terminator, j):
            return j + len(terminator)
        j += 1
    return n  # 未闭合：吞到文件尾（宁漏不误）


def char_end_at(source: str, i: int) -> int:
    """若 source[i] 起是 char 字面量，返回终点下标；否则 -1（生命周期 'a、标签 'outer: 等情形，
    调用方自行跳过单字符）。"""
    m = CHAR_LITERAL_RE.match(source[i : i + 12])
    return i + len(m.group(0)) if m else -1


# ——— 词法掩码（注释→等长空白、字符串/char 原样保留）————
# 第三份实现已删除（#1680 收口）：调用点直调守门家族共享原语库
# mask_non_code(source, True)；全仓 .rs 文件两实现输出逐字节全等
# （2026-09-24 对拍，524 文件零差异）。string_end_at / char_end_at 仍被 cfg(test)
# 块提取的括号配对消费，留本门。

# ——— cfg(test) 内联块提取：属性后随 `mod <name> {`，括号配对取块范围 ———


# cfg(test) 内联块范围（原文偏移区间）
@dataclass(frozen=True)
class Region:
    start: int
    end: int


MOD_DECL_RE: Final[re.Pattern[str]] = re.compile(r"mod\s+\w+\s*\{")


def extract_cfg_test_regions(masked: str) -> list[Region]:
    regions: list[Region] = []
    i = masked.find("#[cfg(test)]")
    while i != -1:
        i += len("#[cfg(test)]")
        # 跳过后随的其余属性（#[allow(...)] 等）与空白
        while True:
            while i < len(masked) and masked[i].isspace():
                i += 1
            if i < len(masked) and masked[i] == "#":
                close = masked.find("]", i)
                if close == -1:
                    break
                i = close + 1
            else:
                break
        mod = MOD_DECL_RE.match(masked[i : i + 64])
        if mod is None:
            # 外挂 `mod x;` 或非 mod 项（use/fn）：不属于本扫描的内联块
            i = masked.find("#[cfg(test)]", i)
            continue
        body_start = i + mod.end()
        depth = 1
        j = body_start
        while j < len(masked) and depth > 0:
            str_end = string_end_at(masked, j)
            if str_end != -1:
                j = str_end
                continue
            if masked[j] == "'":
                j = max(char_end_at(masked, j), j + 1)
                continue
            c = masked[j]
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
            j += 1
        if depth != 0:
            # 配对失衡：跳过该块，宁漏不误
            i = masked.find("#[cfg(test)]", i)
            continue
        regions.append(Region(start=i, end=j))
        i = masked.find("#[cfg(test)]", i)
    return regions


# ——— 四条规则的命中形态 ———
# 规则 1：裸标识符匹配（任意限定路径、裸调用与 use 引入、测试侧平行建库入口
# 如 DbState::open_in_memory 与方法形态全部命中；\b 边界排除更长标识符的子串）。
RULE1_IDENTIFIERS: Final[re.Pattern[str]] = re.compile(r"\b(open_in_memory|init_db)\b")
# 规则 3：固定时刻登记处 pub &str 常量声明（现值提取用；禁令值不在脚本内快照）。
RULE3_DECLARATION: Final[re.Pattern[str]] = re.compile(r'pub\s+const\s+\w+\s*:\s*&str\s*=\s*"([^"]+)"')
# 规则 4：手工构造通道段/清单（字面构造形态），与自建摘要实现（裸标识符）。
# 段/清单构造按 `Type {`（结构体字面量）匹配：类型名单独出现（如 `use ...SegmentEntry`）
# 不命中——仅当文件里真的在拼字面量时才红。
RULE4_LITERAL_CONSTRUCTIONS: Final[tuple[re.Pattern[str], ...]] = (
    re.compile(r"\bSegmentEntry\s*\{"),
    re.compile(r"\bChannelManifest\s*\{"),
)
RULE4_DIGEST_IDENTIFIERS: Final[re.Pattern[str]] = re.compile(r"\b(Sha256|sha2)\b")
# 规则 4 的作用域门：同文件出现下列任一标识才启用摘要禁令（避开与通道无关的
# 摘要用法，见文件头规则 4 说明）。
RULE4_CHANNEL_SURFACE: Final[tuple[re.Pattern[str], ...]] = (
    re.compile(r"\bChannelManifest\b"),
    re.compile(r"\bSegmentEntry\b"),
    re.compile(r"\bEnvelopeMode\b"),
    re.compile(r"\bpublish_raw_segment\b"),
)


# 单条规则命中：规则编号 + 行号
@dataclass(frozen=True)
class Hit:
    rule: int
    line: int


# 带文件定位的命中明细（scan_files 输出面）
@dataclass(frozen=True)
class FileHit:
    rel: str
    rule: int
    line: int


# 四条规则的编号集（判定循环与计数表的单一来源）
RULES: Final[tuple[int, ...]] = (1, 2, 3, 4)


def find_hits(
    masked: str,
    line_of: Callable[[int], int],
    banned_tables: list[str],
    fixed_now_values: list[str],
) -> list[Hit]:
    hits: list[Hit] = []
    for m in RULE1_IDENTIFIERS.finditer(masked):
        hits.append(Hit(rule=1, line=line_of(m.start())))
    if len(banned_tables) > 0:
        re2 = re.compile(f"INSERT\\s+INTO\\s+(?:{'|'.join(banned_tables)})\\b", re.IGNORECASE)
        for m2 in re2.finditer(masked):
            hits.append(Hit(rule=2, line=line_of(m2.start())))
    for value in fixed_now_values:
        at = masked.find(value)
        while at != -1:
            hits.append(Hit(rule=3, line=line_of(at)))
            at = masked.find(value, at + len(value))
    for rule4_re in RULE4_LITERAL_CONSTRUCTIONS:
        for m4 in rule4_re.finditer(masked):
            hits.append(Hit(rule=4, line=line_of(m4.start())))
    # 摘要禁令仅在文件引用了通道面时启用（作用域限定，见文件头规则 4 说明）。
    if any(r.search(masked) for r in RULE4_CHANNEL_SURFACE):
        for m5 in RULE4_DIGEST_IDENTIFIERS.finditer(masked):
            hits.append(Hit(rule=4, line=line_of(m5.start())))
    return hits


# ——— 文件收集：src/** 与 tests/** 下全部 .rs ———
# 遍历机制归守门家族共享单点 walk_text_files（#1680 收口）：.rs 扩展名闭集由库
# RUST_EXTENSIONS 提供；本守门不豁免目录（tests/ 目录成员照扫、靠 is_test_path
# 谓词分流豁免），遍历中剪枝与 walk 后过滤对 .rs 面输出全等。
def walk_rust_files(dir_path: str) -> list[str]:
    return [
        f.abs for f in walk_text_files(dir_path, "", WalkTextOptions(extensions=RUST_EXTENSIONS))
    ]


def member_crate_rust_roots(src_app: str) -> list[str]:
    """workspace 成员 crate 的 Rust 根（crates/<crate>/src 与 crates/<crate>/tests）：
    拆 crate 后测试守门须覆盖全 workspace（spec #1086 / issue #1087），不因目录
    随迁而把成员 crate 的测试代码留在扫描范围之外。"""
    crates_dir = os.path.join(src_app, "crates")
    if not os.path.exists(crates_dir):
        return []
    roots: list[str] = []
    # 成员目录列举归家族共享单点 read_dir_entries（#1742 收口）；原显式 localeCompare
    # 排序与原语排序全等，输出序零变化。
    for entry in read_dir_entries(crates_dir):
        if not entry.is_directory:
            continue
        for sub in ("src", "tests"):
            dir_path = os.path.join(crates_dir, entry.name, sub)
            if os.path.exists(dir_path):
                roots.append(dir_path)
    return roots


def is_test_path(rel_segments: list[str]) -> bool:
    """路径含 tests 目录段，或文件名为 tests.rs（外挂测试模块/目录约定，check_structure.py 同款）"""
    return "tests" in rel_segments[:-1] or rel_segments[-1] == "tests.rs"


def is_thin_shell(rel_segments: list[str]) -> bool:
    """薄皮：父目录恰为 tests 目录段且文件名为 common.rs / batch_common.rs（域薄皮）；
    顶屋 tests/ 子目录下的共享层（tests/api_server/common.rs 等）不豁免，照常计入。"""
    base = rel_segments[-1]
    return (
        len(rel_segments) >= 2
        and rel_segments[-2] == "tests"
        and (base == "common.rs" or base == "batch_common.rs")
    )


def extract_seed_tables(seed_rs_path: str) -> list[str]:
    """工厂种子表清单单一来源：seed.rs 的 INSERT INTO 登记处（表名提取）"""
    if not os.path.exists(seed_rs_path):
        fail(f"工厂种子登记处缺失：{seed_rs_path}（统一测试数据库工厂，issue #751 / ADR-0084）")
    source = Path(seed_rs_path).read_text(encoding="utf-8")
    tables = list(
        dict.fromkeys(
            m.group(1) for m in re.finditer(r"INSERT\s+INTO\s+(\w+)", source, re.IGNORECASE)
        )
    )
    if len(tables) == 0:
        fail(f"种子登记处 {seed_rs_path} 提不出任何 INSERT INTO 表名（清单漂移？）")
    return tables


def extract_fixed_now_values(registry_path: str) -> list[str]:
    """规则 3 禁令清单单一来源：固定时刻登记处的 pub &str 常量现值提取（#1468）——
    登记处文件即固定时刻登记面（FIXED_NOW 唯一住址，ADR-0084 决策 5），其 pub 字符
    串常量闭集就是禁令值；脚本内不留硬编码时刻快照，登记处改值禁令自动跟随。
    在注释掩码后的文本上提取（doc 注释里的声明形状不成禁令值）；登记处缺失或提
    不出任何现值即 fail loud，禁令不静默失效（先例：上方禁用种子表清单提取）。"""
    if not os.path.exists(registry_path):
        fail(f"固定时刻登记处缺失：{registry_path}（FIXED_NOW 唯一住址，ADR-0084 决策 5）")
    masked = mask_non_code(Path(registry_path).read_text(encoding="utf-8"), True)
    values = list(dict.fromkeys(m.group(1) for m in RULE3_DECLARATION.finditer(masked)))
    if len(values) == 0:
        fail(f"固定时刻登记处 {registry_path} 提不出任何 pub const &str 现值（登记处漂移？）")
    return values


# scan_files 的返回：(文件×规则) 命中数与全部命中明细（供纯禁令判定与输出）
@dataclass(frozen=True)
class ScanFilesResult:
    count_by_file: dict[str, dict[int, int]]
    hits: list[FileHit]


def scan_files(
    files: list[str],
    src_app: str,
    banned_tables: list[str],
    fixed_now_values: list[str],
) -> ScanFilesResult:
    """逐文件扫描，返回 (文件×规则) 命中数与全部命中明细（供纯禁令判定与输出）"""
    count_by_file: dict[str, dict[int, int]] = {}
    hits: list[FileHit] = []
    for file in sorted(files):
        rel = os.path.relpath(file, src_app).replace("\\", "/")
        segments = rel.split("/")
        # 工厂本体 = 根包 test_support（唯一登记处；#2083 切轨后 NSH 升根包，
        # 双壳并存期的 crates/nsh 副本面随宿主退役收拢单点）。
        under_test_support = segments[:2] == ["src", "test_support"]
        source = Path(file).read_text(encoding="utf-8")
        masked = mask_non_code(source, True)

        # 各规则的扫描区域：测试路径整文件，产品路径仅内联 cfg(test) 块
        regions: list[Region] = (
            [Region(start=0, end=len(masked))]
            if is_test_path(segments)
            else extract_cfg_test_regions(masked)
        )
        if len(regions) == 0:
            continue

        def line_of(offset: int) -> int:
            return line_at(masked, offset)

        for region in regions:
            text = masked[region.start : region.end]

            def region_line_of(offset: int) -> int:
                # 行号按原文计算：masked 与原文等长等换行，region 起点即原文偏移
                return line_of(region.start + offset)

            for hit in find_hits(text, region_line_of, banned_tables, fixed_now_values):
                # 规则豁免：test_support 是工厂本体，四条规则全部合法
                if under_test_support:
                    continue
                if hit.rule == 2 and is_thin_shell(segments):
                    continue  # 薄皮种子合法（准入规则，ADR-0084 决策 1）
                if rel not in count_by_file:
                    count_by_file[rel] = {}
                by_rule = count_by_file[rel]
                by_rule[hit.rule] = by_rule.get(hit.rule, 0) + 1
                hits.append(FileHit(rel=rel, rule=hit.rule, line=hit.line))
    return ScanFilesResult(count_by_file=count_by_file, hits=hits)


def violations(count_by_file: dict[str, dict[int, int]], hits: list[FileHit]) -> list[str]:
    """纯禁令判定（#758 收口，白名单机制已移除）：扫描树任一命中即违规，返回违规
    清单。原 tests/e2e 例外表随 BDD e2e 于 #2083 退役（目录已不存在，机制随之移除）。"""
    problems: list[str] = []
    RULE_NAMES: Final[dict[int, str]] = {
        1: "直连建库",
        2: "夹具裸SQL",
        3: "默认时刻字面量",
        4: "自建通道线格式",
    }
    for file in sorted(count_by_file):
        for rule in RULES:
            actual = count_by_file[file].get(rule, 0)
            if actual == 0:
                continue
            lines = sorted(h.line for h in hits if h.rel == file and h.rule == rule)
            problems.append(
                f"  {file}:{':'.join(str(line) for line in lines)}\n"
                f"      规则 {rule}（{RULE_NAMES[rule]}）命中 {actual} 处——纯禁令（白名单已移除），"
                f"收敛到 test_support 工厂/种子或域薄皮（ADR-0084）"
            )
    return problems


def main() -> None:
    # 默认住址在 main 内求值而非模块加载期：包装测试需 import 本模块的导出常量
    # （FIXED_NOW_REGISTRY_REL，#1468 夹具派生接缝），默认路径求值不影响 import 面。
    src_app = (
        os.path.abspath(sys.argv[1])
        if len(sys.argv) > 1 and sys.argv[1]
        else str(Path(__file__).resolve().parents[1] / "src-app")
    )
    src_dir = os.path.join(src_app, "src")
    tests_dir = os.path.join(src_app, "tests")
    if not os.path.exists(src_dir):
        fail(f"src-app 目录不存在：{src_app}")

    banned_tables = extract_seed_tables(os.path.join(src_dir, "test_support", "seed.rs"))
    fixed_now_values = extract_fixed_now_values(os.path.join(src_app, FIXED_NOW_REGISTRY_REL))

    files = [
        *walk_rust_files(src_dir),
        *(walk_rust_files(tests_dir) if os.path.exists(tests_dir) else []),
        *(
            f
            for dir_path in member_crate_rust_roots(src_app)
            for f in walk_rust_files(dir_path)
        ),
    ]
    result = scan_files(files, src_app, banned_tables, fixed_now_values)
    problems = violations(result.count_by_file, result.hits)
    if problems:
        print(
            f"✗ Rust 测试守门：发现 {len(problems)} 处违规（纯禁令，无白名单；"
            f"禁用种子表：{' '.join(banned_tables)}；禁用时刻值：{' '.join(fixed_now_values)}）\n"
            + "\n".join(problems)
            + "\n建库/种子/断言唯一入口：src-app/src/test_support/（spec #728 / issue #751，ADR-0084）",
            file=sys.stderr,
        )
        sys.exit(1)

    print(
        f"✅ Rust 测试守门通过（纯禁令：白名单机制已随 #758 收口移除；"
        f"禁用种子表 {len(banned_tables)} 张：{' '.join(banned_tables)}；"
        f"禁用时刻值 {len(fixed_now_values)} 个：{' '.join(fixed_now_values)}）"
    )


# 仅直接运行时执行 main；包装测试以子进程调用（行为等价判据：只测外部可观察结果）。
if __name__ == "__main__":
    main()
