#!/usr/bin/env -S uv run --quiet
# 东财行情面端点与常量零残留守门（issue #1572 / ADR-0130 守门判据①——源码扫描，
# 删除任一遗漏即红）：
# ADR-0130 决策 1 移除东方财富后，行情面不得再出现任何东财端点引用与常量。
# 本守门以「东财自有域名」为禁令标记做全仓源码扫描：任何东财端点的引用必然
# 携带其自有域名（*.eastmoney.com / dfcfw.com / fundgz.1234567.com.cn），换主机、
# 换路径、换常量名都藏不出这一层；重新引入任一东财通道（哪怕只进测试夹具）即红。
#
# 禁令取「域名」而非裸词 eastmoney/东财：存量来源标记 `eastmoney` 是历史事实
# （ADR-0130 决策 7——种子夹具、类型闭集与断言合法在场），ADR / 词汇表 / 调研
# 档案合法讨论东财（历史叙述），裸词禁令会把这些全打红，语义过宽；域名禁令
# 精确对准「端点回归」这一判据本体。
#
# 已登记例外（严格相等校验，与 check-infra-dml 的 REGISTERED_EXCEPTIONS 同纪律）：
# 实际命中 ≠ 登记数、登记文件消失、例外已收敛（命中清零）都红。当前清单为空：
# 唯一例外（东财 FX 日 K 腿主机池）已随 #1551 换 ECB 退役并删除登记，守门收紧到
# 零例外（同 check-infra-dml 自 #1108 起的空清单稳态）；今后若出现新的合法在场，
# 须在此逐条登记（文件 + 预期命中数 + 理由）。
#
# 扫描边界：全仓文本面（rs/md/json/sql/sh/html/toml/yml/yaml；ts/tsx/vue 面已随 Web 前端退役删除，#2171/#2172）。
# docs/ 整域豁免——ADR、调研档案与词汇表的历史叙述（含端点 URL 引证）是其职责
# 本体；CHANGELOG.md 豁免（历史条目）；守门脚本与其包装测试豁免（禁令字面量以
# 数据形态住在其中）。豁免全部 fail loud：登记文件消失即红（清单漂移），不放任
# 守门空转。已知文本不可达逃逸形态（靠评审兜底）：经变量拼接或编码（URL encode、
# IDN）的域名、不携带东财域名的代理中转形态，以及不携带域名的纯常量形态（如
# secid 数字前缀映射、报文字段布局常量——单点回归无域名可扫，评审时按
# ADR-0130 决策 2「东财 secid 前缀映射退役」人工判认）。
#
# Python 化 + uv 运行时（issue #2174 / ADR-0083 修订注记）：类型经
# scripts/check_scripts_types.py（mypy --strict）门槛检查；调用方式
# `uv run scripts/check_eastmoney_residue.py`。
# 默认校验本仓库；测试可传位置参数指向夹具仓库根：
# uv run scripts/check_eastmoney_residue.py [root]
# 包装测试 scripts/test_check_eastmoney_residue.py。
# 行号定位与目录遍历消费 gate_primitives.py（#1680 库归库、门归门）导出的家族共享原语
# （line_at / walk_text_files，issue #1625）；禁令标记与豁免面属本守门政策，自持。

from __future__ import annotations

import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path

from gate_primitives import WalkTextOptions, line_at, locale_sort_key, walk_text_files

DEFAULT_ROOT = str(Path(__file__).resolve().parent.parent)

# 守门脚本本体（相对仓库根）：豁免自证对象，缺失即清单漂移红
SELF_REL = "scripts/check_eastmoney_residue.py"
# 包装测试（相对仓库根）：同上，禁令字面量的另一处数据载体
WRAPPER_TEST_REL = "scripts/test_check_eastmoney_residue.py"

# 扫描的文本面扩展名：源码、契约、脚本与文档化文本；二进制资产不在判据面
SCAN_EXTENSIONS = frozenset(
    {".rs", ".md", ".json", ".sql", ".sh", ".html", ".toml", ".yml", ".yaml", ".feature"}
)

# 整目录豁免：依赖与产物（非源码面）、docs/（历史叙述职责本体，见文件头）
SKIP_DIRS = frozenset({"node_modules", "target", ".git", "docs", "dist", "coverage"})

# 整文件豁免（相对仓库根）：历史记录与禁令字面量的数据载体，逐条登记于 SELF/CHANGELOG
SKIP_FILES = frozenset({"CHANGELOG.md", SELF_REL, WRAPPER_TEST_REL})


def fail(message: str) -> None:
    print(f"✗ 东财行情面零残留守门：{message}", file=sys.stderr)
    sys.exit(1)


# 单条残留命中：仓库相对路径、行号（1 起算）、命中的禁令标记、原文行
@dataclass(frozen=True)
class ResidueHit:
    file: str
    line: int
    marker: str
    text: str


# 已登记例外条目：东财端点域名的合法在场（当前仅 FX 腿），逐条附理由
@dataclass(frozen=True)
class ResidueException:
    file: str
    count: int
    why: str


# 已登记例外（严格相等校验）：实际命中 ≠ 登记数即红（漂移——多出未登记新命中、
# 少掉说明对应腿已退役），例外清零也红（登记条目应随退役删除），登记文件消失
# 同样红（清单漂移，与 EXEMPT_FILES 家族纪律一致）。当前为空：东财 FX 日 K 腿
# 主机池已随 #1551 换 ECB 退役，登记条目随之删除，守门收紧到零例外。
REGISTERED_EXCEPTIONS: tuple[ResidueException, ...] = ()

# 禁令标记：东财自有域名闭集。任何东财端点的引用（主机池、URL、Referer、
#  文档化常量）必然携带其一；新增东财关联域名须显式扩此清单并复核动机。
BANNED_MARKERS: tuple[str, ...] = ("eastmoney.com", "dfcfw", "1234567.com.cn")


# 转义为字面量正则（域名含点），大小写不敏感（主机常量可能全大写）。
# （转义用 re.escape：对本闭集的 ASCII 域名标记与 JS 侧字符类转义逐字全等，#2174）
def marker_pattern(marker: str) -> re.Pattern[str]:
    return re.compile(re.escape(marker), re.IGNORECASE)


# 扫描单个文本：返回全部禁令标记命中（行号 1 起算，原文行去首尾空白）
def scan_residue(rel: str, source: str) -> list[ResidueHit]:
    hits: list[ResidueHit] = []
    lines = source.split("\n")
    for marker in BANNED_MARKERS:
        for m in marker_pattern(marker).finditer(source):
            line = line_at(source, m.start())
            raw_line = lines[line - 1] if line - 1 < len(lines) else ""
            hits.append(ResidueHit(file=rel, line=line, marker=marker, text=raw_line.strip()))
    return sorted(hits, key=lambda h: (h.line, locale_sort_key(h.marker)))


def main() -> None:
    root = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_ROOT
    if not os.path.exists(root):
        fail(f"扫描根不存在：{root}")

    # 豁免自证先行：脚本本体与包装测试必须在场（清单漂移 fail loud，不放任守门空转）
    for rel in (SELF_REL, WRAPPER_TEST_REL):
        if not os.path.exists(os.path.join(root, rel)):
            fail(f"豁免清单漂移：{rel} 不存在——守门或其包装测试被改名/搬迁，请同步 SKIP_FILES 登记")

    # 单次收集全仓文件（家族共享单点 walk_text_files，abs 读文件 / rel 报文定位），逐文件扫描
    #（读文件与扫描同轮完成，不做二次遍历）
    scanned = walk_text_files(
        root,
        "",
        WalkTextOptions(extensions=SCAN_EXTENSIONS, skip_dirs=SKIP_DIRS, skip_files=SKIP_FILES),
    )
    hits = [hit for f in scanned for hit in scan_residue(f.rel, Path(f.abs).read_text("utf-8"))]
    by_file: dict[str, list[ResidueHit]] = {}
    for hit in hits:
        by_file.setdefault(hit.file, []).append(hit)

    problems: list[str] = []
    # 未登记命中：逐条定位 文件:行 + 标记 + 原文行
    registered = {e.file: e for e in REGISTERED_EXCEPTIONS}
    for file, file_hits in by_file.items():
        if file in registered:
            continue
        for hit in file_hits:
            problems.append(
                f"✗ {file}:{hit.line} 命中禁令标记「{hit.marker}」：{hit.text}\n"
                f"    —— 东财行情面端点已随 ADR-0130 决策 1 删除，不得回归（守门判据①）"
            )
    # 例外三向校验：文件消失（清单漂移）、命中清零（例外已收敛）、计数漂移
    for e in REGISTERED_EXCEPTIONS:
        actual = by_file.get(e.file)
        if actual is None and not os.path.exists(os.path.join(root, e.file)):
            problems.append(
                f"✗ 例外清单漂移：{e.file} 不存在——{e.why}，请复核后更新 REGISTERED_EXCEPTIONS"
            )
            continue
        count = len(actual) if actual is not None else 0
        if count == 0:
            problems.append(
                f"✗ 例外已收敛：{e.file} 零命中——{e.why}；删除 REGISTERED_EXCEPTIONS 登记条目，"
                "守门收紧到零例外"
            )
        elif count != e.count:
            problems.append(
                f"✗ 例外计数漂移：{e.file} 实际命中 {count} 处 ≠ 登记 {e.count} 处——"
                f"{e.why}；逐条复核 {file_hits_label(actual)} 后同步登记"
            )

    if len(problems) > 0:
        print(f"✗ 东财行情面零残留守门：{len(problems)} 处问题\n{'\n'.join(problems)}", file=sys.stderr)
        sys.exit(1)
    print(
        f"✓ 东财行情面零残留守门通过：扫描 {len(scanned)} 个文件，未登记端点域名零命中；"
        f"已登记例外 {len(REGISTERED_EXCEPTIONS)} 条"
        + (
            f"（{'、'.join(e.file for e in REGISTERED_EXCEPTIONS)}）"
            if len(REGISTERED_EXCEPTIONS) > 0
            else "（零例外）"
        )
    )


def file_hits_label(hits: list[ResidueHit] | None) -> str:
    return "、".join(f"{h.file}:{h.line}" for h in (hits or []))


# 仅直接运行时执行 main；包装测试以子进程调用（行为等价判据：只测外部可观察结果）。
if __name__ == "__main__":
    main()
