from __future__ import annotations

import re
from pathlib import Path

from gate_mounts import (
    CHECK_SH_FILE,
    CI_NON_GATE_RUNS,
    CI_WORKFLOW_FILE,
    GATE_MOUNTS,
    GateHostFile,
    GateMount,
)

# 守门接线测试（issue #1682）：守门挂载登记的「删除即变红」核对载体，守门家族
# interface 的一环（先例收编：check-sh 三件套接线断言、运行时接线核对先例（
# check-frontend-structure 已随 Web 前端退役 #2171/#2172）、test-exec 守门规则④）。
# 断言对准用户可观察事实——「该门被质量门槛执行于该宿主」的可观察代理 = 宿主文件
# 的非注释执行行以登记形态行首命中（ADR-0087 断言强度：不对准行号与文本形状）。
# 核对，不生成（grilling 定案）：check.sh 与 CI workflow 保持手写，本测试断言
# 双向全等——
#   ① 登记 → 宿主：删任一宿主挂载行即红；
#   ② 宿主 → 登记：删 check.sh 门执行行 / 删登记条目即红（执行行无登记认领）；
#   ③ CI 按宿主投影同 ①②；非门 run 步骤走 CI_NON_GATE_RUNS 显式豁免（政策
#      无源就显式，#1591），判定面 = 挂门 job 内的单行 `run:` 步骤。
# 判定语义 = per-host 行首前缀：非注释行 trim 后行首匹配。echo 展示行行首是
# `echo "▶`，结构上永不可能命中执行形态——echo 假绿免疫是判定语义的结构后果
# （#1112 第三轮审查实测教训；末条用例钉住该语义）。

# 仓库根：pytest 进程 cwd 即仓库根（守门测试自仓库根发起；同
# gate_script_test_helper 的定位观察）。
REPO_ROOT = Path.cwd()


def read_host(file: str) -> str:
    return (REPO_ROOT / file).read_text(encoding="utf-8")


def mounted(line: str, prefix: str) -> bool:
    """宿主行是否命中挂载形态：非空、非注释、trim 后行首前缀。"""
    t = line.strip()
    return t != "" and not t.startswith("#") and t.startswith(prefix)


def mount_of(entry: GateMount, file: GateHostFile) -> tuple[str, str] | None:
    """条目在指定宿主的挂载声明（prefix, where）。"""
    for h in entry.hosts:
        if h.file == file:
            return (h.prefix, h.where)
    return None


def check_sh_gate_lines(content: str) -> list[str]:
    """check.sh 门执行行：首个 `echo "▶` 门标签行之后的非注释、非 echo 行。
    门段起点锚定标签结构——其前是 shebang、头部注释与运行时缺失守卫，皆非门步骤；
    门段内每个门执行行前必有标签行，echo 行与注释行不入执行集合。
    """
    out: list[str] = []
    started = False
    for raw in content.split("\n"):
        t = raw.strip()
        if not started:
            if t.startswith('echo "▶'):
                started = True
            continue
        if t == "" or t.startswith("#") or t.startswith("echo"):
            continue
        out.append(t)
    return out


def ci_run_lines_by_job(content: str) -> dict[str, list[str]]:
    """CI 单行 `run:` 步骤按 job 分组（YAML 列表项装饰 `- ` 剥除后比对）。
    `run: |` / `run: >` 多行脚本块与 `run:` 空值（defaults 键）不入判定面——
    门挂载一律以单行执行形态表达，多行块内挂载会因登记方向无行可认领而变红。
    """
    jobs: dict[str, list[str]] = {}
    in_jobs = False
    current = ""
    for raw in content.split("\n"):
        if raw == "jobs:":
            in_jobs = True
            continue
        if not in_jobs:
            continue
        job_match = re.match(r"^ {2}([A-Za-z_][A-Za-z0-9_-]*):\s*$", raw)
        if job_match is not None:
            current = job_match.group(1)
            jobs[current] = []
            continue
        if current == "":
            continue
        without_decoration = re.sub(r"^- ", "", raw.strip(), count=1)
        if not without_decoration.startswith("run:"):
            continue
        value = without_decoration[len("run:") :].strip()
        if value == "" or value.startswith("|") or value.startswith(">"):
            continue
        jobs[current].append(without_decoration)
    return jobs


def prefixes_for(file: GateHostFile) -> list[str]:
    """宿主的登记前缀清单（该宿主侧的认领面）。"""
    out: list[str] = []
    for entry in GATE_MOUNTS:
        m = mount_of(entry, file)
        if m is not None:
            out.append(m[0])
    return out


class Test守门挂载登记:
    def test_registry_structure_invariants(self) -> None:
        # 登记结构不变量：名称唯一、每条含 check.sh 宿主与 issue/ADR 出处
        assert len(GATE_MOUNTS) > 0  # 拒绝空集假绿
        names = [entry.name for entry in GATE_MOUNTS]
        assert len(set(names)) == len(names), "登记名称必须唯一"
        for entry in GATE_MOUNTS:
            assert entry.refs.strip() != "", f"出处（导航字段）缺失：{entry.name}"
            sh = mount_of(entry, CHECK_SH_FILE)
            assert sh is not None, f"缺 check.sh 宿主声明：{entry.name}"
            assert sh[0].strip() != "", f"check.sh 执行行形态为空：{entry.name}"
            assert sh[1].strip() != "", f"宿主定位说明为空：{entry.name}"

    def test_check_sh_lines_match_registry_bidirectionally(self) -> None:
        # check.sh 执行行 ↔ 登记双向全等（删执行行即红、删登记条目即红）
        lines = check_sh_gate_lines(read_host(CHECK_SH_FILE))
        assert len(lines) > 0, "check.sh 门执行行提取为空——门标签结构漂移会让核对空转"
        prefixes = prefixes_for(CHECK_SH_FILE)
        for line in lines:
            claims = [p for p in prefixes if mounted(line, p)]
            assert len(claims) == 1, f"check.sh 执行行未被任何登记条目认领（删登记条目即红）：{line}"
        for entry in GATE_MOUNTS:
            m = mount_of(entry, CHECK_SH_FILE)
            prefix = m[0] if m is not None else ""
            assert any(
                mounted(line, prefix) for line in lines
            ), f"登记条目在 check.sh 无执行行（删宿主挂载即红）：{entry.name}"

    def test_ci_steps_match_registry_bidirectionally(self) -> None:
        # CI 步骤 ↔ 登记按宿主投影双向全等（删步骤即红、删登记条目即红）
        ci_prefixes = prefixes_for(CI_WORKFLOW_FILE)
        assert len(ci_prefixes) > 0, "CI 宿主零登记——挂载丢失会让本核对空转"
        jobs = ci_run_lines_by_job(read_host(CI_WORKFLOW_FILE))
        gate_jobs = [
            job
            for job, lines in jobs.items()
            if any(any(line.startswith(p) for p in ci_prefixes) for line in lines)
        ]
        assert len(gate_jobs) > 0, "CI 无任何门步骤命中登记（登记的 CI 挂载全部丢失）"
        for job in gate_jobs:
            for line in jobs.get(job, []):
                claims = [p for p in ci_prefixes if line.startswith(p)]
                if line in CI_NON_GATE_RUNS:
                    assert (
                        len(claims) == 0
                    ), f"CI 非门步骤被登记误认领（豁免清单与登记冲突）：{line}"
                    continue
                assert len(claims) == 1, f"CI {job} 步骤未被任何登记条目认领（删登记条目即红）：{line}"
        all_ci_lines = [line for lines in jobs.values() for line in lines]
        for entry in GATE_MOUNTS:
            m = mount_of(entry, CI_WORKFLOW_FILE)
            if m is None:
                continue  # CI 未挂载的门（如经构建脚本承接的前端类型检查）
            assert any(
                line.startswith(m[0]) for line in all_ci_lines
            ), f"登记条目在 CI 无步骤（删宿主挂载即红）：{entry.name}"

    def test_echo_and_comment_lines_cannot_hit_mount(self) -> None:
        # echo 展示行与注释行结构上不可能命中挂载（行首前缀判定，#1112 假绿教训）
        invocation = "uv run scripts/check_structure.py"
        # 真实 check.sh 的门标签行形态：命令字样在 echo 参数里，行首是标签引导词。
        assert mounted(f'echo "▶ 结构守门检查 ({invocation})"', invocation) is False
        assert mounted(f"# {invocation}", invocation) is False
        assert mounted(f"  {invocation}  ", invocation) is True
