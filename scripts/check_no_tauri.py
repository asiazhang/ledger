#!/usr/bin/env -S uv run --quiet
# 全仓无 tauri 守门（issue #2083 / ADR-0145 决策 4/7 切轨后置门）：清单形状面，与命令型门
# `cargo tree --workspace -i tauri`（check.sh / CI backend-lint 同挂，辖**解析后的
# 依赖全图**，含传递边）互补。本门辖「声明形状」：
#
# 面① Cargo 清单（src-app/Cargo.toml + src-app/crates/*/Cargo.toml）：注释掩码后
#   出现 `tauri` 词即红（TOML 注释掩码后）——覆盖 [dependencies] / [dev-dependencies] /
#   [build-dependencies] / [features] / [workspace.dependencies] 全段；tauri
#   只经清单进入依赖图，清单零声明即图零可达（与 cargo tree 门双保险：彼门
#   辖解析结果，本门辖声明文本，漂移任一侧即红）。
#
# 前端死路径面（原面②：src/ + packages/ 的 .ts/.vue + package.json 的
# @tauri-apps 扫描）随 Web 参照前端整体退役删除（#2171/#2172）——扫描根
# 不复存在，面归 Cargo 清单单一面。
#
# 判定语义（fail loud）：扫描面至少扫到一个文件（拒绝空集假绿，与 check-structure
# 同款哨兵）；注释掩码：Cargo 清单自持 TOML 行注释掩码（`#` 行注释，字符串感知）
# ——注释里的历史提及时过境迁不误报，声明与字面命中才算。Cargo 清单掩码后
# **全文**匹配（含字符串值：description 提及 tauri 同样视为残留——切轨后不应有
# 任何需要提及 tauri 的清单内容）。
# Python 化 + uv 运行时（issue #2174 / ADR-0083 修订注记）：类型经
# scripts/check_scripts_types.py（mypy）门槛检查；调用方式 `uv run scripts/check_no_tauri.py`。
# 默认校验本仓库；测试可传位置参数指向夹具仓库根：`uv run scripts/check_no_tauri.py [repo-root]`。
# 挂载登记归 scripts/gate_mounts.py（check.sh + CI 双宿主，issue #1682）。
# 包装测试 scripts/test_check_no_tauri.py。

from __future__ import annotations

import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path

# Cargo 清单出现即红的词（词边界：tauri-plugin-* / tauri-build 同命中）。
TAURI_WORD = re.compile(r"\btauri\b")

# 扫描根（相对仓库根）：Rust workspace 根与成员 crate 清单。
CARGO_MANIFEST_GLOB_DIRS: tuple[str, ...] = ("src-app", "src-app/crates")


# TOML 注释掩码（逐行，不跨行）：行内首个 `#` 起到行尾换空格——TOML 的 `#`
# 只在字符串字面量外成注释；清单内的注释提及（历史门退役注记等）不算声明。
def mask_toml_comments(text: str) -> str:
    out_lines: list[str] = []
    for line in text.split("\n"):
        chars = list(line)
        in_string = False
        quote = ""
        i = 0
        while i < len(chars):
            c = chars[i]
            if in_string:
                if c == "\\":
                    i += 1  # 转义对：连同下字符一并跳过（循环尾再 +1）
                elif c == quote:
                    in_string = False
            elif c == '"' or c == "'":
                in_string = True
                quote = c
            elif c == "#":
                for k in range(i, len(chars)):
                    chars[k] = " "
                break
            i += 1
        out_lines.append("".join(chars))
    return "\n".join(out_lines)


# 仓库内全部 Cargo.toml 清单（workspace 根 + crates 成员；夹具根同构）。
def cargo_manifests(root: str) -> list[str]:
    out: list[str] = []
    for dir_name in CARGO_MANIFEST_GLOB_DIRS:
        abs_path = os.path.join(root, dir_name, "Cargo.toml")
        if os.path.exists(abs_path):
            out.append(abs_path)
    crates_dir = os.path.join(root, "src-app", "crates")
    if os.path.isdir(crates_dir):
        for name in sorted(os.listdir(crates_dir)):
            manifest = os.path.join(crates_dir, name, "Cargo.toml")
            if os.path.isdir(os.path.join(crates_dir, name)) and os.path.exists(manifest):
                out.append(manifest)
    return sorted(out)


# 单条命中：文件（相对仓库根）+ 行号（1 起算）+ 行原文。


@dataclass(frozen=True)
class TauriHit:
    file: str
    line: int
    text: str


# Cargo 清单注释掩码后 `tauri` 词扫描。
def scan_cargo_manifests(root: str) -> list[TauriHit]:
    hits: list[TauriHit] = []
    for abs_path in cargo_manifests(root):
        rel = os.path.relpath(abs_path, os.path.abspath(root))
        masked = mask_toml_comments(Path(abs_path).read_text(encoding="utf-8"))
        raw_lines = masked.split("\n")
        for i, raw_line in enumerate(raw_lines):
            if TAURI_WORD.search(raw_line):
                hits.append(TauriHit(file=rel, line=i + 1, text=raw_line.strip()))
    return hits


def main() -> None:
    repo_root = sys.argv[1] if len(sys.argv) > 1 else str(Path(__file__).resolve().parent.parent)
    problems: list[str] = []
    cargo_hits = scan_cargo_manifests(repo_root)

    if len(cargo_manifests(repo_root)) == 0:
        problems.append("✗ 全仓无 tauri 守门：Cargo 清单扫描集为空（src-app 根缺失？）——拒绝空集假绿")
    for hit in cargo_hits:
        problems.append(
            f"✗ 全仓无 tauri 守门（Cargo 清单）：{hit.file}:{hit.line} 出现 tauri 声明\n"
            f"    {hit.text}\n"
            "    Tauri 壳已随 #2083 切轨退役（ADR-0145 决策 4/7）——清单零声明即依赖图\n"
            "    零可达；命令型门 `cargo tree --workspace -i tauri` 辖解析结果，本门辖声明文本"
        )

    if problems:
        for p in problems:
            print(p, file=sys.stderr)
        print(
            f"❌ 全仓无 tauri 守门失败：{len(problems)} 处残留（Cargo 清单 {len(cargo_hits)}；"
            f"issue #2083 / ADR-0145）",
            file=sys.stderr,
        )
        sys.exit(1)
    print(
        f"✓ 全仓无 tauri 守门：Cargo 清单 {len(cargo_manifests(repo_root))} 份零 tauri 声明"
        f"（#2083 / ADR-0145）"
    )


# 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的扫描函数。
if __name__ == "__main__":
    main()
