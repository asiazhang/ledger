# 被测对象是仓库工具脚本 scripts/check_infra_dml.py（基础设施账本数据表 DML 禁令，
# issue #1135 / ADR-0111 决策 1、决策 5）。
# 脚本以 uv 运行时执行（ADR-0083 修订注记，#2174）：run_gate_script 以
# subprocess('uv run') 与门槛调用同款拉起，测的就是门槛路径。
# 按测试决策只测外部可观察结果——进程退出码与输出，通过位置参数把扫描目标指向
# 临时夹具目录（形状同构 src-app：migrations/ + crates/infra/src/）。

from __future__ import annotations

import os
from pathlib import Path

import pytest

from gate_script_test_helper import RunResult, gate_script, run_gate_script

SCRIPT = gate_script("check_infra_dml.py")


def run(args: tuple[str, ...] = ()) -> RunResult:
    return run_gate_script(SCRIPT, args)


# 迁移 SQL 夹具：账本数据表 + 机制表（形状同构 src-app/migrations 的 CREATE TABLE 清单）。
MIGRATION_SQL = "\n".join(
    [
        "-- 测试夹具：迁移链（形状同构 src-app/migrations）",
        "CREATE TABLE transactions (id TEXT PRIMARY KEY, amount INTEGER);",
        "CREATE TABLE accounts (id TEXT PRIMARY KEY, balance INTEGER);",
        "CREATE TABLE budgets (id TEXT PRIMARY KEY);",
        "CREATE TABLE categories (id TEXT PRIMARY KEY);",
        "CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT);",
        "CREATE TABLE sync_ops (id TEXT PRIMARY KEY);",
        "CREATE TABLE sync_device (id TEXT PRIMARY KEY);",
        "CREATE TABLE sync_parked_ops (id TEXT PRIMARY KEY);",
        "CREATE TABLE sync_stream_positions (id TEXT PRIMARY KEY);",
    ]
)

# 形状同构默认文件：免扫文件必在场，缺失即清单漂移红。
# （已登记例外清单自 #1108 起为空——shell_support 随壳机制迁出根包，
# 例外随迁退役，夹具不再需要例外文件在场。）
DEFAULT_INFRA_FILES: dict[str, str | None] = {
    "crates/infra/src/db/migrate.rs": "pub fn migrate() {}",
    "crates/infra/src/db/schema_guard.rs": "pub fn guard() {}",
}


# 建夹具目录（形状同构 src-app：migrations/ 与免扫文件必在；值为 None 表示
# 刻意省略该默认文件——清单漂移负向样本用）。返回目录路径。
def make_fixture(
    root: Path,
    files: dict[str, str | None] | None = None,
    migration_sql: str = MIGRATION_SQL,
) -> str:
    os.makedirs(os.path.join(str(root), "migrations"), exist_ok=True)
    Path(root, "migrations", "V001__initial.sql").write_text(migration_sql, encoding="utf-8")
    merged: dict[str, str | None] = {**DEFAULT_INFRA_FILES, **(files or {})}
    for name, content in merged.items():
        if content is None:
            continue
        abs_path = os.path.join(str(root), name)
        os.makedirs(os.path.dirname(abs_path), exist_ok=True)
        Path(abs_path).write_text(content, encoding="utf-8")
    return str(root)


class TestCheckInfraDml基础设施账本数据表DML禁令:
    def test_default_repo_scan_all_green_with_exempt_and_exceptions_listed(self) -> None:
        # 默认扫描本仓（门槛路径）：存量零生产命中全绿，免扫与例外明示
        r = run()
        assert r.status == 0
        assert "基础设施账本数据表 DML 禁令通过" in r.output
        # 免扫范围不靠沉默放行：成功输出逐条列出免扫文件与理由
        assert "db/migrate.rs" in r.output
        assert "db/schema_guard.rs" in r.output
        # 已登记例外明示：#1108 起清单为空（shell_support 迁出根包，例外随迁退役）
        assert "已登记例外 0 条" in r.output

    def test_ledger_table_dml_in_production_is_red_with_file_line(self, tmp_path: Path) -> None:
        # 负向判据：基础设施生产代码对账本数据表的 DML 即红并定位到 文件:行
        dir_ = tmp_path / "rogue-fixture"
        dir_.mkdir()
        make_fixture(
            dir_,
            {
                "crates/infra/src/boot/rogue.rs": "\n".join(
                    [
                        "//! 越权模块：基础设施直接写账本数据表（应被守门拦下）",
                        "pub fn rogue(conn: &rusqlite::Connection) {",
                        '  conn.execute("INSERT INTO transactions (id, amount) VALUES (\'t-1\', 100)", []).unwrap();',
                        "  conn.execute('UPDATE accounts SET balance = balance - 1', []).unwrap();",
                        "  conn.execute('DELETE FROM budgets', []).unwrap();",
                        "  conn.execute('REPLACE INTO accounts VALUES (1)', []).unwrap();",
                        "}",
                    ]
                ),
            },
        )
        r = run((str(dir_),))
        assert r.status == 1
        # 定位到 文件:行（INSERT 在第 3 行）
        assert "crates/infra/src/boot/rogue.rs:3" in r.output
        assert "INSERT INTO transactions" in r.output
        assert "UPDATE accounts" in r.output
        assert "DELETE FROM budgets" in r.output
        assert "REPLACE INTO accounts" in r.output
        assert "ADR-0111" in r.output

    def test_mechanism_tables_allowed_and_listed(self, tmp_path: Path) -> None:
        # 机制表（app_settings / sync_ops）放行：登记理由明示，不误报
        dir_ = tmp_path / "mechanism-fixture"
        dir_.mkdir()
        make_fixture(
            dir_,
            {
                "crates/infra/src/settings.rs": "\n".join(
                    [
                        "pub fn put(conn: &rusqlite::Connection) {",
                        "  conn.execute('INSERT INTO app_settings(key, value) VALUES(?1, ?2)', []).unwrap();",
                        "}",
                    ]
                ),
                "crates/infra/src/db/rogue_sync.rs": "pub fn x(c: &rusqlite::Connection) { let _ = c.execute('DELETE FROM sync_ops', []); }",
            },
        )
        r = run((str(dir_),))
        assert r.status == 0
        assert "基础设施账本数据表 DML 禁令通过" in r.output
        assert "app_settings" in r.output
        assert "settings.rs" not in r.output

    def test_qualified_table_name_does_not_escape(self, tmp_path: Path) -> None:
        # 库名前缀限定（main.foo）不逃逸：建表入清单、DML 照样命中
        dir_ = tmp_path / "qualified-fixture"
        dir_.mkdir()
        make_fixture(
            dir_,
            {
                "crates/infra/src/db/rogue_qualified.rs": "pub fn x(c: &rusqlite::Connection) { let _ = c.execute('DELETE FROM main.plugin_cache', []); }",
            },
            MIGRATION_SQL + "\nCREATE TABLE main.plugin_cache (key TEXT PRIMARY KEY);",
        )
        r = run((str(dir_),))
        assert r.status == 1
        assert "DELETE FROM main.plugin_cache" in r.output

    def test_new_table_defaults_to_banned_no_bypass(self, tmp_path: Path) -> None:
        # 默认拒绝：迁移链新表未登记机制表即按账本数据对待，DML 即红（防新表绕过）
        dir_ = tmp_path / "new-table-fixture"
        dir_.mkdir()
        make_fixture(
            dir_,
            {
                "crates/infra/src/db/rogue_cache.rs": "pub fn x(c: &rusqlite::Connection) { let _ = c.execute(\"INSERT INTO plugin_cache VALUES ('k')\", []); }",
            },
            MIGRATION_SQL + "\nCREATE TABLE plugin_cache (key TEXT PRIMARY KEY);",
        )
        r = run((str(dir_),))
        assert r.status == 1
        assert "plugin_cache" in r.output
        assert "crates/infra/src/db/rogue_cache.rs" in r.output

    def test_exempt_files_allowed_but_missing_exempt_file_is_red(self, tmp_path: Path) -> None:
        # 免扫范围（迁移链 / schema 守卫）按职责放行；免扫文件缺失即红（清单漂移 fail loud）
        ok_dir = tmp_path / "exempt-ok"
        ok_dir.mkdir()
        make_fixture(
            ok_dir,
            {
                # 免扫文件内含账本表 DML：迁移与守卫职责所在，放行但明示
                "crates/infra/src/db/migrate.rs": 'pub fn seed() { let _ = "UPDATE transactions SET amount = 0"; }',
                "crates/infra/src/db/schema_guard.rs": 'pub fn g() { let _ = "DELETE FROM accounts"; }',
            },
        )
        ok = run((str(ok_dir),))
        assert ok.status == 0
        assert "免扫" in ok.output

        # 免扫文件消失：清单漂移 fail loud，不放任守门空转
        drifted = tmp_path / "exempt-drift"
        drifted.mkdir()
        make_fixture(
            drifted,
            {
                "crates/infra/src/db/migrate.rs": 'pub fn seed() { let _ = "UPDATE transactions SET amount = 0"; }',
                "crates/infra/src/db/schema_guard.rs": None,
            },
        )
        bad = run((str(drifted),))
        assert bad.status == 1
        assert "schema_guard.rs" in bad.output
        assert "免扫清单漂移" in bad.output

    def test_inline_cfg_test_block_not_exempt(self, tmp_path: Path) -> None:
        # 内联 #[cfg(test)] 不豁免（与守门家族一致）：块内账本表 DML 即红
        dir_ = tmp_path / "inline-cfg-fixture"
        dir_.mkdir()
        make_fixture(
            dir_,
            {
                "crates/infra/src/db/conn_extra.rs": "\n".join(
                    [
                        "pub fn open() {}",
                        "",
                        "#[cfg(test)]",
                        "mod tests {",
                        "  #[test]",
                        "  fn t() {",
                        '    let _ = "INSERT INTO categories (id) VALUES (1)";',
                        "  }",
                        "}",
                    ]
                ),
            },
        )
        r = run((str(dir_),))
        assert r.status == 1
        assert "crates/infra/src/db/conn_extra.rs:7" in r.output
        assert "INSERT INTO categories" in r.output

    def test_external_test_files_exempt(self, tmp_path: Path) -> None:
        # 外挂测试豁免（ADR-0056 决策 5）：tests.rs 文件与 tests/ 目录的夹具 SQL 不辖
        dir_ = tmp_path / "external-tests-fixture"
        dir_.mkdir()
        make_fixture(
            dir_,
            {
                "crates/infra/src/db/tests/fixture.rs": 'fn t() { let _ = "INSERT INTO transactions VALUES (1)"; }',
                "crates/infra/src/boot/tests.rs": 'fn t() { let _ = "DELETE FROM accounts"; }',
            },
        )
        r = run((str(dir_),))
        assert r.status == 0
        assert "基础设施账本数据表 DML 禁令通过" in r.output

    def test_no_create_table_in_migrations_is_red(self, tmp_path: Path) -> None:
        # 迁移链提不出任何 CREATE TABLE 即红（拒绝以空集假绿通过）
        dir_ = tmp_path / "empty-migrations-fixture"
        dir_.mkdir()
        make_fixture(dir_, {}, "-- 空迁移夹具")
        r = run((str(dir_),))
        assert r.status == 1
        assert "CREATE TABLE" in r.output
