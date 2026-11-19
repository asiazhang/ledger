#!/usr/bin/env -S uv run --quiet
# 版本元数据一致性守门（issue #1957 / ADR-0022 修订注记）：应用版本在两个文件里
# 各写一次——根 `src-app/Cargo.toml`
#（壳层版本真源，`CARGO_PKG_VERSION` 消费）与 `src-app/nsh.json`（nsh package
# 打包版本与更新器比较面真源，#2134 起加入）。三处互不引用，任何一处漏改都会
# 让版本漂移——本门把它钉死。`src-tauri/tauri.conf.json` 腿随 Tauri 壳
# 退役（#2083）；`package.json` 腿（Web 参照前端元数据）随前端退役
#（#2171/#2172），判定面收敛为 Rust 壳两腿。
#
# 判定：两处相等即绿；不相等、文件缺失、字段读不出（fail loud，不静默跳过）
# 一律非零退出并逐条列出。`Cargo.lock` 不入判定面——它是 cargo 的产物，
# 随 `cargo build`/`cargo update` 自动跟随，纳入判定只会制造与真实元数据无关的假红。
#
# 扫描边界：只读两份元数据的顶层 version 字段（Cargo.toml 取 `[package]` 段内的
# 那一个），不解析依赖区、不比对依赖版本——依赖升级纪律归
# `docs/agents/dependency-upgrades.md`，与发布版本号是两件事。
#
# Python 化 + uv 运行时（issue #2174 / ADR-0083 修订注记）：类型经
# scripts/check_scripts_types.py（mypy --strict）门槛检查；调用方式
# `uv run scripts/check_versions.py`。
# 默认校验本仓库；测试可传位置参数指向夹具根：`uv run scripts/check_versions.py [root]`。
# 挂载登记归 scripts/gate_mounts.py（check.sh + CI 双宿主，issue #1682）。
# 包装测试 scripts/test_check_versions.py。

from __future__ import annotations

import json
import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path


# 一处版本来源的读取结果：读到即 `version`，读不到即 `problem`（两者互斥）。
@dataclass(frozen=True)
class VersionSourceReading:
    file: str
    version: str | None
    problem: str | None


# 判定面清单（顺序即输出顺序）：两份元数据文件各一条（nsh.json 腿随 #2134
# 更新器重建加入——nsh package 的打包版本与更新器比较面真源；package.json 腿
# 随 Web 参照前端退役删除，#2171/#2172）。
VERSION_SOURCES: tuple[str, ...] = (
    "src-app/Cargo.toml",
    "src-app/nsh.json",
)


# 从 JSON 元数据读顶层 version（nsh.json 腿）。
def read_json_version(text: str) -> str | None:
    try:
        parsed: object = json.loads(text)
    except ValueError:  # 含 json.JSONDecodeError（ValueError 子类，单 catch 即全）
        return None
    if not isinstance(parsed, dict):
        return None
    version = parsed.get("version")
    return version if isinstance(version, str) and version != "" else None


# 从 Cargo.toml 读 `[package]` 段的 version（依赖区同名键不算）。
def read_cargo_package_version(text: str) -> str | None:
    in_package = False
    for raw in text.split("\n"):
        line = raw.strip()
        if line.startswith("["):
            in_package = line == "[package]"
            continue
        if not in_package:
            continue
        m = re.match(r'^version\s*=\s*"([^"]+)"', line)
        if m is not None:
            return m.group(1)
    return None


# 读取全部版本来源：文件缺失与字段读不出各记一条 problem（fail loud）。
def read_version_sources(root: str) -> list[VersionSourceReading]:
    readings: list[VersionSourceReading] = []
    for file in VERSION_SOURCES:
        path = os.path.join(root, file)
        if not os.path.exists(path):
            readings.append(VersionSourceReading(file=file, version=None, problem="文件不存在"))
            continue
        text = Path(path).read_text(encoding="utf-8")
        version = (
            read_cargo_package_version(text) if file.endswith(".toml") else read_json_version(text)
        )
        readings.append(
            VersionSourceReading(
                file=file,
                version=version,
                problem=None if version is not None else "读不出顶层 version 字段",
            )
        )
    return readings


# 一致性判定：读失败或版本不等即返回失败原因清单（空数组 = 通过）。
def version_problems(readings: list[VersionSourceReading]) -> list[str]:
    problems: list[str] = []
    for reading in readings:
        if reading.problem is not None:
            problems.append(f"{reading.file}：{reading.problem}")
    versions = {r.version for r in readings if r.version is not None}
    if len(versions) > 1:
        problems.append(
            f"版本号不一致：{' / '.join(f'{r.file}={r.version if r.version is not None else "（读不出）"}' for r in readings)}"
            "（三处必须相等：任一文件漏改都会让「关于」页、更新比较与安装包声明各说一个数）"
        )
    return problems


def main() -> None:
    root = sys.argv[1] if len(sys.argv) > 1 else str(Path(__file__).resolve().parent.parent)
    readings = read_version_sources(root)
    problems = version_problems(readings)

    if len(problems) > 0:
        for problem in problems:
            print(f"版本元数据一致性：{problem}", file=sys.stderr)
        print(f"❌ 版本元数据一致性检查失败：{len(problems)} 处问题（见 issue #1957）", file=sys.stderr)
        sys.exit(1)

    version = readings[0].version if readings else ""
    print(
        f"✓ 版本元数据一致：{version}（{' / '.join(VERSION_SOURCES)}）——"
        "「关于」页、更新比较与安装包声明同源"
    )


# 仅直接运行时执行 main；被测试 import 时只取导出的判定函数。
if __name__ == "__main__":
    main()
