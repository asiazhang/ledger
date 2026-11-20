# 守门脚本包装测试的共享 run 辅助唯一定义点（issue #1439）：uv spawn 包装此前在
# 守门测试文件逐份手写（TS 时代 13 份副本随运行时迁移一并收敛，issue #2174），
# 语义全同——以门槛同款运行时执行被测脚本（ADR-0083 修订注记：uv 调用与
# check.sh / CI 门槛调用同款，测的就是门槛路径），只取外部可观察接缝：
# 退出码与合并输出（stdout + stderr）。新增守门包装测试直接消费本文件，
# 不再复制副本；各测试文件只保留被测脚本路径定位（gate_script）与一步绑定 run。
# 测试与所测脚本同目录住（#1158）。

from __future__ import annotations

import subprocess
from dataclasses import dataclass
from pathlib import Path


# 一次守门脚本运行的外部可观察结果（ADR-0083 修订注记：退出码与输出即测试接缝）
@dataclass(frozen=True)
class RunResult:
    status: int
    output: str


# 定位被测守门脚本：仓库根下 scripts/<name>.py。
# （守门测试唯一运行口径自仓库根发起，进程 cwd 即仓库根；口径沿用 #1995 的
#  cwd 接缝——测试 runner（scripts/run_tests.py）自仓库根发起 pytest，测试内
#  cwd 不变，调用面零变化）
def gate_script(name: str) -> str:
    return str(Path.cwd() / "scripts" / name)


# 以 uv 运行被测守门脚本（spawn 'uv run' 与门槛调用同款，ADR-0083 修订注记），
# 返回退出码与合并输出（stdout + stderr）。args 缺省为空参（真实仓库默认
# 路径场景）；cwd 可指定（守门的路径归一化以 cwd 为基准，夹具运行时归一化
# 须落在夹具上）。登记偏差（code review #2174 留痕）：命令另带 --quiet——
# 抑制 uv 首跑装包进度噪声，避免污染退出码/输出接缝的断言；门本体命令形态
# 与登记前缀一致（uv run scripts/<name>.py）。
def run_gate_script(
    script: str, args: tuple[str, ...] | list[str] = (), cwd: str | None = None
) -> RunResult:
    r = subprocess.run(
        ["uv", "run", "--quiet", script, *args],
        capture_output=True,
        text=True,
        cwd=cwd,
        check=False,
    )
    return RunResult(status=r.returncode, output=(r.stdout or "") + (r.stderr or ""))
