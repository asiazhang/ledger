# 被测对象是仓库工具脚本 scripts/check_versions.py（版本元数据一致性守门，
# issue #1957 / ADR-0022 修订注记）。脚本以 uv 运行时执行（ADR-0083 修订注记，
# #2174）：run_gate_script 以 subprocess('uv run') 与门槛调用同款拉起，测的就是门槛
# 路径。按测试决策只测外部可观察结果——进程退出码与输出，通过位置参数把目标
# 指向临时夹具目录（形状同构仓库根：两份元数据文件）。

from __future__ import annotations

import json
import os
from pathlib import Path

import pytest

from gate_script_test_helper import RunResult, gate_script, run_gate_script

SCRIPT = gate_script("check_versions.py")


def run(args: tuple[str, ...] = ()) -> RunResult:
    return run_gate_script(SCRIPT, args)


# 两份元数据的最小可解析形态；值为 None 表示刻意省略该文件。
DEFAULT_FILES: dict[str, str | None] = {
    "src-app/Cargo.toml": "\n".join(
        [
            "[package]",
            'name = "nsh-app"',
            'version = "1.2.3"',
            "",
            "[dependencies]",
            'serde = { version = "1" }',
        ]
    ),
    "src-app/nsh.json": json.dumps(
        {"name": "Ledger", "id": "com.zhangheng.ledger.nsh", "version": "1.2.3"},
        indent=2,
    ),
}


# 建夹具目录（形状同构仓库根）；值为 None 表示刻意省略该文件。
def make_fixture(root: Path, files: dict[str, str | None] | None = None) -> str:
    merged: dict[str, str | None] = {**DEFAULT_FILES, **(files or {})}
    for name, content in merged.items():
        if content is None:
            continue
        abs_path = os.path.join(str(root), name)
        os.makedirs(os.path.dirname(abs_path), exist_ok=True)
        Path(abs_path).write_text(content, encoding="utf-8")
    return str(root)


class TestCheckVersions版本元数据一致性守门:
    def test_default_repo_check_all_green(self) -> None:
        # 默认校验本仓（门槛路径）：两处版本一致全绿
        r = run()
        assert r.status == 0
        assert "版本元数据一致" in r.output
        assert "src-app/Cargo.toml / src-app/nsh.json" in r.output

    def test_fixture_consistent_all_green(self, tmp_path: Path) -> None:
        # 夹具两处一致全绿（位置参数指向夹具根）
        r = run((make_fixture(tmp_path / "consistent"),))
        assert r.status == 0
        assert "1.2.3" in r.output

    def test_nsh_json_drift_is_red(self, tmp_path: Path) -> None:
        # nsh.json 漂移即红（#2134 更新器版本真源腿：nsh package 打包版本与更新器比较面）
        root = tmp_path / "nsh-drift"
        root.mkdir()
        make_fixture(
            root,
            {
                "src-app/nsh.json": json.dumps(
                    {"name": "Ledger", "id": "com.zhangheng.ledger.nsh", "version": "1.2.0"},
                    indent=2,
                ),
            },
        )
        r = run((str(root),))
        assert r.status == 1
        assert "版本号不一致" in r.output
        assert "src-app/nsh.json=1.2.0" in r.output
        assert "src-app/Cargo.toml=1.2.3" in r.output

    def test_cargo_toml_drift_is_red_with_both_values(self, tmp_path: Path) -> None:
        # Cargo.toml 漂移即红，并列出两处取值（覆盖 cargo 解析腿：依赖区 version 不参与判定）
        root = tmp_path / "cargo-drift"
        root.mkdir()
        make_fixture(
            root,
            {
                "src-app/Cargo.toml": "\n".join(
                    [
                        "[package]",
                        'name = "nsh-app"',
                        'version = "1.2.0"',
                        "",
                        "[dependencies]",
                        'serde = { version = "1.2.3" }',
                    ]
                ),
            },
        )
        r = run((str(root),))
        assert r.status == 1
        assert "版本号不一致" in r.output
        assert "src-app/Cargo.toml=1.2.0" in r.output
        assert "src-app/nsh.json=1.2.3" in r.output

    def test_missing_file_is_red_and_named(self, tmp_path: Path) -> None:
        # 文件缺失即红并指名缺失文件（fail loud，不静默跳过）
        root = tmp_path / "missing-file"
        root.mkdir()
        make_fixture(root, {"src-app/Cargo.toml": None})
        r = run((str(root),))
        assert r.status == 1
        assert "src-app/Cargo.toml：文件不存在" in r.output

    def test_unreadable_version_field_is_red(self, tmp_path: Path) -> None:
        # version 字段读不出即红（不按空值放行）
        root = tmp_path / "no-version"
        root.mkdir()
        make_fixture(
            root,
            {
                "src-app/nsh.json": json.dumps(
                    {"name": "Ledger", "id": "com.zhangheng.ledger.nsh"},
                    indent=2,
                ),
            },
        )
        r = run((str(root),))
        assert r.status == 1
        assert "src-app/nsh.json：读不出顶层 version 字段" in r.output
