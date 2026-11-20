import json
import os
import re
from pathlib import Path

from gate_script_test_helper import RunResult, gate_script, run_gate_script

# 被测对象是仓库工具脚本 scripts/check_i18n_keys.py（i18n key 全等校验门槛 +
# 码化错误模板覆盖守门，issue #1188 / ADR-0050）。
# 脚本以 uv 运行时执行（ADR-0083 修订注记）：run_gate_script 以 spawn 'uv run' 与
# 门槛调用同款拉起，测的就是门槛路径。
# 按测试决策只测外部可观察结果——进程退出码与输出，不测内部函数；
# 通过位置参数把扫描目标指向临时夹具目录（仿 check-commands.test.ts 先例）。
# 注意：目录名即域前缀（common.json 内层不再重复域名）。
# 夹具布局：<root>/locales/{zh-CN,en-US}（固定含空 errors.json，码化覆盖守门的比对对象）
# + <root>/rust（Rust 扫描根，默认无码化构造点）。
script = gate_script("check_i18n_keys.py")


def make_locale_dir(root: Path, name: str, files: dict[str, object]) -> None:
    dir_path = root / name
    dir_path.mkdir(parents=True)
    for file, content in files.items():
        (dir_path / file).write_text(json.dumps(content), encoding="utf-8")


def make_fixture(
    tmp_path: Path,
    zh_files: dict[str, object],
    en_files: dict[str, object],
    rust_src: str | None = None,
    zh_errors: object | None = None,
    en_errors: object | None = None,
) -> str:
    root = tmp_path
    locales = root / "locales"
    make_locale_dir(
        locales,
        "zh-CN",
        {"errors.json": zh_errors if zh_errors is not None else {}, **zh_files},
    )
    make_locale_dir(
        locales,
        "en-US",
        {"errors.json": en_errors if en_errors is not None else {}, **en_files},
    )
    rust = root / "rust"
    rust.mkdir(parents=True)
    (rust / "lib.rs").write_text(
        rust_src if rust_src is not None else "// 无码化构造点\n", encoding="utf-8"
    )
    return str(root)


def run(root: str) -> RunResult:
    return run_gate_script(
        script, [os.path.join(root, "locales"), os.path.join(root, "rust")]
    )


class TestI18nKey全等校验:
    """i18n key 全等校验（check.sh 质量门槛）"""

    def test_real_repo_default_pass(self) -> None:
        # 真实仓库默认通过：两语言 key 全等且码化错误模板全覆盖（现行仓库绿）
        # 无位置参数 = 真实仓库默认路径（locales 住仓库根 locales/ + src-app 扫描根）。
        # 随守门测试挂载进 CI（issue #1473 引入挂载；#1991 起运行口径 = scripts/
        # 测试 runner）：真实仓违规即红，不再只靠本地
        # 自觉跑 check.sh。两条成功行都携带动态计数/规则名——对应规则被删除时该行
        # 消失、本用例随之变红，绿基线不是空转（「删除规则 → 真实仓用例红」的实证载体）。
        r = run_gate_script(script)
        assert r.status == 0
        assert "i18n key 全等" in r.output
        assert re.search(r"码化错误模板覆盖：[1-9]\d* 个码化错误码", r.output) is not None

    def test_equal_key_sets_pass(self, tmp_path: Path) -> None:
        # 两语言 key 集合全等时通过
        dir_path = make_fixture(
            tmp_path,
            {"common.json": {"save": "保存", "nested": {"ok": "确定"}}, "tx.json": {"title": "交易"}},
            {
                "common.json": {"save": "Save", "nested": {"ok": "OK"}},
                "tx.json": {"title": "Transactions"},
            },
        )
        r = run(dir_path)
        assert r.status == 0
        assert "全等" in r.output

    def test_source_only_key_fails_and_lists_missing(self, tmp_path: Path) -> None:
        # 源语言独有 key（漏翻）→ 失败并列出缺失项
        dir_path = make_fixture(
            tmp_path,
            {"common.json": {"save": "保存", "cancel": "取消"}},
            {"common.json": {"save": "Save"}},
        )
        r = run(dir_path)
        assert r.status == 1
        assert "common.cancel" in r.output

    def test_other_only_key_fails_and_lists_extra(self, tmp_path: Path) -> None:
        # 其他 locale 独有 key（多余）→ 失败并列出多余项
        dir_path = make_fixture(
            tmp_path,
            {"common.json": {"save": "保存"}},
            {"common.json": {"save": "Save", "extra": "Extra"}},
        )
        r = run(dir_path)
        assert r.status == 1
        assert "common.extra" in r.output

    def test_multi_domain_multi_file_any_diff_fails(self, tmp_path: Path) -> None:
        # 多域多文件：任一域有差异即失败（双向都查）
        dir_path = make_fixture(
            tmp_path,
            {"common.json": {"ok": "确定"}, "tx.json": {"a": "甲", "b": "乙"}},
            {"common.json": {"ok": "OK", "zh": "多余"}, "tx.json": {"a": "A"}},
        )
        r = run(dir_path)
        assert r.status == 1
        assert "tx.b" in r.output
        assert "common.zh" in r.output


class Test码化错误模板覆盖守门:
    """码化错误模板覆盖守门（issue #1188 / ADR-0050）"""

    def test_coded_codes_covered_in_both_locales_pass(self, tmp_path: Path) -> None:
        # 码化构造码在两语言 errors.json 均有模板 → 通过
        dir_path = make_fixture(
            tmp_path,
            {"common.json": {"save": "保存"}},
            {"common.json": {"save": "Save"}},
            rust_src='fn f() { AppError::coded("fx.rate-missing", "缺少汇率"); }\n',
            zh_errors={"fx": {"rate-missing": "缺少汇率"}},
            en_errors={"fx": {"rate-missing": "exchange rate missing"}},
        )
        r = run(dir_path)
        assert r.status == 0
        assert "码化错误模板覆盖" in r.output

    def test_missing_template_fails_and_lists_code(self, tmp_path: Path) -> None:
        # 缺模板的码化构造码 → 失败并列出缺失码
        dir_path = make_fixture(
            tmp_path,
            {"common.json": {"save": "保存"}},
            {"common.json": {"save": "Save"}},
            rust_src='fn f() { AppError::codedp_not_found("trade.detail-not-found", "交易不存在"); }\n',
        )
        r = run(dir_path)
        assert r.status == 1
        assert "trade.detail-not-found" in r.output

    def test_en_only_missing_template_fails(self, tmp_path: Path) -> None:
        # 仅 en 缺模板 → 失败（双向校验）
        dir_path = make_fixture(
            tmp_path,
            {"common.json": {"save": "保存"}},
            {"common.json": {"save": "Save"}},
            rust_src='fn f() { AppError::coded("account.not-found", "账户不存在"); }\n',
            zh_errors={"account": {"not-found": "账户不存在: {0}"}},
        )
        r = run(dir_path)
        assert r.status == 1
        assert "account.not-found" in r.output

    def test_const_reference_call_site_resolved_to_value(self, tmp_path: Path) -> None:
        # const 常量引用型构造点解析为常量值参与校验
        dir_path = make_fixture(
            tmp_path,
            {"common.json": {"save": "保存"}},
            {"common.json": {"save": "Save"}},
            rust_src=(
                'const RATE_CODE: &str = "fx.rate-missing";\n'
                'fn f() { AppError::coded(RATE_CODE, "缺少汇率"); }\n'
            ),
        )
        r = run(dir_path)
        assert r.status == 1
        assert "fx.rate-missing" in r.output

    def test_comments_cfg_test_and_test_paths_excluded(self, tmp_path: Path) -> None:
        # 注释、#[cfg(test)] 块与测试路径内的构造点不入枚举
        dir_path = make_fixture(
            tmp_path,
            {"common.json": {"save": "保存"}},
            {"common.json": {"save": "Save"}},
            rust_src="\n".join(
                [
                    '// AppError::coded("comment.only", "注释不算")',
                    '/* AppError::coded("block.only", "块注释不算") */',
                    "#[cfg(test)]",
                    "mod tests {",
                    '  fn t() { AppError::coded("testmod.only", "测试不算"); }',
                    "}",
                    'fn real() { AppError::coded("covered.code", "正文算"); }',
                ]
            ),
            zh_errors={"covered": {"code": "有模板"}},
            en_errors={"covered": {"code": "covered"}},
        )
        rust_tests = Path(dir_path) / "rust" / "tests"
        rust_tests.mkdir(parents=True)
        (rust_tests / "steps.rs").write_text(
            'fn t() { AppError::coded("testfile.only", "测试文件不算"); }\n', encoding="utf-8"
        )
        r = run(dir_path)
        assert r.status == 0
        assert "码化错误模板覆盖" in r.output

    def test_nested_block_comment_masked(self, tmp_path: Path) -> None:
        # 注释掩码口径（issue #1741 收口）：嵌套块注释内的构造点不入枚举
        # 码化构造点提取腿自 #1741 起消费共享原语 mask_non_code(·, True)：块注释按
        # Rust 语义嵌套配对整体掩掉（弱形态剥离器在首个 */ 收尾，夹层里的注释体
        # 被误保留、其中的构造点误入枚举而假红）。
        dir_path = make_fixture(
            tmp_path,
            {"common.json": {"save": "保存"}},
            {"common.json": {"save": "Save"}},
            rust_src="\n".join(
                [
                    '/* outer /* inner */ AppError::coded("nested.only", "嵌套不算") */',
                    'fn real() { AppError::coded("covered.code", "正文算"); }',
                ]
            ),
            zh_errors={"covered": {"code": "有模板"}},
            en_errors={"covered": {"code": "covered"}},
        )
        r = run(dir_path)
        assert r.status == 0
        assert "码化错误模板覆盖" in r.output
