#!/usr/bin/env -S uv run --quiet
# /// script
# requires-python = ">=3.12"
# dependencies = ["pytest==9.2.0"]
# ///
# 守门脚本测试运行器（issue #2174 / ADR-0083 修订注记）：scripts/ 侧测试的唯一
# 运行口径 = 本文件——PEP 723 内联依赖声明 pytest（版本钉死在此，单一事实源，
# 仓库零工具链 manifest），经 uv run 装配运行时后驱动 pytest 收集 scripts/ 下
# 全部 test_*.py（含包装测试与原语级测试）。测试自仓库根发起（进程 cwd = 仓库
# 根，被测门与夹具定位接缝，沿用 #1995 口径）。
#
# 挂载登记归 scripts/gate_mounts.py（check.sh + CI 双宿主，issue #1682）；
# check.sh 与 CI 的守门脚本测试步骤调用本文件，删除任一宿主挂载行即红
# （test_gate_mounts.py）。
#
# 单测文件开发期直跑：`uv run scripts/run_tests.py scripts/test_check_no_tauri.py`
# （首个参数后的内容透传 pytest，pytest 参数归 pytest 语义）。

from __future__ import annotations

import sys
from pathlib import Path

import pytest


def main() -> None:
    scripts_dir = str(Path(__file__).resolve().parent)
    # --import-mode=prepend：测试文件按文件名导入、目录插入 sys.path——与兄弟
    # 脚本模块（check_no_tauri 等）同目录互 import 的接缝；-p no:cacheprovider：
    # 不落 .pytest_cache（仓库工作树零检查器缓存残留）。
    args = [scripts_dir, "--import-mode=prepend", "-p", "no:cacheprovider", "-q", *sys.argv[1:]]
    raise SystemExit(pytest.main(args))


if __name__ == "__main__":
    main()
