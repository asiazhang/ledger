# 被测对象是仓库工具脚本 scripts/check_eastmoney_residue.py（东财行情面端点与
# 常量零残留守门，issue #1572 / ADR-0130 守门判据①——源码扫描，删除任一遗漏即红）。
# 脚本以 uv 运行时执行（ADR-0083 修订注记，#2174）：run_gate_script 以
# subprocess('uv run') 与门槛调用同款拉起，测的就是门槛路径。
# 按测试决策只测外部可观察结果——进程退出码与输出，通过位置参数把扫描目标指向
# 临时夹具目录（形状同构仓库根：scripts/ 守门本体在场 + 登记例外的 FX 腿文件）。

from __future__ import annotations

import os
from pathlib import Path

import pytest

from gate_script_test_helper import RunResult, gate_script, run_gate_script

SCRIPT = gate_script("check_eastmoney_residue.py")


def run(args: tuple[str, ...] = ()) -> RunResult:
    return run_gate_script(SCRIPT, args)


# 登记例外的 FX 腿夹具已随 #1551 退役（REGISTERED_EXCEPTIONS 清空）：
# 原东财 push2his 主机池现在出现在 http.rs 即未登记残留，直接红（收紧后的判据）。
RETIRED_FX_HOST_POOL = "\n".join(
    [
        "// 测试夹具：已退役的东财 FX 日 K 主机池（#1551 换 ECB）",
        "pub const FX_KLINE_HOSTS: &[&str] = &[",
        '    "https://push2his.eastmoney.com",',
        "];",
    ]
)

# 豁免自证与例外登记的默认文件（值为 None 表示刻意省略——负向样本用）
DEFAULT_FILES: dict[str, str | None] = {
    "scripts/check_eastmoney_residue.py": "# 守门本体（豁免自证对象）",
    "scripts/test_check_eastmoney_residue.py": "# 包装测试（豁免自证对象）",
    "src/lib.rs": "pub const ok: i32 = 1;",
}


# 建夹具目录（形状同构仓库根）；值为 None 表示刻意省略该默认文件
def make_fixture(root: Path, files: dict[str, str | None] | None = None) -> str:
    merged: dict[str, str | None] = {**DEFAULT_FILES, **(files or {})}
    for name, content in merged.items():
        if content is None:
            continue
        abs_path = os.path.join(str(root), name)
        os.makedirs(os.path.dirname(abs_path), exist_ok=True)
        Path(abs_path).write_text(content, encoding="utf-8")
    return str(root)


class TestCheckEastmoneyResidue东财行情面零残留守门:
    def test_default_repo_scan_all_green_with_empty_exception_list(self) -> None:
        # 默认扫描本仓（门槛路径）：行情面零残留全绿，已登记例外清单为空
        r = run()
        assert r.status == 0
        assert "东财行情面零残留守门通过" in r.output
        # 零例外稳态（#1551 退役 FX 腿后删除登记条目，同 check-infra-dml 自 #1108 起）
        assert "已登记例外 0 条" in r.output

    def test_unregistered_file_with_eastmoney_domain_is_red(self, tmp_path: Path) -> None:
        # 负向判据：未登记文件出现东财端点域名即红并定位到 文件:行
        dir_ = tmp_path / "rogue-fixture"
        dir_.mkdir()
        make_fixture(
            dir_,
            {
                "src-app/crates/market-sync/src/rogue.rs": "\n".join(
                    [
                        "// 测试夹具：重新引入东财通道（应被守门拦下）",
                        'pub const HOSTS: &[&str] = &["https://push2.eastmoney.com"];',
                        "const REFERER = 'https://fund.eastmoney.com/';",
                    ]
                ),
            },
        )
        r = run((str(dir_),))
        assert r.status == 1
        assert "src-app/crates/market-sync/src/rogue.rs:2" in r.output
        assert "eastmoney.com" in r.output
        assert "ADR-0130" in r.output

    def test_dfcfw_domain_in_banned_marker_set(self, tmp_path: Path) -> None:
        # dfcfw 域名同在禁令闭集：东财自有域任一出现即红
        # 扫描面扩展闭集随 Web 前端退役收窄（#2171/#2172）：夹具改用 .rs 面（原
        # .ts 夹具不再入扫描面，语义不变——任意文本面出现东财域即红）。
        dir_ = tmp_path / "dfcfw-fixture"
        dir_.mkdir()
        make_fixture(
            dir_,
            {"src-app/crates/market-sync/src/rogue2.rs": "// https://dfcfw.eastmoney.com/x.png"},
        )
        r = run((str(dir_),))
        assert r.status == 1
        assert "src-app/crates/market-sync/src/rogue2.rs:1" in r.output

    def test_retired_fx_host_pool_file_is_unregistered_residue_red(self, tmp_path: Path) -> None:
        # 收紧后的判据：原 FX 腿主机池文件（http.rs）再出现东财域名即未登记残留红
        dir_ = tmp_path / "retired-fx-fixture"
        dir_.mkdir()
        make_fixture(dir_, {"src-app/crates/market-sync/src/http.rs": RETIRED_FX_HOST_POOL})
        r = run((str(dir_),))
        assert r.status == 1
        assert "src-app/crates/market-sync/src/http.rs:3" in r.output
        assert "eastmoney.com" in r.output
        assert "ADR-0130" in r.output

    def test_missing_self_or_wrapper_test_is_red(self, tmp_path: Path) -> None:
        # 豁免自证：守门本体或包装测试缺失即红，不放任守门空转
        no_script = tmp_path / "no-script-fixture"
        no_script.mkdir()
        make_fixture(no_script, {"scripts/check_eastmoney_residue.py": None})
        no_script_run = run((str(no_script),))
        assert no_script_run.status == 1
        assert "豁免清单漂移" in no_script_run.output

        no_test = tmp_path / "no-test-fixture"
        no_test.mkdir()
        make_fixture(no_test, {"scripts/test_check_eastmoney_residue.py": None})
        assert run((str(no_test),)).status == 1

    def test_docs_historical_narrative_exempt(self, tmp_path: Path) -> None:
        # docs/ 历史叙述豁免：档案内的端点 URL 引证不计数
        dir_ = tmp_path / "docs-fixture"
        dir_.mkdir()
        make_fixture(
            dir_,
            {
                "docs/research/note.md": "历史档案：https://push2his.eastmoney.com 已于 2026-09 实测失效。",
            },
        )
        r = run((str(dir_),))
        assert r.status == 0
        assert "东财行情面零残留守门通过" in r.output
