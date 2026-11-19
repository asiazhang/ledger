from pathlib import Path
import re

from check_bare_font_size import REGISTERED_EXCEPTIONS
from gate_script_test_helper import RunResult, gate_script, run_gate_script

# 被测对象是仓库工具脚本 scripts/check_bare_font_size.py（壳根包裸字号直调零残留
# 守门，issue #2108 / spec #2099 / ADR-0147——源码扫描，删除扫描规则即存量违规不再
# 被拦）。脚本以 uv 运行时执行（ADR-0083 修订注记）：run_gate_script 以 spawn
# 'uv run' 与门槛调用同款拉起，测的就是门槛路径。
# 按测试决策只测外部可观察结果——进程退出码与输出，通过位置参数把扫描根指向
# 临时夹具目录（形状同构 src-app/src：夹具根直接放 .rs 模块文件）。
script = gate_script("check_bare_font_size.py")


def run(args: tuple[str, ...] = ()) -> RunResult:
    return run_gate_script(script, args)


# 违规直调样例：px() 参数是数字字面量（裸值）——要抓的回归
BARE_LINE = '                .child(text("复制").text_size(px(12.0)).text_color(accent)),'


def make_fixture(tmp_path: Path, files: dict[str, str]) -> str:
    """建夹具目录（形状同构扫描根：.rs 模块平铺/嵌套均可）"""
    for name, content in files.items():
        path = tmp_path / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
    return str(tmp_path)


class Test壳根包裸字号直调零残留守门:
    """check-bare-font-size（壳根包裸字号直调零残留守门，issue #2108）"""

    def test_default_scan_real_repo_green(self) -> None:
        # 默认扫描本仓（门槛路径）：src-app/src 零裸字号直调全绿，例外清单为空
        r = run()
        assert r.status == 0
        assert "壳根包裸字号直调零残留守门通过" in r.output
        # 扫描面非空自证（拒绝空集假绿：扫描根漂移成空面必须红，不能静默绿）
        assert re.search(r"扫描 [1-9][0-9]* 个文件", r.output) is not None
        # 零例外稳态（#2108 交付时点：批次票 #2101–#2107 已把 643 处存量清零）
        assert "已登记例外 0 条" in r.output

    def test_negative_pattern_hits_and_locates_file_line(self, tmp_path: Path) -> None:
        # 负向判据：text_size(px(<数字字面量>)) 直调即红并定位到 文件:行（含 `.5` 无整数位形态）
        dir_path = make_fixture(
            tmp_path,
            {
                "rogue_view.rs": "\n".join(
                    [
                        "use nsh_core::prelude::*;",
                        "pub fn cell() -> impl IntoElement {",
                        '    text("金额")',
                        BARE_LINE,
                        '        .child(text("隙").text_size(px(.5)))',
                        "}",
                    ]
                ),
            },
        )
        r = run((dir_path,))
        assert r.status == 1
        assert "rogue_view.rs:4" in r.output
        assert "rogue_view.rs:5" in r.output
        assert "text_size(px(12.0)" in r.output
        assert "px(.5)" in r.output
        assert "font_size" in r.output

    def test_compliant_forms_pass(self, tmp_path: Path) -> None:
        # 合规形态过门：px(常量引用) 与 typed 预设（text_xs 等）不触发
        dir_path = make_fixture(
            tmp_path,
            {
                "compliant_view.rs": "\n".join(
                    [
                        "use crate::design;",
                        "pub fn cell() -> impl IntoElement {",
                        '    text("金额").text_size(px(design::font_size::CAPTION)).text_xs()',
                        "}",
                    ]
                ),
            },
        )
        r = run((dir_path,))
        assert r.status == 0
        assert "壳根包裸字号直调零残留守门通过" in r.output

    def test_lexical_masking_comments_and_strings(self, tmp_path: Path) -> None:
        # 词法豁免：注释与字符串字面量里的形态不计数（设计文档引证、样例文本合法在场）
        dir_path = make_fixture(
            tmp_path,
            {
                "docs_and_strings.rs": "\n".join(
                    [
                        "//! 收口先例：把裸 `text_size(px(..))` 直调换到字阶常量（#2108）。",
                        'pub const SAMPLE: &str = "text_size(px(12.0)) 只是展示文本，不是调用";',
                    ]
                ),
            },
        )
        r = run((dir_path,))
        assert r.status == 0
        assert "壳根包裸字号直调零残留守门通过" in r.output

    def test_registered_exceptions_empty_at_delivery(self) -> None:
        # 例外登记严格相等校验在册：交付时点为零例外稳态（存量已随批次票清零）
        # 例外机制语义（与 check-eastmoney-residue / check-infra-dml 同款）：实际命中
        # ≠ 登记数即红、例外清零也红（条目应删除）、登记文件消失红。交付时点清单为
        # 空——真实仓任何命中都按未登记残留红（负向判据用例覆盖）；出现新的合法在
        # 场时在此登记并同步补「白名单生效→绿」端到端用例。
        assert REGISTERED_EXCEPTIONS == []

    def test_missing_root_and_empty_scan_face_fail(self, tmp_path: Path) -> None:
        # 扫描根不存在即红；扫描根无 .rs 文件（空面）即红，拒绝静默空转
        r = run((str(tmp_path / "check-bare-font-size-nonexistent-xyz"),))
        assert r.status == 1
        assert "扫描根不存在" in r.output

        empty = make_fixture(tmp_path, {"README.txt": "非 Rust 面，不进扫描"})
        empty_run = run((empty,))
        assert empty_run.status == 1
        assert "扫描面为空" in empty_run.output
