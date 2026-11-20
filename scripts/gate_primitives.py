#!/usr/bin/env -S uv run --quiet
# 守门家族共享原语库（issue #1680）：库归库、门归门——对「受守代码长什么样」不持
# 立场的纯机制住本模块；持立场的（测试豁免谓词、扫描边界扩展名表、豁免目录/文件
# 清单）是政策，留各门。此前原语住在最大守门脚本 check_structure.py 内
# （#1625/#1634/#1637 三次收口把遍历与行号「收」进了以结构守门命名的巨型脚本），
# 兄弟脚本 import 的是一个门——本模块把库与门分离，改遍历语义不必再在两千行守门
# 逻辑里定位它，新增扫描类守门直接 import，不再抄第五份遍历。
#
# 闭集七件（#1680 五件 + #1742 增补平铺面两件）：
# - walk_text_files / WalkedFile / WalkTextOptions —— 递归目录遍历（扩展名闭集过滤、
#   排序保证输出确定、rel 以 rel_base 为前缀归一）；扫描哪些扩展名、豁免哪些目录
#   与文件属各守门政策，经 options 注入。
# - line_at —— 命中下标 → 行号（1 起算），逐命中定位 文件:行 的唯一实现。
# - mask_non_code —— Rust 词法掩码（注释与字符串/char 字面量掩为等长空白、保留换行
#   列位；keep_literals=True 只掩注释），私有助手 _raw_string_open_quote_at。
# - RUST_EXTENSIONS —— Rust 面扩展名闭集（全等副本收口单点，消费方直接 import）。
# #1742 增补（平铺 readdir 与内联 endsWith 的收口/留门判定票，逐处留痕见票）：
# - read_dir_entries / DirEntry —— 只读一层目录列举（排序 + dirent 类型投影，不
#   跟随目录符号链接，与 walk_text_files 同语义；与递归遍历形状不同构，单层语义
#   单独单点）。
# - has_extension —— 点扩展名闭集匹配（walk_text_files 内部同款）。
#
# 消费方（守门家族全量，#1680 T2 收口后；check-commands、check-background-services
# 随 #2083 壳退役移出，check-frontend-structure、check-async-guards、
# check-test-stubs、check-dialog-forms、check-style-blocks 随 #2171/#2172 前端
# 退役移出）：check-structure、check-infra-dml、check-eastmoney-residue、test-exec、
# check-test-support、check-i18n-keys、check-bare-font-size，与原语级测试
# test_gate_primitives.py。掩码与遍历规则改动只动本文件，消费者随引用自动跟随。
#
# 边界（#1466/#1433 划界声明）：面向 Rust 源码的词法掩码，本模块 mask_non_code 是
# TS/Python 侧载体，与 Rust 侧唯一实现 `src-app/src/test_support/scan.rs` 的
# `mask_non_code` 是同一条词法规则的两个运行时载体（双源登记，规则改动必须两侧
# 同步）；防漂移断言消费共享语料夹具 `scripts/fixtures/rust-mask-corpus.rs`
# （test_gate_primitives.py 与 Rust 测试双侧消费，任一侧单独改规则即红）。
#
# Python 化 + uv 运行时（issue #2174 / ADR-0083 修订注记）：类型经 mypy 门槛检查
# （scripts/check_scripts_types.py）；调用方式 `uv run scripts/…py`。本模块是库
# 不是门：不被 check.sh 直接调用、不进守门挂载登记（#1682 的登记闭集 = 可执行门
# 脚本）。

from __future__ import annotations

import os
import re
from dataclasses import dataclass
from typing import Final

# localeCompare 等价（issue #2174 移植注记）：JS localeCompare 按 ICU 排序规则
# 主级大小写不敏感；Python 无内置 ICU，取排序键——主级 casefold 比较、次级码点
# 定序（元组键与「casefold 相等时按原串码点」的两段比较器全等）。本仓扫描树文件
# 名均为 ASCII（字母数字与连字符/下划线/点），该键与 localeCompare 对 ASCII 名称
# 的排序语义一致（test_gate_primitives.py 的排序用例同款钉住）；非 ASCII 名称两
# 语义可能分歧，出现时按双源对拍处理。


def locale_sort_key(name: str) -> tuple[str, str]:
    """localeCompare 等价排序键：主级大小写不敏感（casefold），次级码点定序。"""
    return (name.casefold(), name)


# 遍历收集的文本面文件：绝对路径（读文件用）+ 相对路径（报文定位用，`/` 分隔）


@dataclass(frozen=True)
class WalkedFile:
    abs: str
    rel: str


# 文本面遍历政策（各守门自己的豁免面，经参数注入，不在共享单点内）


@dataclass(frozen=True)
class WalkTextOptions:
    """收集的扩展名闭集（含点，如 ".rs"）；按文件名最后一个点之后的后缀匹配。"""

    extensions: frozenset[str]
    # 目录名剪枝（整目录豁免，如 node_modules / target / docs）
    skip_dirs: frozenset[str] | None = None
    # 相对路径剪枝（整文件豁免，如 CHANGELOG.md）
    skip_files: frozenset[str] | None = None


def walk_text_files(dir_path: str, rel_base: str, options: WalkTextOptions) -> list[WalkedFile]:
    """递归收集目录下的文本面文件（守门家族共享单点，issue #1625）：扩展名闭集过滤、
    目录名排序保证输出确定、rel 以 rel_base 为前缀 `/` 分隔归一。目录判定按
    scandir dirent（不跟随目录符号链接；此语义随 #1680 收口成为家族统一行为——
    原自持遍历中 statSync 跟随符号链接的形态一并归一，本仓扫描树无目录符号链接，
    实害为零、留痕在案）。扫描哪些扩展名、豁免哪些目录与文件属各守门政策，经
    options 注入。
    """
    out: list[WalkedFile] = []

    def walk(current: str, rel: str) -> None:
        with os.scandir(current) as it:
            entries = sorted(it, key=lambda e: locale_sort_key(e.name))
        for entry in entries:
            entry_abs = os.path.join(current, entry.name)
            entry_rel = f"{rel}/{entry.name}" if rel else entry.name
            if entry.is_dir(follow_symlinks=False):
                if options.skip_dirs is None or entry.name not in options.skip_dirs:
                    walk(entry_abs, entry_rel)
                continue
            if not has_extension(entry.name, options.extensions):
                continue
            if options.skip_files is not None and entry_rel in options.skip_files:
                continue
            out.append(WalkedFile(abs=entry_abs, rel=entry_rel))

    walk(dir_path, rel_base)
    return out


# 单层目录列举条目：dirent 类型投影（一层列举的读取面，政策过滤留消费侧）


@dataclass(frozen=True)
class DirEntry:
    name: str
    is_directory: bool
    is_file: bool


def read_dir_entries(dir_path: str) -> list[DirEntry]:
    """只读一层的目录列举（守门家族共享单点，issue #1742）：平铺 readdir 与递归遍历
    形状不同构，单层语义在此统一——locale_compare 排序保证输出确定（与
    walk_text_files 同款家族统一行为；直接消费列举序的站点原 plain sort /
    readdir 原序已实树逐目录对拍全等（crates/、migrations/、locales/*——
    packages/ 对拍面已随 Web 前端退役，#2171/#2172），列举序被下游再排序吸收
    的站点（check-structure disk_module_keys）对拍不参与、结果与列举序无关，
    #1742 留痕）、dirent 类型判定不跟随目录符号链接（同 walk_text_files）。
    列举哪些、怎么过滤属各守门政策，消费侧自持；目录不存在按 scandir 原样抛错
    （调用侧 exists 守卫各自保留，不静默空集）。
    """
    with os.scandir(dir_path) as it:
        entries = sorted(it, key=lambda e: locale_sort_key(e.name))
    return [
        DirEntry(
            name=entry.name,
            is_directory=entry.is_dir(follow_symlinks=False),
            is_file=entry.is_file(follow_symlinks=False),
        )
        for entry in entries
    ]


def line_at(text: str, index: int | None) -> int:
    """命中行定位：匹配下标 → 行号（1 起算，数命中点之前的换行，None 按文首计）。
    守门家族共享单点（issue #1625）：逐命中定位 文件:行 的唯一实现，
    check-infra-dml 的 scan_dml 与 check-eastmoney-residue 的 scan_residue 消费。
    """
    return text.count("\n", 0, index if index is not None else 0) + 1


# 若 i 起是 Rust 原始字符串前缀，返回其后开引号下标；否则 None。
# 覆盖 r"…" / r#"…" 与字节变体 br"…" / br#"…"，# 数任意；前一字符为
# 标识符成分时是普通名字（如 for），不误伤。
_IDENT_PRECEDING = re.compile(r"[A-Za-z0-9_]")


def _raw_string_open_quote_at(text: str, i: int) -> int | None:
    prev = text[i - 1] if i > 0 else ""
    if _IDENT_PRECEDING.match(prev):
        return None
    n = len(text)
    j = i
    if j < n and text[j] == "b" and j + 1 < n and text[j + 1] == "r":
        j += 2
    elif j < n and text[j] == "r":
        j += 1
    else:
        return None
    while j < n and text[j] == "#":
        j += 1
    return j if j < n and text[j] == '"' else None


def mask_non_code(text: str, keep_literals: bool = False) -> str:
    """掩码 Rust 源文本中的注释与字符串/char 字面量：内容替换为等长空白
    （保留换行与列位，行号不变），使依赖扫描只落在真实代码上。
    处理形态：行注释（//、///、//!）、块注释（/* .. */，可嵌套）、
    普通字符串（含转义）、原始字符串 r"…" / r#"…" / r##"…" 及其字节变体
    br"…" / br#"…" / br##"…"（# 数任意；'\\u{…}' 转义不按字面量识别——与
    Rust 侧一致，见下）、
    char 字面量（'a'、'\\n'、'\\\\'、'\\''）；生命周期标注（'a）按非字面量处理。
    `keep_literals=True` 时保留字符串/char 字面量内容、只掩码注释——用于靶形态
    落在字符串里的扫描（原生事务语句 `execute("BEGIN")`，issue #1014；码化错误
    构造点第一参数与 `const NAME: &str` 值、命令注解腿整文掩码，两份弱形态注释
    处理器随 #1741 收口并入）。

    **双源登记**（issue #1433）：本函数与 Rust 侧唯一实现
    `src-app/src/test_support/scan.rs` 的 `mask_non_code` 是同一条词法掩码规则
    的两个运行时载体，规则改动必须两侧同步；防漂移断言消费共享语料夹具
    `scripts/fixtures/rust-mask-corpus.rs`（test_gate_primitives.py 与 Rust 测试
    双侧消费，任一侧单独改规则即红）。TS/Python 侧消费面：check-structure、
    check-infra-dml、check-background-services、test-exec、check-test-support
    （#1680 起第三份实现收口至此）、check-commands（命令注解腿整文掩码）、
    check-i18n-keys（码化构造点提取腿，二者均为 #1741 弱形态副本收口），消费名单
    由 test_gate_primitives.py 锁定；keepLiterals 形态无 Rust 侧
    对应物（scan.rs 头注登记在案的不对称），由 keepLiterals 语料期望单侧锁定。
    """
    out = list(text)
    n = len(text)

    def blank(from_: int, to: int) -> None:
        for k in range(from_, min(to, n)):
            if out[k] != "\n":
                out[k] = " "

    i = 0
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            # 行注释（含 /// 与 //!）到行尾
            end = text.find("\n", i)
            stop = n if end == -1 else end
            blank(i, stop)
            i = stop
        elif c == "/" and i + 1 < n and text[i + 1] == "*":
            # 块注释，Rust 可嵌套
            depth = 1
            j = i + 2
            while j < n and depth > 0:
                if text[j] == "/" and j + 1 < n and text[j + 1] == "*":
                    depth += 1
                    j += 2
                elif text[j] == "*" and j + 1 < n and text[j + 1] == "/":
                    depth -= 1
                    j += 2
                else:
                    j += 1
            blank(i, j)
            i = j
        elif c == '"':
            # 普通字符串：跳过转义对
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                elif text[j] == '"':
                    j += 1
                    break
                else:
                    j += 1
            if not keep_literals:
                blank(i, j)
            i = j
        elif c == "r" or c == "b":
            # 原始字符串 r"…" / r#"…" / r##"…" 与字节变体 br"…" / br#"…" / br##"…"
            open_quote = _raw_string_open_quote_at(text, i)
            if open_quote is None:
                i += 1
                continue
            prefix_end = i + 2 if c == "b" else i + 1
            hashes = open_quote - prefix_end
            close = '"' + "#" * hashes
            end = text.find(close, open_quote + 1)
            stop = n if end == -1 else end + len(close)
            if not keep_literals:
                blank(i, stop)
            i = stop
        elif c == "'":
            # char 字面量 vs 生命周期：有闭引号为字面量，否则是生命周期标注（'a）
            j = i + 1
            if j < n and text[j] == "\\":
                j += 1
                if j < n and text[j] == "{":
                    e = text.find("}", j)
                    j = n if e == -1 else e + 1
                else:
                    j += 1
            else:
                j += 1
            if j < n and text[j] == "'":
                stop = j + 1
                if not keep_literals:
                    blank(i, stop)
                i = stop
            else:
                i += 1
        else:
            i += 1
    return "".join(out)


def has_extension(name: str, extensions: frozenset[str] | set[str]) -> bool:
    """点扩展名闭集匹配：取名字最后一个点之后的后缀对集合判定（与 walk_text_files
    的遍历内匹配同款，issue #1742）。对形如 ".rs" 的点扩展名与 endsWith 全等（裸
    扩展名 ".rs"、多点 "a.b.rs"、"a.rs.txt" 等边缘形态由 test_gate_primitives.py
    全等表钉住）——残余内联 endsWith(".rs") 收口至此 + RUST_EXTENSIONS。
    """
    dot = name.rfind(".")
    return dot != -1 and name[dot:] in extensions


# Rust 面扩展名闭集（walk_text_files 消费参数；全等副本收口单点，issue #1680）
RUST_EXTENSIONS: Final[frozenset[str]] = frozenset({".rs"})
