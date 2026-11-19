import tempfile
from pathlib import Path

from check_test_support import FIXED_NOW_REGISTRY_REL
from gate_script_test_helper import RunResult, gate_script, run_gate_script

# 被测对象是仓库工具脚本 scripts/check_test_support.py（Rust 测试守门，issue #752 落地 /
# #758 收口转纯禁令 / ADR-0084 决策 8）。
# 脚本以 uv 运行时执行（ADR-0083 修订注记）：run_gate_script 以 spawn 'uv run' 与
# 门槛调用同款拉起，测的就是门槛路径。
# 按测试决策只测外部可观察结果——进程退出码与输出，通过位置参数把扫描目标指向
# 临时夹具目录（形状同构 src-app：src/ + tests/）。纯禁令下无白名单常量可注入，
# 全部判定均可经进程接缝覆盖，无需静态导入例外（test_check_structure.py 先例随
# 白名单机制一并移除，#758 收口）。规则 3 的固定时刻登记处夹具自脚本导出的
# FIXED_NOW_REGISTRY_REL 派生登记处位置（#1468：夹具与守门共亢单一来源，无双源
# 漂移；先例 check-test-stubs「命令清单以登记处为单一来源」用例）。
script = gate_script("check_test_support.py")


def run(args: tuple[str, ...] = ()) -> RunResult:
    return run_gate_script(script, args)


# 规则 3 夹具登记处现值：刻意取远离任何真实/历史快照的任意值——夹具登记处是
# 禁令的唯一事实源，值本身不重要；若提取逻辑被删改回硬编码快照，本值不再被
# 命中，规则 3 断言即红（ADR-0087 删除即变红）。
FIXTURE_NOW = "2031-03-04T05:06:07Z"
# 「登记处改值」用例的新值：与现值不同，锚定禁令跟随登记处而非脚本内快照。
FIXTURE_NOW_REVISED = "2032-08-09T10:11:12Z"


def fixed_now_decl(value: str) -> str:
    """登记处夹具行：声明形状与真实 src/test_support/mod.rs 的 FIXED_NOW 同构。"""
    return f'pub const FIXED_NOW: &str = "{value}";'


# 工厂种子登记处夹具：禁用表集合自 INSERT INTO 提取（单一事实源的形状同构）。
SEED_RS = "\n".join(
    [
        "// 测试夹具：统一测试数据库工厂种子登记处（形状同构 src/test_support/seed.rs）",
        'pub fn open() { let _ = "INSERT INTO accounts"; }',
        'pub fn seed_instrument() { let _ = "INSERT INTO instruments"; }',
    ]
)


def make_fixture(
    tmp_path: Path,
    files: dict[str, str],
    registry: str | None = fixed_now_decl(FIXTURE_NOW),
) -> str:
    """建夹具目录（形状同构 src-app：src/test_support/{mod.rs,seed.rs} 必在，登记处
    住址自脚本导出常量派生）。registry 缺省写现值登记处；传字符串模拟登记处改值；
    传 None 模拟登记处缺失（fail loud 用例）。返回目录路径。"""
    # 每次调用独立子目录（同用例可建多份夹具；tmp_path 退出时随夹具一并清理）
    dir_path = Path(tempfile.mkdtemp(dir=tmp_path))
    (dir_path / "src" / "test_support").mkdir(parents=True)
    (dir_path / "src" / "test_support" / "seed.rs").write_text(SEED_RS, encoding="utf-8")
    if registry is not None:
        registry_path = dir_path / FIXED_NOW_REGISTRY_REL
        registry_path.parent.mkdir(parents=True, exist_ok=True)
        registry_path.write_text(registry, encoding="utf-8")
    for name, content in files.items():
        path = dir_path / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
    return str(dir_path)


class TestRust测试守门纯禁令:
    """check-test-support（Rust 测试守门，纯禁令）"""

    def test_default_scan_real_repo_green(self) -> None:
        # 默认扫描本仓（门槛路径）：纯禁令全绿（#758 收口后存量清零）
        r = run()
        assert r.status == 0
        assert "Rust 测试守门通过" in r.output
        assert "纯禁令" in r.output
        assert "禁用种子表 6 张" in r.output

    def test_all_four_rules_hit_samples(self, tmp_path: Path) -> None:
        # 四条规则违规样本全部命中：直连建库、夹具裸 SQL、默认时刻字面量、自建通道线格式
        dir_path = make_fixture(
            tmp_path,
            {
                "src/ledger/tests.rs": "\n".join(
                    [
                        "use crate::db::{init_db, open_in_memory};",
                        "fn t() {",
                        "  let mut c = open_in_memory().unwrap();",
                        "  init_db(&mut c).unwrap();",
                        '  let sql = "INSERT INTO accounts (id) VALUES (1)";',
                        f'  let stamp = "{FIXTURE_NOW}";',
                        "}",
                    ]
                ),
                # 平行建库入口：DbState::open_in_memory 内部即两行序，同样命中（裸标识符匹配）
                "src/ledger/tests/state.rs": "fn t() { let s = DbState::open_in_memory().unwrap(); }",
                # 顶屋 tests/ 子目录下的共享层：文件名 common.rs 但父目录非 tests，薄皮豁免不适用
                "tests/api_server/common.rs": (
                    'fn seed() { let _ = "INSERT INTO instruments (id) VALUES (1)"; }'
                ),
                # 规则 4：手工构造通道段/清单 + 引用了通道面后的自建摘要（#956）
                "tests/commands/channel.rs": "\n".join(
                    [
                        "use crate::sync_engine::{ChannelManifest, SegmentEntry, EnvelopeMode};",
                        "fn t() {",
                        "  let m = ChannelManifest { version: 1, streams: vec![], checkpoint: None };",
                        '  let e = SegmentEntry { file: "x".into(), first_clock: 1, last_clock: 1, size: 1, sha256: "y".into() };',
                        "  use sha2::{Digest, Sha256};",
                        '  let _ = Sha256::digest(b"z");',
                        "  let _ = (m, e);",
                        "}",
                    ]
                ),
                # 规则 4 作用域限定：未引用通道面的摘要用法不误报（ledger-perf 形态）
                "src/ledger/perf/tests.rs": "\n".join(
                    [
                        "use sha2::Digest;",
                        "fn digest_db() { let mut h = sha2::Sha256::new(); let _ = &mut h; }",
                    ]
                ),
            },
        )
        r = run((dir_path,))
        assert r.status == 1
        assert "src/ledger/tests.rs" in r.output
        assert "规则 1（直连建库）命中 4 处" in r.output
        assert "规则 2（夹具裸SQL）命中 1 处" in r.output
        assert "规则 3（默认时刻字面量）命中 1 处" in r.output
        assert "src/ledger/tests/state.rs" in r.output
        assert "tests/api_server/common.rs" in r.output
        # 1 处清单字面构造 + 1 处段字面构造 + 3 处摘要标识（use 行 + Sha256::digest + sha2 路径）
        assert "规则 4（自建通道线格式）命中 5 处" in r.output
        assert "tests/commands/channel.rs" in r.output
        assert "src/ledger/perf/tests.rs" not in r.output

    def test_legal_forms_no_false_positive(self, tmp_path: Path) -> None:
        # 合法形态不误报：工厂本体、域薄皮种子 SQL、域时刻字面量、db 产品代码
        dir_path = make_fixture(
            tmp_path,
            {
                # 工厂本体：建库 + 种子 SQL 全部合法（四条规则豁免；登记处 FIXED_NOW 由
                # make_fixture 默认提供，豁免同证）
                "src/test_support/mod.rs": "\n".join(
                    [
                        fixed_now_decl(FIXTURE_NOW),
                        "fn t() { db::open_in_memory(); db::init_db(); }",
                        'fn seed() { let _ = "INSERT INTO accounts (id) VALUES (1)"; }',
                    ]
                ),
                # db 产品代码：开库两行序合法（产品开库不属测试守门；无 cfg(test) 块不扫）
                "src/db/mod.rs": "fn open_db() { crate::db::open_in_memory(); crate::db::init_db(); }",
                # 域薄皮：种子表 SQL 合法（准入规则，单域特有种子长期留薄皮）
                "src/ledger/tests/common.rs": (
                    'fn seed_local() { let _ = "INSERT INTO accounts (id) VALUES (1)"; }'
                ),
                # 域时刻字面量（时间推进是行为输入）：不是 FIXED_NOW 值，合法
                "src/ledger/tests/trend.rs": 'fn t() { let at = "2026-01-15T12:00:00Z"; }',
                # 注释里的形态不计数（掩码后匹配）
                "src/ledger/tests/commented.rs": (
                    "// db::open_in_memory() 与 INSERT INTO instruments 已迁工厂"
                ),
            },
        )
        r = run((dir_path,))
        assert r.status == 0
        assert "Rust 测试守门通过" in r.output

    def test_product_file_only_cfg_test_block_scanned(self, tmp_path: Path) -> None:
        # 产品文件只扫内联 #[cfg(test)] 块：块内违规命中，产品本体同形态不误报
        dir_path = make_fixture(
            tmp_path,
            {
                "src/ledger/core.rs": "\n".join(
                    [
                        "fn open_db() { crate::db::open_in_memory(); crate::db::init_db(); }",
                        'fn seed() { let _ = "INSERT INTO instruments (id) VALUES (1)"; }',
                        f'const T0: &str = "{FIXTURE_NOW}";',
                        "",
                        "#[cfg(test)]",
                        "mod tests {",
                        "  #[test]",
                        "  fn t() {",
                        "    let mut c = crate::db::open_in_memory();",
                        "    crate::db::init_db(&mut c);",
                        '    let _ = r#"INSERT INTO instruments"#;',
                        f'    let stamp = "{FIXTURE_NOW}";',
                        "  }",
                        "}",
                    ]
                ),
            },
        )
        r = run((dir_path,))
        assert r.status == 1
        assert "src/ledger/core.rs" in r.output
        # 计数全部来自 cfg(test) 块内：产品本体的同形态（1+1 建库、1 裸 SQL、1 字面量）未计入
        assert "规则 1（直连建库）命中 2 处" in r.output
        assert "规则 2（夹具裸SQL）命中 1 处" in r.output
        assert "规则 3（默认时刻字面量）命中 1 处" in r.output

    def test_e2e_exception_removed_pure_ban(self, tmp_path: Path) -> None:
        # tests/e2e 例外机制已随 BDD 退役（#2083）：e2e 目录同形态按纯禁令红
        # BDD e2e 随旧壳退役后，例外表与「规则 1 不辖 e2e」分层豁免一并移除：
        # e2e 形态路径（若复现）与其他测试路径同判——纯禁令无例外。
        violation = "\n".join(
            [
                "fn t() {",
                "  let mut c = crate::db::open_in_memory();",
                "  crate::db::init_db(&mut c);",
                '  let _ = "INSERT INTO accounts (id) VALUES (1)";',
                "}",
            ]
        )
        dir_path = make_fixture(tmp_path, {"tests/e2e/steps.rs": violation})
        r = run((dir_path,))
        assert r.status == 1
        assert "tests/e2e/steps.rs" in r.output
        assert "规则 1（直连建库）命中 2 处" in r.output
        assert "规则 2（夹具裸SQL）命中 1 处" in r.output
        assert "未登记例外" not in r.output

    def test_rule3_values_follow_registry_single_source(self, tmp_path: Path) -> None:
        # 规则 3 时刻值以登记处为单一来源（#1468）：登记处改值禁令自动跟随
        # 登记处换上新值后：新值字面量即红——禁令跟住登记处现值，而非脚本内快照
        revised = make_fixture(
            tmp_path,
            {"src/ledger/tests.rs": f'fn t() {{ let stamp = "{FIXTURE_NOW_REVISED}"; }}'},
            fixed_now_decl(FIXTURE_NOW_REVISED),
        )
        red = run((revised,))
        assert red.status == 1
        assert f"禁用时刻值：{FIXTURE_NOW_REVISED}" in red.output
        assert "规则 3（默认时刻字面量）命中 1 处" in red.output
        # 旧现值字面量不再被禁：禁令清单与登记处严格全等，不超集化保留历史值
        # （「改值后把历史值一并禁掉」的放大式改法在此变红；删除提取退回快照由
        # 上侧红用例兜住）
        stale = make_fixture(
            tmp_path,
            {"src/ledger/tests.rs": f'fn t() {{ let stamp = "{FIXTURE_NOW}"; }}'},
            fixed_now_decl(FIXTURE_NOW_REVISED),
        )
        green = run((stale,))
        assert green.status == 0
        assert "Rust 测试守门通过" in green.output

    def test_registry_missing_or_drained_fail_loud(self, tmp_path: Path) -> None:
        # 固定时刻登记处缺失或提不出现值即 fail loud（#1468）：禁令不静默失效
        missing = make_fixture(tmp_path, {}, registry=None)
        r1 = run((missing,))
        assert r1.status == 1
        assert "固定时刻登记处缺失" in r1.output
        drained = make_fixture(
            tmp_path, {}, registry="// 登记处已搬空：没有任何 pub const 声明可提取"
        )
        r2 = run((drained,))
        assert r2.status == 1
        assert "提不出任何" in r2.output
