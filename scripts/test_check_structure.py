import dataclasses
import os
import tempfile
from dataclasses import dataclass
from pathlib import Path

import pytest

from check_structure import (
    CONVERT_CURRENT_CALLERS,
    CRATE_LAYER,
    CRATE_MODULE_TARGETS,
    CRATES,
    INFRA_SRC_REL,
    TRANSACTION_ZONE_ALLOWED_EDGES,
    TRANSACTION_ZONE_BY_DIR,
)
from gate_script_test_helper import RunResult, gate_script, run_gate_script

# 被测对象是仓库工具脚本 scripts/check_structure.py（结构守门，ADR-0056）。
# 脚本以 uv 运行时执行（ADR-0083 修订注记（#2174））：run_gate_script 以
# spawn 'uv run' 与门槛调用同款拉起，测的就是门槛路径。
# 按测试决策守门行为只测外部可观察结果——进程退出码与输出，不测内部函数；
# 通过位置参数把扫描目标指向临时夹具目录。
# 原语级测试面（mask_non_code 语料 / line_at / walk_text_files）已迁
# scripts/test_gate_primitives.py（#1680 库归库、门归门）：库的正确性由库自己的
# 测试面证明，本文件只测结构守门的门级行为（spawn 外部可观察结果）。
# 夹具自足化（#1595 / T2-3）：每个 crate 的模块面由用例显式声明
# `{crate, dir, libRs, files}`——合成 lib.rs 文本 + 合成磁盘布局一次生成，
# 不再有「相对路径 → 目标 crate」的路由链，也不 import 已退役的手写模块清单。
SCRIPT = gate_script("check_structure.py")


def run(args: list[str]) -> RunResult:
    return run_gate_script(SCRIPT, args)


@dataclass
class CrateCase:
    """用例夹具的显式 crate 模块面声明（#1595 T2-3）：`libRs` 是 crate 根 `lib.rs` 的
    声明文本，`files` 是相对 crate `src` 的模块文件与内容——合成声明面与合成磁盘
    布局由此一次生成，测试不依赖守门脚本的清单常量。
    """

    # crate 名（与 CRATES 政策核成员一致；报文定位用）
    crate: str
    # crate 目录（相对 src-app）
    dir: str
    # crate 根 lib.rs 内容（mod 声明面；空串=无模块的声明面）
    lib_rs: str
    # 模块文件：相对 crate src 的路径 → 内容
    files: dict[str, str]


# 夹具桩内容（无壳层依赖的最小 Rust 文件）
STUB = "// 结构守门夹具桩\npub fn stub() {}\n"

# 当期入口闭集在册成员的夹具桩（含一笔生产调用，#1692）：骨架自足化——每个
# 在册成员文件都存在且恰有一处调用，骨架默认全绿。
CONVERT_CALL_STUB = (
    "// 当期入口调用方闭集夹具桩\n"
    "pub fn read_path_sum() -> i64 {\n"
    '    amount::convert_to_native_current(conn, 1, "CNY")\n'
    "}\n"
)

# infra crate 根 lib.rs 的 test_utils「放行测试」cfg 门（同真实仓库，ADR-0111 决策 5）。
INFRA_TEST_UTILS_GATE = '#[cfg(any(test, feature = "test-utils"))]\n#[doc(hidden)]\n'

# 基础设施 crate 的骨架形状（与真实仓库同形：db / error / test_utils 生产编译门）。
INFRA_CASE = CrateCase(
    crate="ledger-infra",
    dir="crates/infra",
    lib_rs=(
        "pub mod boot;\npub mod db;\npub mod error;\n"
        + INFRA_TEST_UTILS_GATE
        + "pub mod test_utils;\n"
    ),
    files={
        "boot/mod.rs": STUB,
        "db/mod.rs": STUB,
        "error.rs": (
            "pub struct AppError;\n"
            '#[cfg(feature = "http")]\n'
            "impl axum::response::IntoResponse for AppError {\n"
            "    fn into_response(self) -> axum::response::Response {}\n"
            "}\n"
        ),
        "test_utils.rs": STUB,
    },
)

# 核心交易域 crate 的骨架形状（四区目录与区归属政策表一致，供区级层序用例消费）。
TRANSACTION_CASE = CrateCase(
    crate="ledger-transaction",
    dir="crates/transaction",
    lib_rs=(
        "pub mod amount;\npub mod command;\npub mod model;\npub mod read;\n"
        "pub mod seams;\npub mod shared;\npub mod write;\n"
    ),
    files={
        "amount/mod.rs": STUB,
        "command/mod.rs": STUB,
        "model/mod.rs": STUB,
        "read/mod.rs": STUB,
        "seams/mod.rs": STUB,
        "shared/mod.rs": "pub mod search_text;\n",
        "shared/search_text.rs": STUB,
        "write/mod.rs": STUB,
    },
)

# 同步协议 crate 的骨架形状（协议面模块，供协议分层用例消费）。
PROTOCOL_CASE = CrateCase(
    crate="ledger-sync-protocol",
    dir="crates/sync-protocol",
    lib_rs="pub mod op;\n",
    files={"op.rs": STUB},
)


# 骨架默认模块面：每 crate 一枚 core 模块；特定形状的 crate 用常量覆盖。
def default_case(crate: str, dir_: str) -> CrateCase:
    return CrateCase(crate=crate, dir=dir_, lib_rs="pub mod core;\n", files={"core.rs": STUB})


# 骨架里形状特殊的 crate（键 = crate 名；其余成员用 default_case 的默认形状）。
SPECIAL_CASES: dict[str, CrateCase] = {
    INFRA_CASE.crate: INFRA_CASE,
    TRANSACTION_CASE.crate: TRANSACTION_CASE,
    PROTOCOL_CASE.crate: PROTOCOL_CASE,
}

# 骨架成员 crate 的模块面（显式声明；模块名不必与真实仓库相同，机制面一致即可）。
SKELETON_CASES: list[CrateCase] = [
    SPECIAL_CASES.get(crate.name) or default_case(crate.name, crate.dir)
    for crate in CRATES
    if crate.dir != "."
]


# 账户域 crate 的模块面声明（域侧扫描用例的靶；`core.rs` 可被覆盖为违规内容）。
def accounts_case(files: dict[str, str] | None = None, lib_rs: str = "pub mod core;\n") -> CrateCase:
    return CrateCase(
        crate="ledger-accounts",
        dir="crates/accounts",
        lib_rs=lib_rs,
        files={"core.rs": STUB, **(files or {})},
    )


# 基础设施 crate 的模块面声明（在骨架形状上覆盖文件/声明面，供基础层用例消费）。
def infra_case(files: dict[str, str] | None = None, lib_rs: str | None = None) -> CrateCase:
    return CrateCase(
        crate=INFRA_CASE.crate,
        dir=INFRA_CASE.dir,
        lib_rs=lib_rs if lib_rs is not None else INFRA_CASE.lib_rs,
        files={**INFRA_CASE.files, **(files or {})},
    )


# 交易域 crate 的模块面声明（在骨架形状上覆盖文件，供区级层序用例消费）。
def transaction_case(files: dict[str, str] | None = None, lib_rs: str | None = None) -> CrateCase:
    return CrateCase(
        crate=TRANSACTION_CASE.crate,
        dir=TRANSACTION_CASE.dir,
        lib_rs=lib_rs if lib_rs is not None else TRANSACTION_CASE.lib_rs,
        files={**TRANSACTION_CASE.files, **(files or {})},
    )


# 同步协议 crate 的模块面声明（协议分层用例的靶）。
def protocol_case(files: dict[str, str] | None = None) -> CrateCase:
    return CrateCase(
        crate=PROTOCOL_CASE.crate,
        dir=PROTOCOL_CASE.dir,
        lib_rs=PROTOCOL_CASE.lib_rs,
        files={**PROTOCOL_CASE.files, **(files or {})},
    )


@dataclass
class CrateFixtureOverrides:
    """workspace 骨架夹具的可覆盖面（缺省为一份全绿的骨架）。"""

    root_manifest: str | None = None
    # 覆盖成员 crate 的 Cargo.toml：crate 目录（相对 src-app）→ 清单文本
    member_manifests: dict[str, str] | None = None
    # 覆盖 `crates/infra/src/lib.rs` 内容（test_utils cfg 门负向夹具）
    infra_lib_rs: str | None = None
    # 覆盖 `crates/infra/src/error.rs` 内容（http 投影 impl cfg 门负向夹具）
    infra_error_rs: str | None = None
    # 覆盖 `src/api_server/handlers/import.rs` 内容（投资五节锚点 cfg 门负向夹具，#1185）
    api_server_import_rs: str | None = None
    # 覆盖 `src/api_server/mod.rs` 内容（投资五节锚点再导出 cfg 门负向夹具，#1185）
    api_server_mod_rs: str | None = None
    # 覆盖根包 `[dependencies]` 的 ledger-infra 行（生产依赖接线负向夹具）
    root_infra_prod_dep: str | None = None
    # 追加到根包 `[features]` 段的原文行（default feature 负向夹具）
    root_features_extra: str | None = None
    check_sh: str | None = None
    test_sh: str | None = None
    lint_fix_sh: str | None = None
    # 覆盖 `scripts/test_exec.py`（cargo 命令宿主，workspace 范围负向夹具，#1112）
    test_exec_py: str | None = None
    workflow: str | None = None
    # 追加一个未登记的成员目录（crates/<name>）——新 crate 漏登记的负向夹具
    orphan_crate: str | None = None
    # 当期入口闭集：不写出的在册成员文件（缺文件负向夹具，#1692）
    omit_convert_callers: tuple[str, ...] | None = None
    # 当期入口闭集：在册成员文件内容覆写（零调用负向夹具，#1692）
    convert_caller_content: dict[str, str] | None = None
    # 覆盖用例声明的 crate 模块面（{crate, dir, libRs, files}，#1595 T2-3）
    module_cases: tuple[CrateCase, ...] | None = None


# 六件套 deny 声明（workspace 级唯一声明处，ADR-0060）
LINT_DENIES = "\n".join(
    [
        'unwrap_used = "deny"',
        'expect_used = "deny"',
        'panic = "deny"',
        'todo = "deny"',
        'unimplemented = "deny"',
        'unreachable = "deny"',
    ]
)


# 成员 crate 清单骨架：门禁继承 + dev-dependency 测试环（与真实仓库同形）。
def member_manifest(name: str) -> str:
    return "\n".join(
        [
            "[package]",
            f'name = "{name}"',
            'version = "0.6.0"',
            'edition = "2024"',
            "",
            "[dev-dependencies]",
            'nsh-app = { path = "../..", features = ["test-utils"] }',
            "",
            "[lints]",
            "workspace = true",
            "",
        ]
    )


# infra 清单骨架：http 投影门宿主（axum optional + `http = ["dep:axum"]`，default 不含 http）。
INFRA_MANIFEST = "\n".join(
    [
        "[package]",
        'name = "ledger-infra"',
        'version = "0.6.0"',
        'edition = "2024"',
        "",
        "[features]",
        "test-utils = []",
        'http = ["dep:axum"]',
        "",
        "[dependencies]",
        'axum = { version = "0.8", optional = true }',
        "",
        "[lints]",
        "workspace = true",
        "",
    ]
)


# 写夹具文件（相对 crate src）：自动建档目录。
def write_crate_file(src_tauri: str, dir_: str, rel: str, content: str) -> None:
    abs_path = os.path.join(src_tauri, dir_, "src", rel)
    os.makedirs(os.path.dirname(abs_path), exist_ok=True)
    Path(abs_path).write_text(content, encoding="utf-8")


# 建 workspace 骨架夹具：`<root>/src-app/{Cargo.toml,src,crates/*}` + 仓库根门槛
# 宿主（scripts/check.sh、scripts/test.sh、.github/workflows/build.yml）。成员清单
# 自 `CRATES` 政策核（成员登记核对的唯一对象），模块面自用例声明的 `{crate, dir,
# libRs, files}`（#1595 T2-3）。返回脚本参数 `[src-dir, src-app-dir]`。
def make_crate_fixture(
    tmp_path: Path, overrides: CrateFixtureOverrides | None = None
) -> list[str]:
    overrides = overrides if overrides is not None else CrateFixtureOverrides()
    root = Path(tempfile.mkdtemp(prefix="check-structure-crate-", dir=str(tmp_path)))
    src_tauri = str(root / "src-app")
    os.makedirs(src_tauri, exist_ok=True)

    root_manifest = (
        overrides.root_manifest
        if overrides.root_manifest is not None
        else "\n".join(
            [
                "[package]",
                'name = "nsh-app"',
                'version = "0.6.0"',
                'edition = "2024"',
                "",
                "[features]",
                'test-utils = ["ledger-infra/test-utils"]',
                overrides.root_features_extra if overrides.root_features_extra is not None else "",
                "",
                "[dependencies]",
                overrides.root_infra_prod_dep
                if overrides.root_infra_prod_dep is not None
                else 'ledger-infra = { path = "crates/infra" }',
                "",
                "[workspace]",
                'members = ["crates/*"]',
                'resolver = "3"',
                "",
                "[workspace.lints.clippy]",
                LINT_DENIES,
                "",
                "[dev-dependencies]",
                'nsh-app = { path = ".", features = ["test-utils"] }',
                "",
                "[lints]",
                "workspace = true",
                "",
            ]
        )
    )
    Path(src_tauri, "Cargo.toml").write_text(root_manifest, encoding="utf-8")

    cases: dict[str, CrateCase] = {}
    for c in SKELETON_CASES:
        cases[c.dir] = c
    for c in overrides.module_cases or ():
        # 夹具自足化要求用例显式声明成员 crate：声明与政策核不符即夹具自身出错（否则
        # 错字会让用例静默落在骨架默认形状上、变绿变假）。
        member = next((x for x in CRATES if x.dir == c.dir), None)
        if member is None or member.name != c.crate:
            raise RuntimeError(f"夹具声明与 CRATES 政策核不一致：{c.crate} / {c.dir}")
        cases[c.dir] = c
    # 当期入口调用方闭集的在册成员面（#1692）：为每个在册 crate 文件补 lib.rs 声明
    # 与含调用的桩（骨架自足化，与模块面用例显式声明同一理由）；先克隆再改写，
    # 骨架常量对象跨夹具共享，不得原地变异。omit_convert_callers 留给缺文件负向用例。
    omit_callers = set(overrides.omit_convert_callers or ())

    def caller_content(file: str) -> str:
        if overrides.convert_caller_content is not None and file in overrides.convert_caller_content:
            return overrides.convert_caller_content[file]
        return CONVERT_CALL_STUB

    for entry in CONVERT_CURRENT_CALLERS:
        if entry.file in omit_callers or not entry.file.startswith("crates/"):
            continue
        rest = entry.file[len("crates/") :]
        slash = rest.index("/")
        crate_dir = f"crates/{rest[:slash]}"
        # files 键含 .rs 后缀（与磁盘文件同名），mod 声明名去后缀——两者不同源，混用
        # 会写出无后缀文件、被模块面投影当目录扫而 ENOTDIR。
        file_name = rest[slash + 1 :]
        if file_name.startswith("src/"):
            file_name = file_name[len("src/") :]
        key = file_name[: -len(".rs")] if file_name.endswith(".rs") else file_name
        spec = cases.get(crate_dir)
        if spec is None:
            crate = next((x for x in CRATES if x.dir == crate_dir), None)
            if crate is None:
                continue
            spec = default_case(crate.name, crate_dir)
        spec = dataclasses.replace(spec, files=dict(spec.files))
        cases[crate_dir] = spec
        content = caller_content(entry.file)
        if key == "lib":
            spec.lib_rs += content
        else:
            if file_name not in spec.files:
                spec.files[file_name] = content
            if f"mod {key};" not in spec.lib_rs:
                spec.lib_rs += f"pub mod {key};\n"
    for crate in CRATES:
        if crate.dir == ".":
            continue
        spec_c = cases.get(crate.dir)
        if spec_c is None:
            spec_c = default_case(crate.name, crate.dir)
        os.makedirs(os.path.join(src_tauri, crate.dir, "src"), exist_ok=True)
        manifest: str
        if overrides.member_manifests is not None and crate.dir in overrides.member_manifests:
            manifest = overrides.member_manifests[crate.dir]
        elif crate.name == "ledger-infra":
            manifest = INFRA_MANIFEST
        else:
            manifest = member_manifest(crate.name)
        Path(src_tauri, crate.dir, "Cargo.toml").write_text(manifest, encoding="utf-8")
        Path(src_tauri, crate.dir, "src", "lib.rs").write_text(spec_c.lib_rs, encoding="utf-8")
        for rel, content in spec_c.files.items():
            write_crate_file(src_tauri, crate.dir, rel, content)

    if overrides.infra_lib_rs is not None:
        Path(src_tauri, INFRA_SRC_REL, "lib.rs").write_text(overrides.infra_lib_rs, encoding="utf-8")
    if overrides.infra_error_rs is not None:
        Path(src_tauri, INFRA_SRC_REL, "error.rs").write_text(
            overrides.infra_error_rs, encoding="utf-8"
        )

    # 根包壳层面：lib.rs 与投资五节锚点（#1185）。
    # 锚点门桩住根包 src/api_server（#2081 迁居 + #2083 升根包）。
    os.makedirs(os.path.join(src_tauri, "src", "api_server", "handlers"), exist_ok=True)
    Path(src_tauri, "src", "lib.rs").write_text("pub fn stub() {}\n", encoding="utf-8")
    Path(src_tauri, "src", "api_server", "handlers", "import.rs").write_text(
        overrides.api_server_import_rs
        if overrides.api_server_import_rs is not None
        else '#[cfg(any(test, feature = "test-utils"))]\n'
        "#[doc(hidden)]\n"
        'pub const INVESTMENT_SECTION_HEADERS: [&str; 5] = ["## 节"];\n',
        encoding="utf-8",
    )
    Path(src_tauri, "src", "api_server", "mod.rs").write_text(
        overrides.api_server_mod_rs
        if overrides.api_server_mod_rs is not None
        else '#[cfg(any(test, feature = "test-utils"))]\n'
        "#[doc(hidden)]\n"
        "pub use handlers::import::INVESTMENT_SECTION_HEADERS;\n",
        encoding="utf-8",
    )

    # 当期入口闭集的根 src 在册成员（#1692）：与 crate 面同款自足化写出。
    for entry in CONVERT_CURRENT_CALLERS:
        if entry.file in omit_callers or not entry.file.startswith("src/"):
            continue
        abs_path = os.path.join(src_tauri, entry.file)
        os.makedirs(os.path.dirname(abs_path), exist_ok=True)
        Path(abs_path).write_text(caller_content(entry.file), encoding="utf-8")

    if overrides.orphan_crate is not None:
        os.makedirs(os.path.join(src_tauri, "crates", overrides.orphan_crate, "src"), exist_ok=True)
        Path(src_tauri, "crates", overrides.orphan_crate, "Cargo.toml").write_text(
            '[package]\nname = "orphan"\nversion = "0.1.0"\nedition = "2024"\n',
            encoding="utf-8",
        )
        Path(src_tauri, "crates", overrides.orphan_crate, "src", "lib.rs").write_text(
            "pub fn stub() {}\n", encoding="utf-8"
        )

    os.makedirs(os.path.join(root, "scripts"), exist_ok=True)
    os.makedirs(os.path.join(root, ".github", "workflows"), exist_ok=True)
    Path(root, "scripts", "check.sh").write_text(
        overrides.check_sh
        if overrides.check_sh is not None
        else "( cd src-app && cargo clippy --workspace --all-targets --all-features -- -D warnings )\n"
        "( cd src-app && cargo fmt --all -- --check )\n",
        encoding="utf-8",
    )
    Path(root, "scripts", "test.sh").write_text(
        overrides.test_sh if overrides.test_sh is not None else "( cd src-app && cargo test --workspace )\n",
        encoding="utf-8",
    )
    Path(root, "scripts", "lint-fix.sh").write_text(
        overrides.lint_fix_sh
        if overrides.lint_fix_sh is not None
        else "( cd src-app && cargo fmt --all && cargo clippy --fix --workspace --all-targets --all-features --allow-dirty --allow-staged )\n",
        encoding="utf-8",
    )
    Path(root, "scripts", "test_exec.py").write_text(
        overrides.test_exec_py
        if overrides.test_exec_py is not None
        else "# 构建一次（口径说明：`cargo test` 默认执行面）\n"
        'BUILD_CWD = "src-app"\n'
        "await run_child(\n"
        "    cargo,\n"
        "    [\n"
        '        "test",\n'
        '        "--workspace",\n'
        '        "--no-run",\n'
        "    ],\n"
        ")\n",
        encoding="utf-8",
    )
    Path(root, ".github", "workflows", "build.yml").write_text(
        overrides.workflow
        if overrides.workflow is not None
        else "jobs:\n"
        "  b:\n"
        "    steps:\n"
        '      - run: cargo test --workspace --lib --test "*"\n'
        "      - run: cargo fmt --all --check\n"
        "      - run: cargo clippy --workspace --all-targets --all-features -- -D warnings\n"
        "\n",
        encoding="utf-8",
    )

    return [os.path.join(src_tauri, "src"), src_tauri]


# 骨架 + 用例声明的目标 crate 模块面（#1595 T2-3：{crate, dir, libRs, files}）。
def fixture_with(
    tmp_path: Path, case_spec: CrateCase, overrides: CrateFixtureOverrides | None = None
) -> list[str]:
    merged = overrides if overrides is not None else CrateFixtureOverrides()
    merged = dataclasses.replace(
        merged, module_cases=(case_spec, *(merged.module_cases or ()))
    )
    return make_crate_fixture(tmp_path, merged)


class TestCheckStructure:
    """check-structure（结构守门）"""

    def test_real_repo_default_passes_module_projection(self) -> None:
        # 真实仓库自检绿是唯一能发现「派生器认不出仓库实际形态」的测试。
        r = run([])
        assert r.status == 0
        assert "零依赖" in r.output
        assert f"crate 模块面 {len(CRATE_MODULE_TARGETS)} 份" in r.output
        # 投影覆盖全等（防派生器静默丢成员）：每个非壳层政策核成员恰有一份模块面
        #（壳层 = 根包 nsh-app 不整册登记模块面；#2083 切轨后白名单政策条目表随
        # 旧壳退役）。
        member_crates = [crate for crate in CRATES if crate.layer != CRATE_LAYER.SHELL]
        assert len(CRATE_MODULE_TARGETS) == len(member_crates)
        assert sorted(t.src_rel for t in CRATE_MODULE_TARGETS) == sorted(
            f"{crate.dir}/src" for crate in member_crates
        )

    def test_skeleton_fixture_all_clean_stubs_passes(self, tmp_path: Path) -> None:
        r = run(make_crate_fixture(tmp_path))
        assert r.status == 0
        assert "零依赖" in r.output


class TestModuleFaceProjection:
    """check-structure 模块面投影核对（#1595：expected 由 lib.rs mod 声明派生）"""

    def test_orphan_module_file_red(self, tmp_path: Path) -> None:
        # 孤儿模块文件（磁盘多出未声明模块）→ 红（判据②：删检测即变红）
        args = fixture_with(tmp_path, accounts_case())
        write_crate_file(args[1], "crates/accounts", "orphan.rs", STUB)
        r = run(args)
        assert r.status == 1
        assert "未声明模块" in r.output
        assert "orphan" in r.output
        assert "crates/accounts/src" in r.output

    def test_orphan_module_dir_red(self, tmp_path: Path) -> None:
        # 孤儿模块目录（扫得到非测试文件的目录）→ 红
        args = fixture_with(tmp_path, accounts_case())
        write_crate_file(args[1], "crates/accounts", "orphan_dir/helper.rs", STUB)
        r = run(args)
        assert r.status == 1
        assert "未声明模块" in r.output
        assert "orphan_dir" in r.output

    def test_test_exemption_forms_do_not_trigger_orphan(self, tmp_path: Path) -> None:
        # 测试豁免形态不触发未声明：顶层 tests.rs 与仅含 tests.rs 的目录（ADR-0056 决策 5）
        args = fixture_with(tmp_path, accounts_case())
        write_crate_file(args[1], "crates/accounts", "tests.rs", STUB)
        write_crate_file(args[1], "crates/accounts", "extra/tests.rs", STUB)
        r = run(args)
        assert r.status == 0

    def test_declared_but_missing_on_disk_red(self, tmp_path: Path) -> None:
        # lib.rs 声明而磁盘缺失 → 红（声明即 rustc 契约）
        args = fixture_with(tmp_path, accounts_case(lib_rs="pub mod core;\npub mod ghost;\n"))
        r = run(args)
        assert r.status == 1
        assert "声明缺失" in r.output
        assert "ghost" in r.output

    def test_missing_lib_rs_fail_loud(self, tmp_path: Path) -> None:
        # crate 根 lib.rs 缺失 → fail loud（无法派生 expected）
        args = fixture_with(tmp_path, accounts_case())
        os.remove(os.path.join(args[1], "crates/accounts/src/lib.rs"))
        r = run(args)
        assert r.status == 1
        assert "找不到 crate 根声明文件" in r.output
        assert "crates/accounts/src/lib.rs" in r.output

    def test_lib_rs_is_decl_and_reexport_face(self, tmp_path: Path) -> None:
        # crate 根 lib.rs 是声明与再导出面：再导出不入 expected、不误报
        args = fixture_with(
            tmp_path, accounts_case(lib_rs="pub mod core;\npub use crate::core::Account;\n")
        )
        r = run(args)
        assert r.status == 0

    def test_file_and_dir_module_same_name_coexist(self, tmp_path: Path) -> None:
        # 文件模块与目录模块同名共存（transport.rs + transport/）→ 绿（声明面与磁盘双向全等）
        # `mod transport;` 在 Rust 里同时允许 transport.rs（模块本体）与 transport/
        # （子模块目录）——磁盘枚举按模块键判等，不因同名报孤儿。
        args = fixture_with(
            tmp_path,
            CrateCase(
                crate="ledger-sync-engine",
                dir="crates/sync-engine",
                lib_rs="pub mod transport;\n",
                files={"transport.rs": STUB, "transport/s3.rs": STUB},
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_shape_cfg_test_tests_exempt(self, tmp_path: Path) -> None:
        # 形状判定：#[cfg(test)] mod tests; 豁免（ADR-0056 决策 5）→ 绿
        args = fixture_with(
            tmp_path, accounts_case(lib_rs="pub mod core;\n#[cfg(test)]\nmod tests;\n")
        )
        r = run(args)
        assert r.status == 0

    def test_shape_exempt_by_target_module_name_tests(self, tmp_path: Path) -> None:
        # 形状判定：豁免按目标模块名 tests 判，#[cfg(test)] mod helper; 仍需文件
        bad = run(
            fixture_with(
                tmp_path, accounts_case(lib_rs="pub mod core;\n#[cfg(test)]\nmod helper;\n")
            )
        )
        assert bad.status == 1
        assert "声明缺失" in bad.output
        assert "helper" in bad.output
        good = run(
            fixture_with(
                tmp_path,
                accounts_case(
                    files={"helper.rs": STUB},
                    lib_rs="pub mod core;\n#[cfg(test)]\nmod helper;\n",
                ),
            )
        )
        assert good.status == 0

    def test_shape_non_pub_and_lint_attr_mods_counted(self, tmp_path: Path) -> None:
        # 形状判定：非 pub mod、pub(crate) mod、#[allow(dead_code)] mod 计入 expected → 绿
        args = fixture_with(
            tmp_path,
            accounts_case(
                files={
                    "balance.rs": STUB,
                    "command.rs": STUB,
                    "core.rs": STUB,
                    "model.rs": STUB,
                },
                lib_rs=(
                    "mod balance;\n#[allow(dead_code)]\nmod command;\n"
                    "#[allow(dead_code)] mod core;\npub(crate) mod model;\n"
                ),
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_shape_feature_gated_mod_counted(self, tmp_path: Path) -> None:
        # 形状判定：feature 门控 mod（含 rustfmt 拆行多行 cfg）计入 expected → 绿
        args = fixture_with(
            tmp_path,
            accounts_case(
                files={
                    "balance.rs": STUB,
                    "command.rs": STUB,
                    "core.rs": STUB,
                    "model.rs": STUB,
                },
                lib_rs=(
                    '#[cfg(feature = "x")]\nmod balance;\n'
                    '#[cfg(any(\n    test,\n    feature = "test-utils",\n))]\n#[doc(hidden)]\nmod command;\n'
                    "mod core;\nmod model;\n"
                ),
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_shape_raw_identifier_decl_counted(self, tmp_path: Path) -> None:
        # 形状判定：raw identifier 声明（mod r#type;）计入 expected → 绿
        args = fixture_with(
            tmp_path,
            accounts_case(
                files={"core.rs": STUB, "type.rs": STUB},
                lib_rs="pub mod core;\npub mod r#type;\n",
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_shape_mod_in_comment_and_string_not_flagged(self, tmp_path: Path) -> None:
        # 形状判定：注释与字符串中的 mod 不误报（掩码边界）→ 绿
        args = fixture_with(
            tmp_path,
            accounts_case(
                lib_rs='// mod phantom;\nconst DOC: &str = "mod phantom;";\npub mod core;\n'
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_shape_path_attr_fail_loud(self, tmp_path: Path) -> None:
        # 形状判定：#[path] mod → fail loud（改写模块↔文件映射）
        args = fixture_with(
            tmp_path, accounts_case(lib_rs='#[path = "core_impl.rs"]\nmod core;\n')
        )
        r = run(args)
        assert r.status == 1
        assert "认不出的 lib.rs 声明形状" in r.output
        assert "path" in r.output

    def test_shape_include_fail_loud(self, tmp_path: Path) -> None:
        # 形状判定：include! → fail loud（展开面文本不可达）
        args = fixture_with(
            tmp_path, accounts_case(lib_rs='include!("generated.rs");\npub mod core;\n')
        )
        r = run(args)
        assert r.status == 1
        assert "认不出的 lib.rs 声明形状" in r.output
        assert "include!" in r.output

    def test_shape_inline_block_and_unknown_attr_fail_loud(self, tmp_path: Path) -> None:
        # 形状判定：内联模块块与认不出的属性链 → fail loud
        inline = run(fixture_with(tmp_path, accounts_case(lib_rs="mod core { }\n")))
        assert inline.status == 1
        assert "内联模块块" in inline.output
        unknown = run(
            fixture_with(tmp_path, accounts_case(lib_rs="#[some_proc_macro]\nmod core;\n"))
        )
        assert unknown.status == 1
        assert "认不出的 lib.rs 声明形状" in unknown.output
        assert "some_proc_macro" in unknown.output

    def test_single_file_module_decl_but_dir_only_tests_red(self, tmp_path: Path) -> None:
        # 单文件模块声明但目录只剩测试豁免 → 红（声明面要真实模块，不静默失靶）
        args = fixture_with(
            tmp_path, accounts_case(lib_rs="pub mod core;\npub mod only_tests;\n")
        )
        write_crate_file(args[1], "crates/accounts", "only_tests/tests.rs", STUB)
        r = run(args)
        assert r.status == 1
        assert "声明缺失" in r.output
        assert "only_tests" in r.output


# 成员 crate 清单桩：依赖面 + 门禁继承。
def _crate_manifest(name: str, deps: str) -> str:
    return (
        f'[package]\nname = "{name}"\nversion = "0.6.0"\nedition = "2024"\n\n'
        f"[dependencies]\n{deps}\n\n[lints]\nworkspace = true\n"
    )


# infra 清单桩：带 http 投影门 features/axum 形态 + 给定 dev-dependency 行。
def _infra_fixture_manifest(dev_deps: str) -> str:
    return (
        '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n'
        '[features]\nhttp = ["dep:axum"]\n\n'
        '[dependencies]\naxum = { version = "0.8", optional = true }\n\n'
        f"[dev-dependencies]\n{dev_deps}\n\n[lints]\nworkspace = true\n"
    )


class TestCrossCrateDependencyDirection:
    """check-structure 跨 crate 依赖方向（#1596 cargo 退役 / ADR-0071 修订注记）"""

    def test_text_scan_retired_infra_refs_domain_and_domain_refs_shell(
        self, tmp_path: Path
    ) -> None:
        # 基础设施文件引用域模块 / 域 crate 模块引用壳层 → 文本扫描已退役（红源换为编译期）
        # `crate::test_support` 在 ledger-infra 内即 `ledger_infra::test_support`——不存在；
        # 即便写成 `nsh_app::test_support`，dev-dependency 也只对测试目标可见，
        # 生产面编译失败。原文扫描已无靶向价值。
        args = fixture_with(
            tmp_path,
            infra_case(files={"db/helper.rs": "use crate::test_support::open;\npub fn x() {}\n"}),
        )
        r = run(args)
        assert r.status == 0
        account = fixture_with(
            tmp_path,
            accounts_case(files={"core.rs": "use crate::commands as shell;\npub fn y() {}\n"}),
        )
        r = run(account)
        assert r.status == 0

    def test_text_scan_retired_protocol_refs_domain(self, tmp_path: Path) -> None:
        # 协议 crate 模块引用域目录 → 文本扫描已退役（红源换为编译期 / 层秩声明面）
        args = fixture_with(
            tmp_path,
            protocol_case(files={"op.rs": "use crate::test_support::open;\npub fn x() {}\n"}),
        )
        r = run(args)
        assert r.status == 0

    def test_text_scan_retired_domain_refs_sync_domain(self, tmp_path: Path) -> None:
        # 域 crate 模块引用同步域 → 文本扫描已退役（红源换为编译期 / 声明面禁令）
        # `crate::sync_engine` 在域 crate 内编不过；`ledger_sync_engine::` 必伴随声明
        # 依赖，由 crate 边界禁令拦下（负向夹具见下）。
        args = fixture_with(
            tmp_path,
            accounts_case(
                files={
                    "core.rs": "use crate::sync_engine::engine::ReplayEffect;\npub fn x() {}\n"
                }
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_infra_dev_dep_higher_layer_unrecorded_red(self, tmp_path: Path) -> None:
        # 基础设施 dev-dependency 反向依赖更高层 crate 未留痕 → 红（换载体：声明面核对）
        # cargo 对 dev-dependency 环放行（测试目标与生产依赖图分离），是编译期盲区：
        # 基础设施对业务域 crate 的 dev-dependency 须逐条留痕于
        # INFRA_DOMAIN_ALLOWED_EDGES，清单之外的声明即红。
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/infra": _infra_fixture_manifest(
                        'ledger-policy = { path = "../policy" }'
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "基础设施→域方向" in r.output
        assert "ledger-policy" in r.output

    def test_infra_dev_dep_recorded_in_ledger_green(self, tmp_path: Path) -> None:
        # 基础设施 dev-dependency 已留痕于台账（测试专用边）→ 绿
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/infra": _infra_fixture_manifest(
                        'nsh-app = { path = "../..", features = ["test-utils"] }\n'
                        'ledger-transaction = { path = "../transaction" }'
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_infra_dev_dep_non_domain_layer_not_restricted(self, tmp_path: Path) -> None:
        # 基础设施 dev-dependency 指向非域层（协议 / 基础设施）→ 不受台账约束（生产方向仍归层秩核对）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/infra": _infra_fixture_manifest(
                        'ledger-infra = { path = "." }\n'
                        'ledger-sync-protocol = { path = "../sync-protocol" }'
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_infra_crate_prod_dep_domain_crate_red(self, tmp_path: Path) -> None:
        # 基础设施 crate 生产依赖域 crate → 红（层秩声明面核对，F8 生产面换载体后仍守）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/infra": (
                        '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n'
                        '[features]\nhttp = ["dep:axum"]\n\n'
                        '[dependencies]\naxum = { version = "0.8", optional = true }\n'
                        'ledger-policy = { path = "../policy" }\n\n[lints]\nworkspace = true\n'
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "crate 依赖方向" in r.output
        assert "ledger-infra" in r.output
        assert "ledger-policy" in r.output

    def test_protocol_crate_prod_dep_domain_and_shell_red(self, tmp_path: Path) -> None:
        # 协议 crate 生产依赖域 crate / 壳 crate → 红（层秩声明面核对，#1089 退役文本扫描后仍守）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/sync-protocol": _crate_manifest(
                        "ledger-sync-protocol",
                        'ledger-transaction = { path = "../transaction" }\n'
                        'nsh-app = { path = "../..", features = ["test-utils"] }',
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "crate 依赖方向" in r.output
        assert "ledger-sync-protocol" in r.output
        assert "ledger-transaction" in r.output
        assert "nsh-app" in r.output

    def test_domain_crate_prod_dep_transaction_green(self, tmp_path: Path) -> None:
        # 账户域 crate 生产依赖核心交易域 crate → 绿（域→域合法上层依赖，ADR-0071 决策 5）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/accounts": _crate_manifest(
                        "ledger-accounts",
                        'ledger-transaction = { path = "../transaction" }',
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_real_repo_declaration_face_passes(self) -> None:
        # 真实仓库默认通过：跨 crate 方向走声明面（认许边留痕于脚本）
        r = run([])
        assert r.status == 0
        assert "跨 crate 依赖方向：声明面判定" in r.output
        assert "基础设施 dev-dependency 方向零未留痕声明（认许边 4 条" in r.output


class TestInfraBlockEdges:
    """check-structure 基础设施 crate 内块间禁边（ADR-0111 决策 4 / #1134）"""

    def test_db_refs_boot_non_allowed_file_red(self, tmp_path: Path) -> None:
        # db 引用 boot（非认许边文件）→ 红并定位文件行号
        args = fixture_with(
            tmp_path,
            infra_case(
                files={
                    "db/helper.rs": "use crate::boot::encryption::probe_file_kind;\npub fn x() {}\n"
                }
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "crate 内反向依赖" in r.output
        assert "db/helper.rs:1" in r.output

    def test_both_block_edge_shapes_scanned_db_rs_red(self, tmp_path: Path) -> None:
        # 块间禁边两形态都入扫描：db.rs（单文件模块）注入违规同样红
        # 目录形态（db/helper.rs）由上一枚用例覆盖；本用例锁单文件模块形态
        #（db.rs）——两形态都在扫描面内（条目由声明投影，不因形态不同漏扫）。
        args = fixture_with(
            tmp_path,
            CrateCase(
                crate="ledger-infra",
                dir="crates/infra",
                lib_rs=INFRA_CASE.lib_rs,
                files={
                    "boot/mod.rs": STUB,
                    "db.rs": "use crate::boot::encryption::probe_file_kind;\npub fn x() {}\n",
                    "error.rs": INFRA_CASE.files["error.rs"],
                    "test_utils.rs": STUB,
                },
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "crate 内反向依赖" in r.output
        assert "db.rs:1" in r.output

    def test_db_refs_signals_red(self, tmp_path: Path) -> None:
        # db 引用 signals → 红（shell_support 靶已随 #1108 迁出退役）
        args = fixture_with(
            tmp_path,
            infra_case(files={"db/runtime.rs": "use crate::signals::WriteOp;\npub fn x() {}\n"}),
        )
        r = run(args)
        assert r.status == 1
        assert "signals" in r.output

    def test_db_mod_rs_shim_green_other_file_red(self, tmp_path: Path) -> None:
        # db/mod.rs 再导出 shim 合规绿；他文件同引用红
        shim = "pub use crate::boot::{encryption, data_location};\npub fn x() {}\n"
        args = fixture_with(tmp_path, infra_case(files={"db/mod.rs": shim}))
        r = run(args)
        assert r.status == 0
        bad = fixture_with(tmp_path, infra_case(files={"db/helper.rs": shim}))
        r = run(bad)
        assert r.status == 1
        assert "db/helper.rs:1" in r.output

    def test_boot_to_db_legal_direction_green(self, tmp_path: Path) -> None:
        # boot → db 合法单向 → 绿
        args = fixture_with(
            tmp_path,
            infra_case(
                files={"boot/helper.rs": "use crate::db::open_connection;\npub fn x() {}\n"}
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_block_path_in_comment_and_string_not_flagged(self, tmp_path: Path) -> None:
        # 注释与字符串中的块路径不误报
        args = fixture_with(
            tmp_path,
            infra_case(
                files={
                    "db/helper.rs": "\n".join(
                        [
                            "/// [`crate::boot`] 升顶层（文档注释不算）",
                            "// 历史 shell_support 引用已随 #1108 迁出根包",
                            'let s = "crate::signals::WriteOp";',
                            "pub fn f() {}",
                            "",
                        ]
                    )
                }
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_external_test_dir_exempt_db_tests_refs_boot(self, tmp_path: Path) -> None:
        # 外挂测试豁免：db/tests/ 引用 boot 不红
        args = fixture_with(
            tmp_path,
            infra_case(
                files={
                    "db/tests/common.rs": "pub fn s() { crate::boot::encryption::probe_file_kind(); }\n"
                }
            ),
        )
        r = run(args)
        assert r.status == 0


class TestModelDomainBan:
    """check-structure 模型域化禁令（ADR-0059 决策 6 / #424 T7 收口）"""

    def test_rule1_crate_models_path_residue_red(self, tmp_path: Path) -> None:
        # 规则①：crate::models 全局模型路径残留 → 红
        args = fixture_with(
            tmp_path,
            accounts_case(files={"core.rs": "use crate::models::Transaction;\npub fn x() {}\n"}),
        )
        r = run(args)
        assert r.status == 1
        assert "全局模型路径残留" in r.output
        assert "core.rs:1" in r.output

    def test_rule1_models_path_in_comment_and_string_not_flagged(self, tmp_path: Path) -> None:
        # 规则①：注释与字符串中的 models 路径不误报（掩码边界）
        args = make_crate_fixture(tmp_path)
        write_crate_file(
            args[1],
            "src",
            "test_support/cost.rs",
            "\n".join(
                [
                    "/// 全局模型目录已消亡，crate::models 是历史形态（文档注释不算引用）",
                    "// 见 crate::models::Transaction 说明",
                    'let s = "crate::models::Transaction";',
                    "pub fn f() {}",
                ]
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_rule1_external_test_exempt(self, tmp_path: Path) -> None:
        # 规则①：外挂测试豁免（tests.rs / tests/ 目录不参与扫描）
        args = make_crate_fixture(tmp_path)
        write_crate_file(
            args[1], ".", "test_support/tests.rs", "use crate::models::Transaction;\n"
        )
        write_crate_file(
            args[1],
            ".",
            "test_support/tests/scaffold.rs",
            "use crate::models::Transaction;\n",
        )
        r = run(args)
        assert r.status == 0

    def test_rule2_domain_seam_glob_reexport_red(self, tmp_path: Path) -> None:
        # 规则②：域接缝 glob 再导出 pub use model::* → 红
        args = make_crate_fixture(tmp_path)
        write_crate_file(
            args[1], ".", "test_support/mod.rs", "mod model;\npub use model::*;\n"
        )
        r = run(args)
        assert r.status == 1
        assert "glob 再导出" in r.output
        assert "test_support/mod.rs:2" in r.output

    def test_rule2_cross_domain_flatten_and_legacy_dir_forms_red(self, tmp_path: Path) -> None:
        # 规则②：跨域拍平与旧全局目录同名形态 → 红
        flat = make_crate_fixture(tmp_path)
        write_crate_file(
            flat[1], ".", "test_support/mod.rs", "pub use crate::transaction::model::*;\n"
        )
        r = run(flat)
        assert "glob 再导出" in r.output
        legacy = make_crate_fixture(tmp_path)
        write_crate_file(legacy[1], ".", "test_support/mod.rs", "pub use models::*;\n")
        r = run(legacy)
        assert "glob 再导出" in r.output

    def test_rule2_glob_aggregation_in_model_file_red(self, tmp_path: Path) -> None:
        # 规则②：域模型文件内 glob 聚合 → 红（文件与目录形态同判）
        file_case = make_crate_fixture(tmp_path)
        write_crate_file(
            file_case[1], ".", "test_support/model.rs", "pub use super::crud::*;\n"
        )
        r = run(file_case)
        assert r.status == 1
        assert "glob 聚合" in r.output
        assert "test_support/model.rs:1" in r.output
        dir_case = make_crate_fixture(tmp_path)
        write_crate_file(
            dir_case[1],
            ".",
            "test_support/model/price.rs",
            "pub use crate::test_support::types::*;\npub fn x() {}\n",
        )
        r2 = run(dir_case)
        assert r2.status == 1
        assert "域模型文件内 glob 聚合" in r2.output
        assert "test_support/model/price.rs" in r2.output

    def test_rule2_per_type_reexport_and_private_glob_green(self, tmp_path: Path) -> None:
        # 规则②：逐类型再导出与域内私有 glob 引用合规 → 绿
        args = make_crate_fixture(tmp_path)
        write_crate_file(
            args[1],
            ".",
            "test_support/mod.rs",
            "mod model;\npub use model::{Item, ItemInput};\n",
        )
        write_crate_file(
            args[1],
            ".",
            "test_support/behavior.rs",
            "use super::model::*;\npub fn x() {}\n",
        )
        write_crate_file(args[1], ".", "test_support/model.rs", "pub struct Item;\n")
        r = run(args)
        assert r.status == 0


class TestNativeTxStmtBan:
    """check-structure 原生事务语句禁令（issue #1014 / #1003 定案 7）"""

    def test_handwritten_begin_in_product_code_red(self, tmp_path: Path) -> None:
        # 产品代码手写 BEGIN → 红并定位文件行号
        args = fixture_with(
            tmp_path,
            accounts_case(
                files={
                    "core.rs": 'pub fn f(conn: &Connection) {\n    conn.execute("BEGIN", []);\n}\n'
                }
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "原生事务语句" in r.output
        assert "core.rs:2" in r.output
        assert "db/tx_scope.rs" in r.output

    def test_commit_rollback_and_execute_batch_red(self, tmp_path: Path) -> None:
        # COMMIT / ROLLBACK 手写与 execute_batch 变体同样识别 → 红
        args = fixture_with(
            tmp_path,
            accounts_case(
                files={
                    "core.rs": (
                        "pub fn f(conn: &Connection) {\n"
                        '    conn.execute("COMMIT", []);\n'
                        '    conn.execute_batch("ROLLBACK");\n'
                        "}\n"
                    )
                }
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "core.rs:2" in r.output
        assert "core.rs:3" in r.output
        assert "db/tx_scope.rs" in r.output

    def test_sole_legal_location_tx_scope_green(self, tmp_path: Path) -> None:
        # 唯一合法住址 db/tx_scope.rs 内的原生事务语句 → 绿（execute 与 execute_batch 同）
        args = fixture_with(
            tmp_path,
            infra_case(
                files={
                    "db/tx_scope.rs": (
                        "pub fn hold(conn: &Connection) {\n"
                        '    conn.execute("BEGIN", []);\n'
                        '    conn.execute_batch("COMMIT");\n'
                        "}\n"
                    )
                }
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_execute_begin_in_comment_not_flagged(self, tmp_path: Path) -> None:
        # 注释中的 execute("BEGIN") 不误报（只掩码注释、保留字符串）
        args = fixture_with(
            tmp_path,
            accounts_case(
                files={
                    "core.rs": "\n".join(
                        [
                            '// 原生事务语句禁令：conn.execute("BEGIN", []) 只许出现在 db/tx_scope.rs',
                            '/// conn.execute("COMMIT", [])',
                            "pub fn f() {}",
                            "",
                        ]
                    )
                }
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_real_repo_default_passes_native_tx_stmt(self) -> None:
        # 真实仓库默认通过：原生事务语句仅 db/tx_scope.rs 一处
        r = run([])
        assert r.status == 0
        assert "原生事务语句全树扫描" in r.output


class TestTransactionZoneOrdering:
    """check-structure 交易域区级层序（ADR-0113 决策 7 / #1181 / #1597）"""

    def test_root_level_single_file_must_move_into_zone_dir_red(self, tmp_path: Path) -> None:
        # 负向夹具各由一条断言锚定（ADR-0087 断言强度，断言对准退出码与输出）：
        # 区级层序、根级单文件、未登记区目录各有一枚夹具；删任一条断言须动脚本
        #（清单外无豁免面），对应夹具转绿 → 该夹具测试失败（CI 红）。
        # ④ 根级单文件必须进区目录（fail loud）→ 红（#1597，删除检查即变红）
        args = fixture_with(
            tmp_path,
            transaction_case(files={"search_semantics.rs": STUB}, lib_rs=TRANSACTION_CASE.lib_rs),
        )
        r = run(args)
        assert r.status == 1
        assert "根级单文件必须进区目录" in r.output

    def test_unregistered_top_level_dir_red(self, tmp_path: Path) -> None:
        # ④ 未登记区归属的顶层目录 → 红（判向不静默失靶，#1597）
        args = fixture_with(
            tmp_path,
            transaction_case(
                files={"rogue_zone/helper.rs": STUB},
                lib_rs=TRANSACTION_CASE.lib_rs + "pub mod rogue_zone;\n",
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "区目录未登记区归属" in r.output

    def test_zone_dir_table_matches_real_repo_top_level_dirs(self) -> None:
        # 区目录表与真实仓库顶层区目录全等（#1597）：zone 表无悬空行、顶层目录全有区归属
        # 常量表核对无法经夹具红，用双向全等锚替代（同登记面长度锚先例）：新增 /
        # 删除区目录而 zone 表不同步 → 本断言红；zone 表悬空行同理。真实仓库自检
        #（run([])）同样照 zone 表判向，两侧互为见证。
        src_dir = str(Path.cwd() / "src-app" / "crates" / "transaction" / "src")
        with os.scandir(src_dir) as it:
            dir_names = [
                e.name for e in it if e.is_dir(follow_symlinks=False) and e.name != "tests"
            ]
        dirs = sorted(
            name
            for name in dir_names
            if any(f.endswith(".rs") for f in os.listdir(os.path.join(src_dir, name)))
        )
        assert sorted(TRANSACTION_ZONE_BY_DIR) == dirs

    def test_real_repo_default_passes_zone_ordering(self) -> None:
        # 真实仓库默认通过：区级层序零未认许反向引用（#1182 消除反边后认许边归空）
        r = run([])
        assert r.status == 0
        assert "区级层序零未认许反向引用" in r.output
        assert f"认许边 {len(TRANSACTION_ZONE_ALLOWED_EDGES)} 条" in r.output

    def test_shared_zone_refs_write_zone_red(self, tmp_path: Path) -> None:
        # ② 共享语义引用写路径（认许边之外）→ 红并定位文件行号
        args = fixture_with(
            tmp_path,
            transaction_case(
                files={
                    "command/payload.rs": "use crate::write::writer::NormalizedRow;\npub fn x() {}\n"
                }
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "区级反向依赖" in r.output
        assert "command/payload.rs:1" in r.output

    def test_shared_zone_refs_seam_zone_red(self, tmp_path: Path) -> None:
        # ② 共享语义引用接缝（认许边之外）→ 红
        args = fixture_with(
            tmp_path,
            transaction_case(
                files={
                    "shared/search_text.rs": "use crate::seams::merchant::ensure_merchant;\npub fn x() {}\n"
                }
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "区级反向依赖" in r.output

    def test_seam_zone_refs_path_zones_red(self, tmp_path: Path) -> None:
        # ② 接缝引用路径区（写 / 读）→ 红
        seam_to_write = fixture_with(
            tmp_path,
            transaction_case(
                files={"seams/merchant.rs": "use crate::write::protocol::create;\npub fn x() {}\n"}
            ),
        )
        r = run(seam_to_write)
        assert r.status == 1
        seam_to_read = fixture_with(
            tmp_path,
            transaction_case(
                files={"seams/investment.rs": "use crate::read::list_transactions;\npub fn x() {}\n"}
            ),
        )
        r = run(seam_to_read)
        assert r.status == 1

    def test_write_read_paths_mutually_independent_red(self, tmp_path: Path) -> None:
        # ② 写读两径互不依赖（双向）→ 红
        write_to_read = fixture_with(
            tmp_path,
            transaction_case(
                files={
                    "write/batch.rs": "use crate::read::search::search_transactions;\npub fn x() {}\n"
                }
            ),
        )
        r = run(write_to_read)
        assert r.status == 1
        assert "区级反向依赖" in r.output
        read_to_write = fixture_with(
            tmp_path,
            transaction_case(
                files={
                    "read/extra.rs": "use crate::write::batch::TransactionBatch;\npub fn x() {}\n"
                }
            ),
        )
        r = run(read_to_write)
        assert r.status == 1

    def test_legal_rank_chain_green(self, tmp_path: Path) -> None:
        # 合法层序链：写→接缝→共享语义、读→同区、同区互依 → 绿
        args = fixture_with(
            tmp_path,
            transaction_case(
                files={
                    "write/batch.rs": (
                        "use crate::seams::balance::recalculate;\n"
                        "use crate::amount::TransactionKind;\npub fn x() {}\n"
                    ),
                    "read/search.rs": (
                        "use crate::read::source::list_view;\nuse crate::model::Transaction;\npub fn y() {}\n"
                    ),
                    "write/protocol.rs": "use crate::write::writer::insert_row;\npub fn z() {}\n",
                }
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_lib_rs_cross_zone_reexport_not_judged(self, tmp_path: Path) -> None:
        # crate 根 lib.rs 是声明与再导出面：跨区再导出不参与区级判向（免登不误报）
        args = fixture_with(
            tmp_path,
            transaction_case(
                lib_rs=TRANSACTION_CASE.lib_rs + "pub use crate::write::writer::NormalizedRow;\n"
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_cross_zone_path_in_comment_and_string_not_flagged(self, tmp_path: Path) -> None:
        # 注释与字符串中的跨区路径不误报（掩码边界）
        args = fixture_with(
            tmp_path,
            transaction_case(
                files={
                    "shared/search_text.rs": "\n".join(
                        [
                            "/// 消费方见 `crate::write::writer` 与 `crate::read`（文档注释不算依赖）",
                            "// crate::write::protocol::create",
                            'let s = "crate::write::batch::run";',
                            "pub fn f() {}",
                            "",
                        ]
                    )
                }
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_brace_list_expanded_entry_wise_non_first_segment_hits(self, tmp_path: Path) -> None:
        # 花括号列举逐条展开：非首段跨区条目同样命中
        args = fixture_with(
            tmp_path,
            transaction_case(
                files={
                    "shared/search_text.rs": "use crate::{model::Transaction, read::list_view};\npub fn x() {}\n"
                }
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "区级反向依赖" in r.output
        assert "read" in r.output

    def test_brace_list_close_brace_not_swallowed_off_by_one_anchor(self, tmp_path: Path) -> None:
        # 花括号列举闭括号后文本不吞入：后续枚举变体同名不误报（off-by-one 回归锚）
        args = fixture_with(
            tmp_path,
            transaction_case(
                files={
                    "shared/search_text.rs": (
                        "use crate::{model::Transaction};\npub enum E { A, write }\npub fn x() {}\n"
                    )
                }
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_external_test_dir_exempt_write_writer_tests_refs_read(self, tmp_path: Path) -> None:
        # 外挂测试豁免：write/writer/tests/ 引用读路径不红（ADR-0056 决策 5）
        args = fixture_with(
            tmp_path,
            transaction_case(
                files={
                    "write/writer/tests/fixture.rs": "use crate::read::list_view;\npub fn s() {}\n"
                }
            ),
        )
        r = run(args)
        assert r.status == 0


class TestCrateBoundaryChecks:
    """check-structure crate 边界核对（spec #1086 / issue #1087 门禁前置）"""

    def test_workspace_skeleton_fixture_passes(self, tmp_path: Path) -> None:
        # workspace 骨架夹具：成员登记 + 门禁继承 + 命令覆盖齐全 → 通过
        r = run(make_crate_fixture(tmp_path))
        assert r.status == 0
        assert f"crate 边界 {len(CRATES)} 个" in r.output

    def test_real_repo_crate_boundary_summary(self) -> None:
        # 真实仓库默认通过：crate 边界核对入摘要
        r = run([])
        assert r.status == 0
        assert f"crate 边界 {len(CRATES)} 个" in r.output

    def test_member_crate_missing_lints_inheritance_red(self, tmp_path: Path) -> None:
        # 成员 crate 删掉 [lints] workspace = true → 红（门禁继承删除即变红）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    INFRA_SRC_REL[: -len("/src")]: (
                        '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n'
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "门禁继承" in r.output
        assert "ledger-infra" in r.output

    def test_root_crate_missing_lints_inheritance_red(self, tmp_path: Path) -> None:
        # 根包删掉 [lints] workspace = true → 红
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                root_manifest="\n".join(
                    [
                        "[package]",
                        'name = "nsh-app"',
                        'version = "0.6.0"',
                        'edition = "2024"',
                        "",
                        "[workspace]",
                        'members = ["crates/*"]',
                        'resolver = "3"',
                        "",
                        "[workspace.lints.clippy]",
                        LINT_DENIES,
                        "",
                    ]
                )
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "根包缺 [lints] workspace = true" in r.output

    def test_new_member_dir_unregistered_red(self, tmp_path: Path) -> None:
        # 新增成员目录未登记 CRATES → 红（成员登记删除即变红）
        args = make_crate_fixture(tmp_path, CrateFixtureOverrides(orphan_crate="newdomain"))
        r = run(args)
        assert r.status == 1
        assert "未登记 CRATES" in r.output
        assert "crates/newdomain" in r.output

    def test_workspace_members_not_glob_red(self, tmp_path: Path) -> None:
        # workspace members 未用 crates/* glob → 红
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                root_manifest="\n".join(
                    [
                        "[package]",
                        'name = "nsh-app"',
                        'version = "0.6.0"',
                        'edition = "2024"',
                        "",
                        "[workspace]",
                        'members = ["crates/infra"]',
                        'resolver = "3"',
                        "",
                        "[workspace.lints.clippy]",
                        LINT_DENIES,
                        "",
                        "[lints]",
                        "workspace = true",
                        "",
                    ]
                )
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "crates/*" in r.output

    def test_static_and_test_commands_missing_workspace_red(self, tmp_path: Path) -> None:
        # 静态检查/测试命令缺 --workspace → 红（命令覆盖全成员删除即变红）
        args = make_crate_fixture(
            tmp_path, CrateFixtureOverrides(test_sh="( cd src-app && cargo test )\n")
        )
        r = run(args)
        assert r.status == 1
        assert "缺 --workspace" in r.output
        assert "test.sh" in r.output

    def test_test_exec_py_host_missing_workspace_red(self, tmp_path: Path) -> None:
        # test_exec.py（.py 形态宿主）缺 --workspace → 红（#1112 登记）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(test_exec_py='BUILD = "cargo test --no-run"\n'),
        )
        r = run(args)
        assert r.status == 1
        assert "缺 --workspace" in r.output
        assert "test_exec.py" in r.output

    def test_test_exec_py_comment_cargo_test_not_command(self, tmp_path: Path) -> None:
        # test_exec.py 注释里的 `cargo test` 不算命令（# 注释剔除，不假红）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                test_exec_py=(
                    "# 口径说明：cargo test 默认执行面\n"
                    'result = run(cargo, ["test", "--workspace", "--no-run"])\n'
                )
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_test_exec_py_array_form_missing_workspace_red(self, tmp_path: Path) -> None:
        # test_exec.py 数组形态命令（真实命令面）缺 --workspace → 红（#1112 P2 登记生效）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                test_exec_py="run_child(cargo, ['test', '--no-run'])\n"
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "数组形态缺" in r.output
        assert "test_exec.py" in r.output

    def test_test_exec_py_array_form_with_workspace_passes(self, tmp_path: Path) -> None:
        # test_exec.py 数组形态命令带 --workspace → 通过（数组面单独成立）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                test_exec_py="run_child(cargo, ['test', '--workspace', '--no-run'])\n"
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_command_wrapped_in_echo_red_empty_set_rejected(self, tmp_path: Path) -> None:
        # 命令被 echo 包成说明文字 → 红（引号内不是命令面，拒绝空集假绿，#1112 P1）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                test_sh='echo "( cd src-app && cargo test --workspace )"\n'
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "未发现任何命令位置上的 cargo 命令" in r.output
        assert "空集假绿" in r.output

    def test_all_targets_flag_not_workspace_scope_red(self, tmp_path: Path) -> None:
        # `--all-targets` 等 `--all*` 旗标不算 workspace 范围 → 红（防假绿回归）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                lint_fix_sh=(
                    "( cd src-app && cargo clippy --fix --all-targets --all-features "
                    "--allow-dirty --allow-staged )\n"
                )
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "缺 --workspace" in r.output
        assert "lint-fix.sh" in r.output

    def test_cargo_fmt_missing_all_red(self, tmp_path: Path) -> None:
        # cargo fmt 缺 --all → 红（fmt 的 workspace 别名是 --all）
        args = make_crate_fixture(
            tmp_path, CrateFixtureOverrides(check_sh="( cd src-app && cargo fmt -- --check )\n")
        )
        r = run(args)
        assert r.status == 1
        assert "缺 --workspace" in r.output
        assert "check.sh" in r.output

    def test_infra_crate_reverse_dep_shell_red(self, tmp_path: Path) -> None:
        # 基础设施 crate 反向依赖壳层 crate → 红（依赖方向核对）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/infra": (
                        '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n'
                        '[dependencies]\nnsh-app = { path = "../..", features = ["test-utils"] }\n\n'
                        "[lints]\nworkspace = true\n"
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "crate 依赖方向" in r.output
        assert "ledger-infra" in r.output

    def test_infra_crate_test_dev_dep_shell_green(self, tmp_path: Path) -> None:
        # 基础设施 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/infra": (
                        '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n'
                        '[features]\nhttp = ["dep:axum"]\n\n'
                        '[dependencies]\naxum = { version = "0.8", optional = true }\n\n'
                        '[dev-dependencies]\nnsh-app = { path = "../..", features = ["test-utils"] }\n\n'
                        "[lints]\nworkspace = true\n"
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_domain_crate_prod_dep_shell_red(self, tmp_path: Path) -> None:
        # 域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/accounts": (
                        '[package]\nname = "ledger-accounts"\nversion = "0.6.0"\nedition = "2024"\n\n'
                        '[dependencies]\nnsh-app = { path = "../..", features = ["test-utils"] }\n\n'
                        "[lints]\nworkspace = true\n"
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "crate 依赖方向" in r.output
        assert "ledger-accounts" in r.output

    def test_domain_crate_prod_dep_lower_domain_green(self, tmp_path: Path) -> None:
        # 域 crate 生产依赖更底层域 crate → 绿（域→域合法上层依赖，ADR-0112 决策 2）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/accounts": (
                        '[package]\nname = "ledger-accounts"\nversion = "0.6.0"\nedition = "2024"\n\n'
                        '[dependencies]\nledger-transaction = { path = "../transaction" }\n\n'
                        '[dev-dependencies]\nnsh-app = { path = "../..", features = ["test-utils"] }\n\n'
                        "[lints]\nworkspace = true\n"
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_all_member_crates_prod_dep_shell_each_red(self, tmp_path: Path) -> None:
        # 全成员 crate 生产依赖壳层 crate → 逐个红（旧逐 crate describe 的声明面覆盖等价）
        # 逐 crate 覆盖等价（#1596 前 12 个逐域 describe 各测本 crate 的声明面红线）：
        # 声明面核对对每个成员 crate 都成立，任一 crate 掉出核对面即此处红。
        for crate in CRATES:
            if crate.dir == ".":
                continue  # 根包即壳层自身
            manifest = (
                f'[package]\nname = "{crate.name}"\nversion = "0.6.0"\nedition = "2024"\n\n'
                '[dependencies]\nnsh-app = { path = "../..", features = ["test-utils"] }\n\n'
                "[lints]\nworkspace = true\n"
            )
            args = make_crate_fixture(
                tmp_path, CrateFixtureOverrides(member_manifests={crate.dir: manifest})
            )
            r = run(args)
            assert r.status == 1, f"{crate.name} 生产依赖壳层未变红"
            assert "crate 依赖方向" in r.output
            assert crate.name in r.output

    def test_domain_crate_prod_dep_sync_domain_red(self, tmp_path: Path) -> None:
        # 业务域 crate 生产依赖多端同步域 crate → 红（同层禁边，ADR-0101 决策 4b / #1107）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/accounts": (
                        '[package]\nname = "ledger-accounts"\nversion = "0.6.0"\nedition = "2024"\n\n'
                        '[dependencies]\nledger-sync-engine = { path = "../sync-engine" }\n\n'
                        '[dev-dependencies]\nnsh-app = { path = "../..", features = ["test-utils"] }\n\n'
                        "[lints]\nworkspace = true\n"
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "业务域 crate ledger-accounts 生产依赖多端同步域 ledger-sync-engine" in r.output


def _infra_lib_rs_with(test_utils_decl: str) -> str:
    return "pub mod boot;\npub mod db;\npub mod error;\n" + test_utils_decl


class TestTestUtilsProductionGate:
    """check-structure test_utils 生产编译门（ADR-0111 决策 5 / issue #1132）"""

    def test_real_repo_and_skeleton_default_passes(self, tmp_path: Path) -> None:
        # 真实仓库与骨架夹具默认通过：cfg 门 + 生产依赖不启用 test-utils
        r = run([])
        assert "test_utils 生产编译门" in r.output
        r = run(make_crate_fixture(tmp_path))
        assert r.status == 0

    def test_infra_lib_rs_cfg_gate_removed_red(self, tmp_path: Path) -> None:
        # infra lib.rs 摘掉 test_utils cfg 门 → 红（删除 cfg 门即变红）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                infra_lib_rs=_infra_lib_rs_with(
                    "pub fn stub() {}\n#[doc(hidden)]\npub mod test_utils;\n"
                )
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "test_utils 生产编译门" in r.output
        assert "cfg 门" in r.output

    def test_only_plain_comment_before_decl_still_red(self, tmp_path: Path) -> None:
        # 模块声明前只有普通注释 → 仍红（注释不构成门）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                infra_lib_rs=_infra_lib_rs_with(
                    "pub fn stub() {}\n// 仅注释说明，不构成门\npub mod test_utils;\n"
                )
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "test_utils 生产编译门" in r.output

    def test_cfg_gate_before_doc_hidden_green(self, tmp_path: Path) -> None:
        # cfg 门写在 doc(hidden) 之前 → 绿（属性顺序不敏感）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                infra_lib_rs=_infra_lib_rs_with(
                    'pub fn stub() {}\n#[doc(hidden)]\n#[cfg(any(test, feature = "test-utils"))]\n'
                    "pub mod test_utils;\n"
                )
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_rustfmt_multiline_cfg_gate_green(self, tmp_path: Path) -> None:
        # rustfmt 拆行的多行 cfg 门 → 绿（属性链解析读到配对闭合为止，#1469）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                infra_lib_rs=_infra_lib_rs_with(
                    'pub fn stub() {}\n#[cfg(any(\n    test,\n    feature = "test-utils",\n))]\n'
                    "#[doc(hidden)]\npub mod test_utils;\n"
                )
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_multiline_cfg_gate_comment_not_paren_not_flagged(self, tmp_path: Path) -> None:
        # 多行 cfg 门内注释提及 not( 不误判 → 绿（属性全文掩码注释后判定，#1469）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                infra_lib_rs=_infra_lib_rs_with(
                    'pub fn stub() {}\n#[cfg(any(\n'
                    "    // 反向门 not(test) 形态已废弃，改为 feature 放行\n"
                    '    feature = "test-utils",\n))]\npub mod test_utils;\n'
                )
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_multiline_reversed_gate_still_red(self, tmp_path: Path) -> None:
        # 多行反向门 #[cfg(not(…))] → 仍红（多行解析不弱化反向门判定，#1469）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                infra_lib_rs=_infra_lib_rs_with(
                    "pub fn stub() {}\n#[cfg(not(\n    test,\n))]\npub mod test_utils;\n"
                )
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "test_utils 生产编译门" in r.output
        assert "放行测试" in r.output

    def test_reversed_gate_not_test_red(self, tmp_path: Path) -> None:
        # 反向门 #[cfg(not(test))] → 红（模块只留给生产）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                infra_lib_rs=_infra_lib_rs_with(
                    "pub fn stub() {}\n#[cfg(not(test))]\npub mod test_utils;\n"
                )
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "放行测试" in r.output

    def test_unrelated_cfg_gate_red(self, tmp_path: Path) -> None:
        # 与测试无关的 cfg 门 → 红（等价于无门）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                infra_lib_rs=_infra_lib_rs_with(
                    "pub fn stub() {}\n#[cfg(debug_assertions)]\npub mod test_utils;\n"
                )
            ),
        )
        r = run(args)
        assert r.status == 1

    def test_root_prod_dep_enables_test_utils_red(self, tmp_path: Path) -> None:
        # 根包生产依赖 ledger-infra 启用 test-utils → 红（生产会编入测试器具）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                root_infra_prod_dep=(
                    'ledger-infra = { path = "crates/infra", features = ["test-utils"] }'
                )
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "test_utils 生产编译门" in r.output
        assert "生产依赖" in r.output

    def test_infra_or_root_features_default_contains_test_utils_red(
        self, tmp_path: Path
    ) -> None:
        # infra / 根包 [features] default 含 test-utils（含转发链） → 红
        infra_default = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/infra": (
                        '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n'
                        '[features]\ntest-utils = []\ndefault = ["test-utils"]\n\n'
                        "[lints]\nworkspace = true\n"
                    )
                },
            ),
        )
        r = run(infra_default)
        assert "default 包含 test-utils" in r.output
        root_default = make_crate_fixture(
            tmp_path, CrateFixtureOverrides(root_features_extra='default = ["test-utils"]')
        )
        r = run(root_default)
        assert "default 包含 test-utils" in r.output
        forwarded = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                root_features_extra='default = ["devkit"]\ndevkit = ["ledger-infra/test-utils"]'
            ),
        )
        r = run(forwarded)
        assert "default 包含 test-utils" in r.output


class TestInvestmentSectionAnchorGate:
    """check-structure 投资五节锚点生产编译门（issue #1185）"""

    def test_real_repo_and_skeleton_default_passes(self, tmp_path: Path) -> None:
        # 真实仓库与骨架夹具默认通过：常量与再导出均带「放行测试」cfg 门
        r = run([])
        assert "投资五节锚点生产编译门" in r.output
        r = run(make_crate_fixture(tmp_path))
        assert r.status == 0

    def test_anchor_const_cfg_gate_removed_red(self, tmp_path: Path) -> None:
        # 锚点常量摘掉 cfg 门 → 红（删除 cfg 门即变红）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                api_server_import_rs=(
                    '#[doc(hidden)]\npub const INVESTMENT_SECTION_HEADERS: [&str; 5] = ["## 节"];\n'
                )
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "投资五节锚点生产编译门" in r.output
        assert "cfg 门" in r.output

    def test_api_server_reexport_cfg_gate_removed_red(self, tmp_path: Path) -> None:
        # api_server 再导出摘掉 cfg 门 → 红（测试锚点会静默进生产二进制）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                api_server_mod_rs="pub use handlers::import::INVESTMENT_SECTION_HEADERS;\n"
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "投资五节锚点生产编译门" in r.output
        assert "放行测试" in r.output

    def test_anchor_decl_deleted_red(self, tmp_path: Path) -> None:
        # 锚点声明被删除 → 红（两层锁共享面不可无声明消失）
        args = make_crate_fixture(
            tmp_path, CrateFixtureOverrides(api_server_import_rs="pub fn stub() {}\n")
        )
        r = run(args)
        assert r.status == 1
        assert "投资五节锚点生产编译门" in r.output
        assert "找不到" in r.output


class TestHttpProjectionFeatureGate:
    """check-structure http 投影 feature 门（ADR-0111 决策 5 / issue #1133）"""

    def test_real_repo_and_skeleton_default_passes(self, tmp_path: Path) -> None:
        # 真实仓库与骨架夹具默认通过：axum optional + http 门 + default 不含 http + 域侧不启用
        r = run([])
        assert "http 投影 feature 门" in r.output
        r = run(make_crate_fixture(tmp_path))
        assert r.status == 0

    def test_infra_axum_dep_optional_removed_red(self, tmp_path: Path) -> None:
        # infra axum 依赖摘掉 optional → 红（裸依赖即无条件编入 axum）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/infra": (
                        '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n'
                        '[features]\nhttp = ["dep:axum"]\n\n'
                        '[dependencies]\naxum = "0.8"\n\n[lints]\nworkspace = true\n'
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "http 投影 feature 门" in r.output
        assert "optional" in r.output

    def test_http_feature_not_forwarding_dep_axum_red(self, tmp_path: Path) -> None:
        # http feature 未转发 dep:axum → 红（门与依赖面脱钩，门形同虚设）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/infra": (
                        '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n'
                        "[features]\nhttp = []\n\n"
                        '[dependencies]\naxum = { version = "0.8", optional = true }\n\n'
                        "[lints]\nworkspace = true\n"
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "dep:axum" in r.output

    def test_error_rs_impl_cfg_gate_removed_red(self, tmp_path: Path) -> None:
        # error.rs impl 摘掉 cfg 门 → 红（删除 cfg 门即变红）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                infra_error_rs=(
                    "pub struct AppError;\n"
                    "impl axum::response::IntoResponse for AppError {\n"
                    "    fn into_response(self) -> axum::response::Response {}\n"
                    "}\n"
                )
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "http 投影 feature 门" in r.output
        assert "cfg 门" in r.output

    def test_only_plain_comment_before_impl_still_red(self, tmp_path: Path) -> None:
        # impl 前只有普通注释 → 仍红（注释不构成门）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                infra_error_rs=(
                    "pub struct AppError;\n"
                    "// 仅注释说明，不构成门\n"
                    "impl axum::response::IntoResponse for AppError {\n"
                    "    fn into_response(self) -> axum::response::Response {}\n"
                    "}\n"
                )
            ),
        )
        r = run(args)
        assert r.status == 1

    def test_reversed_gate_not_feature_http_red(self, tmp_path: Path) -> None:
        # 反向门 #[cfg(not(feature = "http"))] → 红（feature 开启反而消失）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                infra_error_rs=(
                    "pub struct AppError;\n"
                    '#[cfg(not(feature = "http"))]\n'
                    "impl axum::response::IntoResponse for AppError {\n"
                    "    fn into_response(self) -> axum::response::Response {}\n"
                    "}\n"
                )
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "http 投影 feature 门" in r.output

    def test_rustfmt_multiline_cfg_gate_green(self, tmp_path: Path) -> None:
        # rustfmt 拆行的多行 cfg 门 → 绿（属性链解析读到配对闭合为止，#1469）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                infra_error_rs=(
                    "pub struct AppError;\n"
                    '#[cfg(all(\n    feature = "http",\n    target_os = "macOS",\n))]\n'
                    "impl axum::response::IntoResponse for AppError {\n"
                    "    fn into_response(self) -> axum::response::Response {}\n"
                    "}\n"
                )
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_multiline_reversed_gate_still_red(self, tmp_path: Path) -> None:
        # 多行反向门 #[cfg(not(…))] → 仍红（多行解析不弱化反向门判定，#1469）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                infra_error_rs=(
                    "pub struct AppError;\n"
                    '#[cfg(not(\n    feature = "http",\n))]\n'
                    "impl axum::response::IntoResponse for AppError {\n"
                    "    fn into_response(self) -> axum::response::Response {}\n"
                    "}\n"
                )
            ),
        )
        r = run(args)
        assert r.status == 1

    def test_infra_or_root_features_default_contains_http_red(self, tmp_path: Path) -> None:
        # infra / 根包 [features] default 含 http → 红（默认 feature 即生产编入）
        infra_default = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/infra": (
                        '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n'
                        '[features]\nhttp = ["dep:axum"]\ndefault = ["http"]\n\n'
                        '[dependencies]\naxum = { version = "0.8", optional = true }\n\n'
                        "[lints]\nworkspace = true\n"
                    )
                },
            ),
        )
        r = run(infra_default)
        assert "default 包含 http" in r.output
        root_default = make_crate_fixture(
            tmp_path, CrateFixtureOverrides(root_features_extra='default = ["http"]')
        )
        r = run(root_default)
        assert "default 包含 http" in r.output

    def test_domain_side_member_enables_http_red(self, tmp_path: Path) -> None:
        # 域侧成员生产依赖对 ledger-infra 启用 http → 红（域侧引入 axum）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/sync-protocol": (
                        "[package]\nname = \"ledger-sync-protocol\"\nversion = \"0.6.0\"\n"
                        'edition = "2024"\n\n'
                        '[dependencies]\nledger-infra = { path = "../infra", features = ["http"] }\n\n'
                        "[lints]\nworkspace = true\n"
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "http 投影 feature 门" in r.output
        assert "域侧" in r.output

    def test_domain_side_member_direct_axum_dep_red(self, tmp_path: Path) -> None:
        # 域侧成员直接声明 axum 生产依赖 → 红（不只经 ledger-infra/http 一条路）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/sync-protocol": (
                        "[package]\nname = \"ledger-sync-protocol\"\nversion = \"0.6.0\"\n"
                        'edition = "2024"\n\n'
                        '[dependencies]\nledger-infra = { path = "../infra" }\naxum = "0.8"\n\n'
                        "[lints]\nworkspace = true\n"
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "直接声明 axum" in r.output

    def test_domain_side_member_dev_dep_axum_green(self, tmp_path: Path) -> None:
        # 域侧成员仅 dev-dependencies 声明 axum → 绿（测试专用边不在此限）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                member_manifests={
                    "crates/sync-protocol": (
                        "[package]\nname = \"ledger-sync-protocol\"\nversion = \"0.6.0\"\n"
                        'edition = "2024"\n\n'
                        '[dependencies]\nledger-infra = { path = "../infra" }\n\n'
                        '[dev-dependencies]\naxum = "0.8"\n\n[lints]\nworkspace = true\n'
                    )
                },
            ),
        )
        r = run(args)
        assert r.status == 0


class TestConvertCurrentCallers:
    """check-structure 当期入口调用方闭集（ADR-0011 决策 3 + 2026-09-22 修订 ① / #1692）"""

    def test_real_repo_and_skeleton_default_passes(self, tmp_path: Path) -> None:
        # 真实仓库与骨架夹具默认通过：在册成员全部在册且恰有调用
        r = run([])
        assert r.status == 0
        assert f"当期入口调用方闭集 {len(CONVERT_CURRENT_CALLERS)} 成员全在册" in r.output
        r = run(make_crate_fixture(tmp_path))
        assert r.status == 0

    def test_caller_outside_whitelist_red_with_location(self, tmp_path: Path) -> None:
        # 白名单外生产文件调用当期入口 → 红并定位文件行号（白名单外即红）
        args = fixture_with(
            tmp_path,
            accounts_case(
                files={
                    "core.rs": 'pub fn probe() { amount::convert_to_native_current(conn, 1, "CNY") }\n'
                }
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "当期入口调用方闭集" in r.output
        assert "crates/accounts/src/core.rs:1" in r.output

    def test_mention_in_comment_and_string_green(self, tmp_path: Path) -> None:
        # 注释与字符串里的提及 → 绿（掩码后不构成调用）
        args = fixture_with(
            tmp_path,
            accounts_case(
                files={"core.rs": '// convert_to_native_current(conn, 1, "CNY")\npub fn stub() {}\n'}
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_fn_definition_face_green(self, tmp_path: Path) -> None:
        # `fn` 定义面 → 绿（定义不是调用）
        args = fixture_with(
            tmp_path,
            accounts_case(
                files={
                    "core.rs": (
                        "pub fn convert_to_native_current(c: &C, a: i64, s: &str) -> i64 { a }\n"
                        "pub fn stub() {}\n"
                    )
                }
            ),
        )
        r = run(args)
        assert r.status == 0

    def test_test_path_exempt_green(self, tmp_path: Path) -> None:
        # 测试路径豁免 → 绿（/tests/ 路径约定，ADR-0056 决策 5 同规）
        args = fixture_with(
            tmp_path, accounts_case(files={"tests/amount.rs": CONVERT_CALL_STUB})
        )
        r = run(args)
        assert r.status == 0

    def test_registered_member_file_missing_red(self, tmp_path: Path) -> None:
        # 在册成员文件缺失 → 红（清单漂移 fail loud）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(omit_convert_callers=("crates/scheduled/src/spend.rs",)),
        )
        r = run(args)
        assert r.status == 1
        assert "在册成员文件不存在" in r.output
        assert "crates/scheduled/src/spend.rs" in r.output

    def test_registered_member_zero_calls_red(self, tmp_path: Path) -> None:
        # 在册成员零调用 → 红（拒绝空集假绿：符号改名/调用全删不静默过）
        args = make_crate_fixture(
            tmp_path,
            CrateFixtureOverrides(
                convert_caller_content={"crates/dashboard/src/lib.rs": STUB}
            ),
        )
        r = run(args)
        assert r.status == 1
        assert "在册成员零调用" in r.output
        assert "crates/dashboard/src/lib.rs" in r.output
