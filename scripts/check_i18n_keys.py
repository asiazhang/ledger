#!/usr/bin/env -S uv run --quiet
# i18n 文案 key 全等校验（issue #342 / ADR-0049）：源语言 zh-CN 与其余各 locale
# 的 key 集合双向全等——任一方向孤儿（源语言独有 / 其他 locale 独有）即非零退出，
# 漏翻在合入前被拦截。仿命令集双向全等校验先例（scripts/check-commands.ts）：
# 纯函数导出供单测（scripts/test_check_i18n_keys.py），CLI 入口可独立运行。
# TypeScript/Bun 运行时已随 #2174 移植为 Python/uv（issue #734 / ADR-0083 修订注记）：
# 类型经 scripts/check_scripts_types.py（mypy --strict）门槛检查；调用方式
# `uv run scripts/check_i18n_keys.py`。
# 默认校验本仓库（locales 文案资源住仓库根 locales/——原住 packages/i18n 随 Web
# 参照前端退役迁址，#2171/#2172，ADR-0050 门保留）；测试可传位置
# 参数指向夹具目录：uv run scripts/check_i18n_keys.py [locales-dir] [rust-src-dir]
# 真实仓绿基线用例：scripts/test_check_i18n_keys.py（issue #1473）。
#
# 码化错误模板覆盖守门（issue #1188 / ADR-0050）：在 key 全等之外，再枚举 Rust
# 生产代码中全部码化错误构造点（AppError::coded / codedp / coded_not_found /
# codedp_not_found 的第一参数），要求每个码在 zh-CN 与 en-US 的 errors.json 中
# 都有模板——「新增错误条件 = 后端码化构造器 + errors.json 两份补翻译，漏翻在
# CI 拦截」由此从评审约定变为机器守门。边界（已知且可接受）：
# - 只静态枚举字符串字面量与 `const NAME: &str` 一级常量引用；宏参数
#   （closed_set! 的 err_code）与 format! 拼接形态的动态码无法静态解析，
#   计入 unresolved 计数仅供人审（漏报方向安全：不会误伤存量已覆盖的码）。
# - #[cfg(test)] 块与测试路径（tests 目录 / *tests.rs）不扫：测试内构造不直达用户。
# - 序列化层统一携带的系统通用码（db.error / parse.error / io.error，ADR-0050
#   决策 2「底层驱动消息不翻译也不进码表」）无构造点，天然不入枚举，无需白名单。

from __future__ import annotations

import json
import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Final

from gate_primitives import RUST_EXTENSIONS, WalkTextOptions, mask_non_code, read_dir_entries, walk_text_files

# 源语言目录名（其余 locale 一律与它比对）
SOURCE_LOCALE_DIR: Final[str] = "zh-CN"


def _object_entries(obj: object) -> list[tuple[str, object]]:
    """JS Object.entries 等价：dict 取键值对，list/str 取下标枚举，其余为空。"""
    if isinstance(obj, dict):
        return [(str(k), v) for k, v in obj.items()]
    if isinstance(obj, (list, str)):
        return [(str(i), v) for i, v in enumerate(obj)]
    return []


def flatten_keys(obj: object, prefix: str = "") -> list[str]:
    """递归展开 JSON 对象为点分 key 全集（叶子 key 才计入；数组整体视为叶子）。"""
    keys: list[str] = []
    for k, v in _object_entries(obj):
        path = f"{prefix}.{k}" if prefix else k
        if isinstance(v, dict):
            keys.extend(flatten_keys(v, path))
        else:
            keys.append(path)
    return sorted(keys)


# 双向集合差的逐 locale 失败项
@dataclass(frozen=True)
class LocaleFailure:
    locale: str
    source_only: list[str]
    other_only: list[str]


def diff_key_sets(source_keys: list[str], other_keys: list[str]) -> tuple[list[str], list[str]]:
    """双向集合差：sourceOnly = 源语言独有（其他 locale 漏翻），otherOnly = 其他 locale 独有。"""
    other = set(other_keys)
    source = set(source_keys)
    return (
        [k for k in source_keys if k not in other],
        [k for k in other_keys if k not in source],
    )


def collect_locale_keys(dir_path: str) -> list[str]:
    """读取单个 locale 目录：聚合同目录全部 *.json 的点分 key（文件名作为顶层域前缀）"""
    keys: list[str] = []
    # 列举机制归家族共享单点 read_dir_entries（#1742 收口）：排序归一为 locale_compare
    # （原 plain sort 实树对拍全等）；.json 是 i18n 键文件扩展名政策（单消费、无副本
    # 可漂移），留门不换扩展名闭集（#1742 留门）。
    for entry in read_dir_entries(dir_path):
        if not entry.name.endswith(".json"):
            continue
        domain = entry.name[: -len(".json")]
        parsed: object = json.loads(
            (Path(dir_path) / entry.name).read_text(encoding="utf-8")
        )
        keys.extend(flatten_keys(parsed, domain))
    return sorted(keys)


# ---------------------------------------------------------------------------
# 码化错误模板覆盖（issue #1188 / ADR-0050）：Rust 码化构造点 → errors.json 模板
# ---------------------------------------------------------------------------

# 码化错误构造调用形态（第一参数恒为错误码）
CODED_CALL_RE: Final[re.Pattern[str]] = re.compile(r"\b(codedp_not_found|coded_not_found|codedp|coded)\s*\(")
# `const NAME: &str = "..."` 一级常量表（跨文件聚合，供常量引用型调用点解析）
RUST_STR_CONST_RE: Final[re.Pattern[str]] = re.compile(r'\bconst\s+([A-Z_][A-Z0-9_]*)\s*:\s*&str\s*=\s*"([^"]*)"')

# 弱形态注释预处理器 strip_rust_comments 已收口（issue #1741）：与共享原语
# mask_non_code 在真实仓 275 个非测试 .rs 逐文件对拍（码集 + unresolved 计数 +
# 常量表聚合）零差异，管线改消费 gate_primitives 的 mask_non_code(src, True)。
# keepLiterals 形态只掩注释、保留字符串/char 字面量（码化构造点第一参数与
# const 值都是字符串字面量，须原样回吐），且等长保位、识别原始串/嵌套块
# 注释/char 字面量——弱形态不识别这些形态的分歧从此由单一权威裁决；消费
# 名单由 test_gate_primitives.py 锁定。

CFG_TEST_ATTR_RE: Final[re.Pattern[str]] = re.compile(r"#\[\s*cfg\s*\(\s*test\s*\)\s*\]")


def strip_cfg_test_blocks(src: str) -> str:
    """移除 #[cfg(test)] 修饰的 mod 块（跟踪大括号配对，块可重复出现）。"""
    out = src
    while True:
        m = CFG_TEST_ATTR_RE.search(out)
        if m is None:
            break
        brace = out.find("{", m.end())
        if brace == -1:
            out = out[: m.start()]
            break
        depth = 0
        i = brace
        while i < len(out):
            if out[i] == "{":
                depth += 1
            elif out[i] == "}":
                depth -= 1
                if depth == 0:
                    break
            i += 1
        out = out[: m.start()] + out[i + 1 :]
    return out


def is_rust_test_path(rel_path: str) -> bool:
    """测试路径判定：tests 目录 / *tests.rs（测试内构造不直达用户，不入枚举）。"""
    parts = rel_path.split("/")
    file_name = parts[-1]
    if any(p == "tests" or p.endswith("_tests") for p in parts[:-1]):
        return True
    return file_name.endswith("tests.rs") or file_name.endswith("_tests.rs")


# collect_rust_coded_error_codes 的返回：去重排序后的码 + 无法静态解析的调用点计数
@dataclass(frozen=True)
class RustCodedCodesResult:
    codes: list[str]
    unresolved_call_sites: int


def walk_rust_files(root: str) -> list[str]:
    """递归收集 root 下全部 .rs 文件（target/ 剪枝是本守门扫描边界政策）：遍历机制归
    守门家族共享单点 walk_text_files（#1680 收口），扩展名闭集由库 RUST_EXTENSIONS 提供。"""
    return [
        f.abs
        for f in walk_text_files(
            root,
            "",
            WalkTextOptions(extensions=RUST_EXTENSIONS, skip_dirs=frozenset({"target"})),
        )
    ]


FN_DEF_TAIL_RE: Final[re.Pattern[str]] = re.compile(r"\bfn\s+$")
CONST_IDENT_RE: Final[re.Pattern[str]] = re.compile(r"[A-Z_][A-Z0-9_]*")


def collect_rust_coded_error_codes(rust_root: str) -> RustCodedCodesResult:
    """枚举 Rust 生产代码中全部码化错误码（字面量 + `const NAME: &str` 一级引用）。
    无法静态解析的动态构造点（宏参数、format! 拼接）计入 unresolved_call_sites，
    仅供人审，不作失败条件（漏报方向安全，见文件头边界说明）。"""
    const_table: dict[str, str] = {}
    sources: list[str] = []
    for path in walk_rust_files(rust_root):
        rel = path[len(rust_root) + 1 :]
        if is_rust_test_path(rel):
            continue
        src = strip_cfg_test_blocks(mask_non_code(Path(path).read_text(encoding="utf-8"), True))
        sources.append(src)
        for m in RUST_STR_CONST_RE.finditer(src):
            name = m.group(1)
            value = m.group(2)
            if name not in const_table:
                const_table[name] = value
    codes: set[str] = set()
    unresolved_call_sites = 0
    for src in sources:
        for m in CODED_CALL_RE.finditer(src):
            # 排除构造器定义处（fn coded(...) / pub fn codedp(...)）
            index = m.start()
            before = src[max(0, index - 30) : index]
            if FN_DEF_TAIL_RE.search(before):
                continue
            # 抓第一参数：跳过 ( 与空白，期待字符串字面量或常量标识符
            j = index + len(m.group(0))
            while j < len(src) and src[j] in (" ", "\t", "\r", "\n"):
                j += 1
            if j < len(src) and src[j] == '"':
                end = src.find('"', j + 1)
                if end != -1:
                    code = src[j + 1 : end]
                    if code:
                        codes.add(code)
                    continue
            ident = CONST_IDENT_RE.match(src[j:])
            if ident is not None:
                value = const_table.get(ident.group(0))
                if value is not None:
                    if value:
                        codes.add(value)
                    continue
            unresolved_call_sites += 1
    return RustCodedCodesResult(codes=sorted(codes), unresolved_call_sites=unresolved_call_sites)


def diff_coded_coverage(codes: list[str], error_template_keys: list[str]) -> list[str]:
    """码化模板缺失差集：枚举码有而 locale 模板没有的键"""
    keys = set(error_template_keys)
    return [c for c in codes if c not in keys]


# compare_locales_dir 返回：源语言名、参与比对的 locale 清单、失败明细
@dataclass(frozen=True)
class CompareResult:
    source_locale: str
    locales: list[str]
    failures: list[LocaleFailure]


def compare_locales_dir(locales_dir: str) -> CompareResult:
    """比对 locales 目录下源语言与其余 locale 的 key 集合。"""
    # 子目录列举归家族共享单点 read_dir_entries（#1742 收口）；原显式 localeCompare 排序
    # 与原语排序全等，输出序零变化。
    dirs = [e.name for e in read_dir_entries(locales_dir) if e.is_directory]
    source_keys = collect_locale_keys(os.path.join(locales_dir, SOURCE_LOCALE_DIR))
    failures: list[LocaleFailure] = []
    for locale in dirs:
        if locale == SOURCE_LOCALE_DIR:
            continue
        source_only, other_only = diff_key_sets(
            source_keys, collect_locale_keys(os.path.join(locales_dir, locale))
        )
        if source_only or other_only:
            failures.append(
                LocaleFailure(locale=locale, source_only=source_only, other_only=other_only)
            )
    return CompareResult(source_locale=SOURCE_LOCALE_DIR, locales=dirs, failures=failures)


def main() -> None:
    repo_root = Path(__file__).resolve().parents[1]
    locales_dir = sys.argv[1] if len(sys.argv) > 1 else str(repo_root / "locales")
    rust_root = sys.argv[2] if len(sys.argv) > 2 else str(repo_root / "src-app")
    result = compare_locales_dir(locales_dir)
    if not result.failures:
        print(
            f"✅ i18n key 全等：{' / '.join(result.locales)} "
            f"各语言 key 集合与源语言 {result.source_locale} 全等"
        )
    else:
        for f in result.failures:
            print(
                f"✗ locale「{f.locale}」与源语言「{result.source_locale}」key 集合不一致：",
                file=sys.stderr,
            )
            for key in f.source_only:
                print(f"  - 缺失：{key}", file=sys.stderr)
            for key in f.other_only:
                print(f"  - 多余：{key}", file=sys.stderr)
        sys.exit(1)

    # 码化错误模板覆盖守门（issue #1188 / ADR-0050）：每个码化构造码必须在
    # zh-CN 与 en-US 的 errors.json 都有模板（码内点号即 JSON 嵌套路径）。
    coded = collect_rust_coded_error_codes(rust_root)
    zh_errors = flatten_keys(
        json.loads(
            (Path(locales_dir) / "zh-CN" / "errors.json").read_text(encoding="utf-8")
        )
    )
    en_errors = flatten_keys(
        json.loads(
            (Path(locales_dir) / "en-US" / "errors.json").read_text(encoding="utf-8")
        )
    )
    missing = diff_coded_coverage(coded.codes, zh_errors) + diff_coded_coverage(
        coded.codes, en_errors
    )
    if missing:
        print(
            "✗ 码化错误模板缺失（ADR-0050：新增错误条件 = 码化构造器 + errors.json 两份补翻译）：",
            file=sys.stderr,
        )
        for code in dict.fromkeys(missing):
            print(f"  - 缺模板：{code}", file=sys.stderr)
        sys.exit(1)
    unresolved_note = (
        f"（另有 {coded.unresolved_call_sites} 处动态码构造点无法静态枚举，漏报方向安全）"
        if coded.unresolved_call_sites > 0
        else ""
    )
    print(
        f"✅ 码化错误模板覆盖：{len(coded.codes)} 个码化错误码在 zh-CN/en-US errors.json 均有模板"
        f"{unresolved_note}"
    )


# 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的比对函数
# （与 scripts/check-commands.ts 同一惯法）。
if __name__ == "__main__":
    main()
