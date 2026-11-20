#!/usr/bin/env -S uv run --quiet
# /// script
# requires-python = ">=3.12"
# dependencies = [
#   "mypy==2.4.1",
#   "pytest==9.2.0",
# ]
# ///
# 守门脚本类型检查门（issue #734/#740 门保留，运行时迁移 issue #2174 /
# ADR-0083 修订注记）：TS 版类型检查（vue-tsc → 原生 tsc，tsconfig.scripts.json）
# 随 Python 化退役，宿主改为 mypy——纯 Python 工具链（pyright 依赖 node 运行时，
# 与「仓库零 JS 工具链」目标相悖，排除），PEP 723 内联依赖钉死版本（单一事实源，
# 仓库零工具链 manifest），经 uv run 装配后类型检查 scripts/ 下全部脚本与测试。
# 工具链形态（沿用 #2171/#2172 裁决：仓库零 manifest、运行时拉取、不考虑离线
# 环境）。
#
# 门槛水位：--strict（与 TS 版 strict 套件同精神）——移植代码全量显式标注，
# 门槛新增不放宽。pytest 与 mypy 同环境装配（测试文件的 pytest 导入须可解析类型），
# 两版本钉死与本测试运行器 run_tests.py 同源一致。
#
# 挂载登记归 scripts/gate_mounts.py（check.sh + CI 双宿主，issue #1682）；
# check.sh 首门与 CI gates job 首步调用本文件，删除任一宿主挂载行即红
# （test_gate_mounts.py）。

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

# mypy 门槛面：scripts/ 全部 Python 源（门脚本 + 共享库 + 测试 + 测试助手 +
# 运行器）。mypy 配置内联在调用参数（无 manifest，单一事实源 = 本文件）；
# 进程 cwd = scripts/——顶层平铺模块（无 __init__.py）按裸模块名解析，兄弟互
# import 的口径与 pytest --import-mode=prepend 一致。mypy 缓存放仓库根
# .mypy_cache/（.gitignore 豁免）。
MYPY_ARGS: list[str] = [
    "--strict",
]


def main() -> None:
    repo_root = Path(__file__).resolve().parent.parent
    cmd = [sys.executable, "-m", "mypy", *MYPY_ARGS, "--cache-dir", str(repo_root / ".mypy_cache"), "."]
    result = subprocess.run(cmd, check=False, cwd=str(Path(__file__).resolve().parent))
    raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
