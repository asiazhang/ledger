# 被测对象是仓库工具脚本 scripts/test_exec.py（测试执行器与两入口覆盖守门，
# issue #1112）。脚本以 uv 运行时执行（ADR-0083 修订注记 / issue #2174）：
# run_gate_script 以 spawn 'uv run' 与门槛调用同款拉起，测的就是门槛路径。按测试
# 决策只测外部可观察结果——进程退出码与输出，不测内部函数；夹具经 `--root` 指向
# 临时工作区（只需 manifest + 目录形态，守门不调用 cargo，仓与 CI 上均零依赖）。
# 测试与所测脚本同目录住（#1158）。

import os
from dataclasses import dataclass
from pathlib import Path

from gate_script_test_helper import RunResult, gate_script, run_gate_script

SCRIPT = gate_script("test_exec.py")


def run(args: list[str]) -> RunResult:
    return run_gate_script(SCRIPT, args)


# 夹具工作区：根包（lib + workspace glob）+ 成员 alpha（lib + 集成测试 api/e2e）。
@dataclass
class FixtureOverrides:
    root_manifest: str | None = None
    alpha_manifest: str | None = None
    # test.sh 内容；缺省 = 合规双入口形态。
    test_sh: str | None = None
    # 追加的成员文件（相对夹具根，含目录）。
    files: dict[str, str] | None = None
    # 为 True 时删除两个 lib 目标（用于 --doc 登记失效夹具）。
    without_lib: bool = False


DEFAULT_TEST_SH = "\n".join(
    [
        "#!/bin/sh",
        "set -eu",
        "# 入口自检先行（真实 scripts/test.sh 同址接线）",
        "uv run scripts/test_exec.py check",
        "uv run scripts/test_exec.py",
        "( cd src-app && cargo test --workspace --test e2e )",
        "( cd src-app && cargo test --workspace --doc )",
        "",
    ]
)

DEFAULT_ROOT_MANIFEST = "\n".join(
    [
        "[package]",
        'name = "root-app"',
        'version = "0.0.0"',
        'edition = "2021"',
        "",
        "[workspace]",
        'members = ["crates/*"]',
        "",
    ]
)

DEFAULT_ALPHA_MANIFEST = "\n".join(
    [
        "[package]",
        'name = "alpha"',
        'version = "0.0.0"',
        'edition = "2021"',
        "",
        "[[test]]",
        'name = "e2e"',
        'path = "tests/e2e.rs"',
        "harness = false",
        "",
    ]
)


def make_fixture(tmp_path: Path, overrides: FixtureOverrides | None = None) -> str:
    o = overrides or FixtureOverrides()
    root = str(tmp_path)
    src_tauri = os.path.join(root, "src-app")
    alpha = os.path.join(src_tauri, "crates", "alpha")
    os.makedirs(os.path.join(alpha, "src"), exist_ok=True)
    os.makedirs(os.path.join(alpha, "tests"), exist_ok=True)
    os.makedirs(os.path.join(src_tauri, "src"), exist_ok=True)
    os.makedirs(os.path.join(root, "scripts"), exist_ok=True)

    Path(src_tauri, "Cargo.toml").write_text(
        o.root_manifest if o.root_manifest is not None else DEFAULT_ROOT_MANIFEST,
        encoding="utf-8",
    )
    if not o.without_lib:
        Path(src_tauri, "src", "lib.rs").write_text("pub fn boot() {}\n", encoding="utf-8")
        Path(alpha, "src", "lib.rs").write_text("pub fn alpha() {}\n", encoding="utf-8")
    Path(alpha, "Cargo.toml").write_text(
        o.alpha_manifest if o.alpha_manifest is not None else DEFAULT_ALPHA_MANIFEST,
        encoding="utf-8",
    )
    Path(alpha, "tests", "e2e.rs").write_text("fn main() {}\n", encoding="utf-8")
    Path(alpha, "tests", "api.rs").write_text("#[test]\nfn api() {}\n", encoding="utf-8")
    Path(root, "scripts", "test.sh").write_text(
        o.test_sh if o.test_sh is not None else DEFAULT_TEST_SH, encoding="utf-8"
    )
    for rel, content in (o.files or {}).items():
        abs_path = os.path.join(root, rel)
        os.makedirs(os.path.dirname(abs_path), exist_ok=True)
        Path(abs_path).write_text(content, encoding="utf-8")
    return root


# 带 test = false bin 的根 manifest（bin 零单测，源文件由 files 覆写提供）。
ROOT_WITH_TEST_FALSE_BIN = "\n".join(
    [
        "[package]",
        'name = "root-app"',
        'version = "0.0.0"',
        'edition = "2021"',
        "",
        "[workspace]",
        'members = ["crates/*"]',
        "",
        "[[bin]]",
        'name = "root-app"',
        'path = "src/main.rs"',
        "test = false",
        "",
    ]
)

# 豁免登记匹配用根 manifest：包名 = nsh-app（EXEMPT_BINS 的 package 名，#2082 迁居后）。
ROOT_NAMED_NSH_APP = "\n".join(
    [
        "[package]",
        'name = "nsh-app"',
        'version = "0.0.0"',
        'edition = "2021"',
        "",
        "[workspace]",
        'members = ["crates/*"]',
        "",
    ]
)


class Test测试执行器两入口覆盖守门:
    def test_compliant_fixture_targets_equal_entry_union(self, tmp_path: Path) -> None:
        r = run(["check", "--root", make_fixture(tmp_path)])
        assert r.status == 0
        # 6 = 根包 lib + doc、alpha lib + doc、alpha 集成 api + e2e；e2e 归非并发入口。
        assert "目标 6 个" in r.output
        assert "并发入口 3" in r.output
        assert "集成测试 1" in r.output
        assert "e2e" in r.output

    def test_same_package_lib_and_test_name_no_key_collision(self, tmp_path: Path) -> None:
        # cargo 允许同包内 lib 与集成 target 同名；只用「包::名」会把两者合成一个，
        # 静态清单与 run 交叉核对双双假绿、实际少跑一个二进制。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        files={
                            "src-app/crates/alpha/tests/alpha.rs": "#[test]\nfn same_name() {}\n"
                        }
                    ),
                ),
            ]
        )
        assert r.status == 0
        # 7 = 根包 lib + doc、alpha lib + doc、alpha 集成 api / alpha / e2e。
        assert "目标 7 个" in r.output
        assert "lib 单测 2" in r.output
        assert "集成测试 2" in r.output

    def test_new_harness_false_target_unregistered_red(self, tmp_path: Path) -> None:
        # 新增 harness=false 测试目标未登记非并发入口 → 红（目标静默漏跑即红）。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        alpha_manifest="\n".join(
                            [
                                "[package]",
                                'name = "alpha"',
                                'version = "0.0.0"',
                                'edition = "2021"',
                                "",
                                "[[test]]",
                                'name = "e2e"',
                                'path = "tests/e2e.rs"',
                                "harness = false",
                                "",
                                "[[test]]",
                                'name = "custom"',
                                'path = "tests/custom.rs"',
                                "harness = false",
                                "",
                            ]
                        ),
                        files={"src-app/crates/alpha/tests/custom.rs": "fn main() {}\n"},
                    ),
                ),
            ]
        )
        assert r.status == 1
        assert "alpha::custom" in r.output
        assert "静默漏跑" in r.output

    def test_delegated_e2e_removed_red(self, tmp_path: Path) -> None:
        # 非并发入口未登记 e2e（删 --test e2e）→ 红。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        test_sh="\n".join(
                            [
                                "#!/bin/sh",
                                "set -eu",
                                "uv run scripts/test_exec.py",
                                "( cd src-app && cargo test --workspace --doc )",
                                "",
                            ]
                        )
                    ),
                ),
            ]
        )
        assert r.status == 1
        assert "alpha::e2e" in r.output
        assert "--test e2e" in r.output

    def test_delegated_non_harness_target_registered_red(self, tmp_path: Path) -> None:
        # 非并发入口登记了非自定义 harness 目标（--test api 漂移）→ 红。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        test_sh="\n".join(
                            [
                                "#!/bin/sh",
                                "set -eu",
                                "uv run scripts/test_exec.py",
                                "( cd src-app && cargo test --workspace --test e2e )",
                                "( cd src-app && cargo test --workspace --test api )",
                                "( cd src-app && cargo test --workspace --doc )",
                                "",
                            ]
                        )
                    ),
                ),
            ]
        )
        assert r.status == 1
        assert "--test api" in r.output
        assert "不对应任何 harness = false 目标" in r.output

    def test_missing_doc_entry_red(self, tmp_path: Path) -> None:
        # 缺 cargo test --doc → 红（doc-test 静默漏跑即红）。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        test_sh="\n".join(
                            [
                                "#!/bin/sh",
                                "set -eu",
                                "uv run scripts/test_exec.py",
                                "( cd src-app && cargo test --workspace --test e2e )",
                                "",
                            ]
                        )
                    ),
                ),
            ]
        )
        assert r.status == 1
        assert "doc-test" in r.output

    def test_doc_entry_without_workspace_scope_red(self, tmp_path: Path) -> None:
        # doc 入口退化成 `cargo test --doc`（缺 workspace 范围）→ 红。
        # 只查 `--doc` 存在不够：非虚拟 workspace 下 `cargo test --doc` 只跑根包的
        # doctest，成员 crate 的 doc-test 静默漏跑而守门仍绿（issue #1112 审查发现）。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        test_sh="\n".join(
                            [
                                "#!/bin/sh",
                                "set -eu",
                                "uv run scripts/test_exec.py",
                                "( cd src-app && cargo test --workspace --test e2e )",
                                "( cd src-app && cargo test --doc )",
                                "",
                            ]
                        )
                    ),
                ),
            ]
        )
        assert r.status == 1
        assert "缺 workspace 范围" in r.output
        assert "doc-test 静默漏跑" in r.output

    def test_runtime_cargo_env_read_red(self, tmp_path: Path) -> None:
        # 执行器等价性：测试运行期读 cargo 注入环境变量 → 红（fail loud）。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        files={
                            "src-app/src/reads_manifest.rs":
                                'pub fn root() -> String { std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default() }\n'
                        }
                    ),
                ),
            ]
        )
        assert r.status == 1
        assert "执行器等价性" in r.output
        assert "CARGO_MANIFEST_DIR" in r.output

    def test_runtime_cargo_env_comment_mention_not_hit(self, tmp_path: Path) -> None:
        # 执行器等价性：注释里提到变量名不算命中（注释掩码，不假红）。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        files={
                            "src-app/src/notes.rs":
                                '// 约定：测试运行期不得写 std::env::var("CARGO_MANIFEST_DIR")\n'
                                '/// 同上：env::var("OUT_DIR")\n'
                                "pub fn ok() {}\n"
                        }
                    ),
                ),
            ]
        )
        assert r.status == 0

    def test_doc_registered_without_doc_targets_red(self, tmp_path: Path) -> None:
        # 工作区无 doc-test 目标却在非并发入口登记 --doc → 红（登记失效）。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        without_lib=True,
                        alpha_manifest="\n".join(
                            [
                                "[package]",
                                'name = "alpha"',
                                'version = "0.0.0"',
                                'edition = "2021"',
                                "",
                                "[[test]]",
                                'name = "e2e"',
                                'path = "tests/e2e.rs"',
                                "harness = false",
                                "",
                            ]
                        ),
                    ),
                ),
            ]
        )
        assert r.status == 1
        assert "无 doc-test 目标" in r.output

    def test_parallel_entry_call_removed_red(self, tmp_path: Path) -> None:
        # scripts/test.sh 删掉并发入口调用 → 红（接线删除即红）。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        test_sh="\n".join(
                            [
                                "#!/bin/sh",
                                "set -eu",
                                "( cd src-app && cargo test --workspace --test e2e )",
                                "( cd src-app && cargo test --workspace --doc )",
                                "",
                            ]
                        )
                    ),
                ),
            ]
        )
        assert r.status == 1
        assert "未调用并发入口" in r.output

    def test_all_entries_echoed_red(self, tmp_path: Path) -> None:
        # scripts/test.sh 三条入口全被 echo 包成说明文字 → 红（说明文字不算命令位置）。
        # 相对固定点的强度削弱（#1112 第三轮审查 P1）：旧判定只看「非注释行含
        # scripts/test_exec.py 子串」与「按空白切词找 --test / --doc」，把三条真命令
        # 全包成 echo "…" 后守门仍绿——而 `./scripts/test.sh` 一条测试都没跑，却因
        # 三条 echo 全成功而退出 0。判据必须落在命令位置。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        test_sh="\n".join(
                            [
                                "#!/bin/sh",
                                "set -eu",
                                "uv run scripts/test_exec.py check",
                                'echo "uv run scripts/test_exec.py"',
                                'echo "( cd src-app && cargo test --workspace --test e2e )"',
                                'echo "( cd src-app && cargo test --workspace --doc )"',
                                "",
                            ]
                        )
                    ),
                ),
            ]
        )
        assert r.status == 1
        assert "静默漏跑" in r.output
        assert "未调用并发入口" in r.output

    def test_check_only_line_and_echoed_run_line_red(self, tmp_path: Path) -> None:
        # scripts/test.sh 只留自检行 `… check`、运行行被 echo → 红（自检行不算运行接线）。
        # 入口自检行与运行行都调 scripts/test_exec.py：若按子串/词序判「已调用并发入口」，
        # 删掉运行行只留 `check` 也能假绿，本轮把两者分开（run 才算运行接线）。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        test_sh="\n".join(
                            [
                                "#!/bin/sh",
                                "set -eu",
                                "uv run scripts/test_exec.py check",
                                'echo "uv run scripts/test_exec.py"',
                                "( cd src-app && cargo test --workspace --test e2e )",
                                "( cd src-app && cargo test --workspace --doc )",
                                "",
                            ]
                        )
                    ),
                ),
            ]
        )
        assert r.status == 1
        assert "未调用并发入口" in r.output

    def test_legal_prefix_forms_still_wired(self, tmp_path: Path) -> None:
        # scripts/test.sh 合法前缀写法（command / if / VAR=x）仍算运行接线 → 不假红。
        for parallel_line in [
            "command uv run scripts/test_exec.py",
            "if uv run scripts/test_exec.py; then :; fi",
            "LEDGER_JOBS=4 uv run scripts/test_exec.py",
        ]:
            r = run(
                [
                    "check",
                    "--root",
                    make_fixture(
                        tmp_path,
                        FixtureOverrides(
                            test_sh="\n".join(
                                [
                                    "#!/bin/sh",
                                    "set -eu",
                                    parallel_line,
                                    "( cd src-app && cargo test --workspace --test e2e )",
                                    "( cd src-app && cargo test --workspace --doc )",
                                    "",
                                ]
                            )
                        ),
                    ),
                ]
            )
            assert r.status == 0, parallel_line

    def test_doc_entry_all_alias_not_false_red(self, tmp_path: Path) -> None:
        # doc 入口用 `--all` 别名（`cargo test --all --doc`）→ 不假红。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        test_sh="\n".join(
                            [
                                "#!/bin/sh",
                                "set -eu",
                                "uv run scripts/test_exec.py",
                                "( cd src-app && cargo test --workspace --test e2e )",
                                "( cd src-app && cargo test --all --doc )",
                                "",
                            ]
                        )
                    ),
                ),
            ]
        )
        assert r.status == 0

    def test_auto_keys_rejected_red(self, tmp_path: Path) -> None:
        # manifest 出现 auto* 开关（发现面被改写）→ 红（拒绝猜测）。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        alpha_manifest="\n".join(
                            [
                                "[package]",
                                'name = "alpha"',
                                'version = "0.0.0"',
                                'edition = "2021"',
                                "autotests = false",
                                "",
                                "[[test]]",
                                'name = "e2e"',
                                'path = "tests/e2e.rs"',
                                "harness = false",
                                "",
                            ]
                        )
                    ),
                ),
            ]
        )
        assert r.status == 1
        assert "autotests" in r.output

    def test_unsupported_members_glob_red(self, tmp_path: Path) -> None:
        # members 出现未支持的 glob 形态 → 红。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        root_manifest="\n".join(
                            [
                                "[package]",
                                'name = "root-app"',
                                'version = "0.0.0"',
                                'edition = "2021"',
                                "",
                                "[workspace]",
                                'members = ["crates/**"]',
                                "",
                            ]
                        )
                    ),
                ),
            ]
        )
        assert r.status == 1
        assert "未支持的 members glob" in r.output

    def test_test_false_bin_without_tests_green(self, tmp_path: Path) -> None:
        # bin test = false 且源码零单测 → 绿（bin 不入目标清单）。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        root_manifest=ROOT_WITH_TEST_FALSE_BIN,
                        files={"src-app/src/main.rs": "fn main() {}\n"},
                    ),
                ),
            ]
        )
        assert r.status == 0
        # bin 不产生测试目标：目标数与无 bin 夹具一致（6），bin 单测计数 0。
        assert "目标 6 个" in r.output
        assert "bin 单测 0" in r.output

    def test_test_false_bin_with_test_attr_red(self, tmp_path: Path) -> None:
        # bin test = false 但源码新增 #[test] → 红（静默漏跑即红）。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        root_manifest=ROOT_WITH_TEST_FALSE_BIN,
                        files={
                            "src-app/src/main.rs": "#[test]\nfn stray() {}\n\nfn main() {}\n"
                        },
                    ),
                ),
            ]
        )
        assert r.status == 1
        assert "test = false" in r.output
        assert "静默漏跑" in r.output

    def test_test_false_bin_comment_mention_not_red(self, tmp_path: Path) -> None:
        # bin test = false 源码注释里提到 #[test] → 不假红（注释掩码）。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        root_manifest=ROOT_WITH_TEST_FALSE_BIN,
                        files={
                            "src-app/src/main.rs": "// 不要在 bin 里加 #[test] 或 #[cfg(test)]\nfn main() {}\n"
                        },
                    ),
                ),
            ]
        )
        assert r.status == 0

    def test_bin_with_tests_unregistered_red(self, tmp_path: Path) -> None:
        # bin 带单测且未登记豁免 → 红（bin 单测不入并发入口）。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        files={"src-app/src/main.rs": "#[test]\nfn unit() {}\n\nfn main() {}\n"}
                    ),
                ),
            ]
        )
        assert r.status == 1
        assert "bin 单测未登记豁免" in r.output
        assert "root-app::root-app" in r.output

    def test_exempt_registration_hit(self, tmp_path: Path) -> None:
        # 豁免登记命中（nsh-app::ledger-perf）→ 出并发入口，summary 标注承接入口。
        r = run(
            [
                "check",
                "--root",
                make_fixture(
                    tmp_path,
                    FixtureOverrides(
                        root_manifest=ROOT_NAMED_NSH_APP,
                        files={
                            # #2082 迁居形态：ledger-perf 为 nsh-app 同包 bin（成员 crate 自带
                            # manifest，bin 源文件经 src/bin/<name>/main.rs 自动发现）。
                            "src-app/crates/nsh/Cargo.toml":
                                '[package]\nname = "nsh-app"\nversion = "0.0.0"\nedition = "2021"\n',
                            "src-app/crates/nsh/src/bin/ledger-perf/main.rs":
                                "#[test]\nfn gen() {}\n\nfn main() {}\n",
                        },
                    ),
                ),
            ]
        )
        assert r.status == 0
        assert "bin 单测豁免 1 个" in r.output
        assert "ledger-perf → perf-bench.yml 每日基准" in r.output
        assert "bin 单测 0" in r.output

    def test_dead_registration_after_bin_removed_not_red(self, tmp_path: Path) -> None:
        # bin 删除后残留的死登记 → 不红（无副作用，summary 不再展示）。
        # 守门方向是「发现的 test = true bin 都必须登记」：bin 删除后登记无匹配对象，
        # 不拖累任何工作区（含夹具）；删登记才红（bin 回到并发入口被未登记检查拦住）。
        r = run(
            [
                "check",
                "--root",
                make_fixture(tmp_path, FixtureOverrides(root_manifest=ROOT_NAMED_NSH_APP)),
            ]
        )
        assert r.status == 0
        assert "bin 单测豁免" not in r.output


class Test覆盖守门真实仓库绿基线:
    # 观察面是 `uv run scripts/test_exec.py check` 的**退出码与输出**（ADR-0087 断言
    # 强度）——守门读真实仓库的 scripts/test.sh（双入口 + 自检）与运行期环境扫描，
    # 任一处漂移都让这里变红。各删除形态的「制造变红」另由上面的夹具用例逐条覆盖
    # （删并发入口、删/漂移 --test、删 --doc、doc 缺 workspace 范围、运行期读 cargo
    # 注入环境）；check.sh / CI 的挂载接线核对归守门接线测试
    #（scripts/test_gate_mounts.py，登记住 scripts/gate_mounts.py 守门挂载登记，
    # issue #1682）——原守门规则④已退役并入该登记。

    def test_real_repo_both_entries_and_equivalence_gate_present(self) -> None:
        # 真实仓库：两条入口 + 等价性守门齐备 → 通过。
        r = run(["check"])
        assert r.status == 0
        assert "测试执行覆盖守门" in r.output
