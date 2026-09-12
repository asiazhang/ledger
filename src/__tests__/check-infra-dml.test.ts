import { afterAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'

// 被测对象是仓库工具脚本 scripts/check-infra-dml.ts（基础设施账本数据表 DML 禁令，
// issue #1135 / ADR-0111 决策 1、决策 5）。
// 脚本以 Bun 运行时执行（ADR-0083）：spawnSync('bun') 与门槛调用同款，测的就是门槛路径。
// 按测试决策只测外部可观察结果——进程退出码与输出，通过位置参数把扫描目标指向
// 临时夹具目录（形状同构 src-tauri：migrations/ + crates/infra/src/）。
// （vitest 转换后 import.meta.url 非 file: scheme，取进程 cwd = 仓库根定位脚本）
const script = join(process.cwd(), 'scripts', 'check-infra-dml.ts')

interface RunResult {
  status: number
  output: string
}

function run(args: string[] = []): RunResult {
  const r = spawnSync('bun', [script, ...args], { encoding: 'utf8' })
  return { status: r.status ?? -1, output: (r.stdout ?? '') + (r.stderr ?? '') }
}

const tempDirs: string[] = []
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true })
})

/** 迁移 SQL 夹具：账本数据表 + 机制表（形状同构 src-tauri/migrations 的 CREATE TABLE 清单）。 */
const MIGRATION_SQL = [
  '-- 测试夹具：迁移链（形状同构 src-tauri/migrations）',
  'CREATE TABLE transactions (id TEXT PRIMARY KEY, amount INTEGER);',
  'CREATE TABLE accounts (id TEXT PRIMARY KEY, balance INTEGER);',
  'CREATE TABLE budgets (id TEXT PRIMARY KEY);',
  'CREATE TABLE categories (id TEXT PRIMARY KEY);',
  'CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT);',
  'CREATE TABLE sync_ops (id TEXT PRIMARY KEY);',
  'CREATE TABLE sync_device (id TEXT PRIMARY KEY);',
  'CREATE TABLE sync_parked_ops (id TEXT PRIMARY KEY);',
  'CREATE TABLE sync_stream_positions (id TEXT PRIMARY KEY);',
].join('\n')

/** 已登记例外文件的夹具内容：内联 cfg(test) 夹具恰 1 处命中（= 登记数）。 */
const WRITE_ENTRY_VIOLATION = [
  '#[cfg(test)]',
  'mod tests {',
  '  #[test]',
  '  fn t() {',
  '    let _ = "INSERT INTO categories (id) VALUES (1)";',
  '  }',
  '}',
].join('\n')

/** 形状同构默认文件：免扫文件与例外文件必在场，缺失即清单漂移红。 */
const DEFAULT_INFRA_FILES: Record<string, string> = {
  'crates/infra/src/db/migrate.rs': 'pub fn migrate() {}',
  'crates/infra/src/db/schema_guard.rs': 'pub fn guard() {}',
  'crates/infra/src/write_entry.rs': WRITE_ENTRY_VIOLATION,
}

/** 建夹具目录（形状同构 src-tauri：migrations/ 与免扫文件必在；值为 null 表示
 *  刻意省略该默认文件——清单漂移负向样本用）。返回目录路径。 */
function makeFixture(
  files: Record<string, string | null>,
  migrationSql: string = MIGRATION_SQL,
): string {
  const dir = mkdtempSync(join(tmpdir(), 'check-infra-dml-'))
  tempDirs.push(dir)
  mkdirSync(join(dir, 'migrations'), { recursive: true })
  writeFileSync(join(dir, 'migrations', 'V001__initial.sql'), migrationSql)
  const all: Record<string, string | null> = { ...DEFAULT_INFRA_FILES, ...files }
  for (const [name, content] of Object.entries(all)) {
    if (content === null) continue
    mkdirSync(dirname(join(dir, name)), { recursive: true })
    writeFileSync(join(dir, name), content)
  }
  return dir
}

describe('check-infra-dml（基础设施账本数据表 DML 禁令，issue #1135 / ADR-0111）', () => {
  it('默认扫描本仓（门槛路径）：存量零生产命中全绿，免扫与例外明示', () => {
    const r = run()
    expect(r.status).toBe(0)
    expect(r.output).toContain('基础设施账本数据表 DML 禁令通过')
    // 免扫范围不靠沉默放行：成功输出逐条列出免扫文件与理由
    expect(r.output).toContain('db/migrate.rs')
    expect(r.output).toContain('db/schema_guard.rs')
    // 已登记例外（write_entry.rs 内联测试夹具）明示
    expect(r.output).toContain('已登记例外 1 条')
  })

  it('负向判据：基础设施生产代码对账本数据表的 DML 即红并定位到 文件:行', () => {
    const dir = makeFixture({
      'crates/infra/src/boot/rogue.rs': [
        '//! 越权模块：基础设施直接写账本数据表（应被守门拦下）',
        'pub fn rogue(conn: &rusqlite::Connection) {',
        "  conn.execute(\"INSERT INTO transactions (id, amount) VALUES ('t-1', 100)\", []).unwrap();",
        "  conn.execute('UPDATE accounts SET balance = balance - 1', []).unwrap();",
        "  conn.execute('DELETE FROM budgets', []).unwrap();",
        "  conn.execute('REPLACE INTO accounts VALUES (1)', []).unwrap();",
        '}',
      ].join('\n'),
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    // 定位到 文件:行（INSERT 在第 3 行）
    expect(r.output).toContain('crates/infra/src/boot/rogue.rs:3')
    expect(r.output).toContain('INSERT INTO transactions')
    expect(r.output).toContain('UPDATE accounts')
    expect(r.output).toContain('DELETE FROM budgets')
    expect(r.output).toContain('REPLACE INTO accounts')
    expect(r.output).toContain('ADR-0111')
  })

  it('机制表（app_settings / sync_ops）放行：登记理由明示，不误报', () => {
    const dir = makeFixture({
      'crates/infra/src/settings.rs': [
        'pub fn put(conn: &rusqlite::Connection) {',
        "  conn.execute('INSERT INTO app_settings(key, value) VALUES(?1, ?2)', []).unwrap();",
        '}',
      ].join('\n'),
      'crates/infra/src/db/rogue_sync.rs': "pub fn x(c: &rusqlite::Connection) { let _ = c.execute('DELETE FROM sync_ops', []); }",
    })
    const r = run([dir])
    expect(r.status).toBe(0)
    expect(r.output).toContain('基础设施账本数据表 DML 禁令通过')
    expect(r.output).toContain('app_settings')
    expect(r.output).not.toContain('settings.rs')
  })

  it('库名前缀限定（main.foo）不逃逸：建表入清单、DML 照样命中', () => {
    const dir = makeFixture(
      {
        'crates/infra/src/db/rogue_qualified.rs':
          "pub fn x(c: &rusqlite::Connection) { let _ = c.execute('DELETE FROM main.plugin_cache', []); }",
      },
      MIGRATION_SQL + '\nCREATE TABLE main.plugin_cache (key TEXT PRIMARY KEY);',
    )
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('DELETE FROM main.plugin_cache')
  })

  it('默认拒绝：迁移链新表未登记机制表即按账本数据对待，DML 即红（防新表绕过）', () => {
    const dir = makeFixture(
      {
        'crates/infra/src/db/rogue_cache.rs':
          "pub fn x(c: &rusqlite::Connection) { let _ = c.execute(\"INSERT INTO plugin_cache VALUES ('k')\", []); }",
      },
      MIGRATION_SQL + '\nCREATE TABLE plugin_cache (key TEXT PRIMARY KEY);',
    )
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('plugin_cache')
    expect(r.output).toContain('crates/infra/src/db/rogue_cache.rs')
  })

  it('免扫范围（迁移链 / schema 守卫）按职责放行；免扫文件缺失即红（清单漂移 fail loud）', () => {
    const dir = makeFixture({
      // 免扫文件内含账本表 DML：迁移与守卫职责所在，放行但明示
      'crates/infra/src/db/migrate.rs': 'pub fn seed() { let _ = "UPDATE transactions SET amount = 0"; }',
      'crates/infra/src/db/schema_guard.rs': 'pub fn g() { let _ = "DELETE FROM accounts"; }',
    })
    const ok = run([dir])
    expect(ok.status).toBe(0)
    expect(ok.output).toContain('免扫')

    // 免扫文件消失：清单漂移 fail loud，不放任守门空转
    const drifted = makeFixture({
      'crates/infra/src/db/migrate.rs': 'pub fn seed() { let _ = "UPDATE transactions SET amount = 0"; }',
      'crates/infra/src/db/schema_guard.rs': null,
    })
    const bad = run([drifted])
    expect(bad.status).toBe(1)
    expect(bad.output).toContain('schema_guard.rs')
    expect(bad.output).toContain('免扫清单漂移')
  })

  it('内联 #[cfg(test)] 不豁免（与守门家族一致）：块内账本表 DML 即红', () => {
    const dir = makeFixture({
      'crates/infra/src/db/conn_extra.rs': [
        'pub fn open() {}',
        '',
        '#[cfg(test)]',
        'mod tests {',
        '  #[test]',
        '  fn t() {',
        '    let _ = "INSERT INTO categories (id) VALUES (1)";',
        '  }',
        '}',
      ].join('\n'),
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('crates/infra/src/db/conn_extra.rs:7')
    expect(r.output).toContain('INSERT INTO categories')
  })

  it('已登记例外（write_entry.rs 内联测试夹具）严格相等校验：命中数漂移即红、命中清零即红', () => {
    const violation = [
      '#[cfg(test)]',
      'mod tests {',
      '  #[test]',
      '  fn t() {',
      '    let _ = "INSERT INTO categories (id) VALUES (1)";',
      '  }',
      '}',
    ].join('\n')
    // 恰好 1 处命中 = 登记数：放行
    const exact = makeFixture({ 'crates/infra/src/write_entry.rs': violation })
    expect(run([exact]).status).toBe(0)
    expect(run([exact]).output).toContain('已登记例外 1 条')

    // 登记文件消失（改名/搬迁）：清单漂移 fail loud（与 EXEMPT_FILES 同纪律）
    const renamed = makeFixture({ 'crates/infra/src/write_entry.rs': null })
    const missing = run([renamed])
    expect(missing.status).toBe(1)
    expect(missing.output).toContain('例外清单漂移')

    // 2 处命中 ≠ 登记的 1 处：红
    const drifted = makeFixture({
      'crates/infra/src/write_entry.rs': violation + '\n' + violation,
    })
    const more = run([drifted])
    expect(more.status).toBe(1)
    expect(more.output).toContain('实际命中 2 处 ≠ 登记的 1 处')

    // 命中清零：例外已收敛，登记条目应删除，红
    const converged = makeFixture({ 'crates/infra/src/write_entry.rs': 'pub fn x() {}' })
    const gone = run([converged])
    expect(gone.status).toBe(1)
    expect(gone.output).toContain('例外已收敛')
  })

  it('外挂测试豁免（ADR-0056 决策 5）：tests.rs 文件与 tests/ 目录的夹具 SQL 不辖', () => {
    const dir = makeFixture({
      'crates/infra/src/db/tests/fixture.rs': 'fn t() { let _ = "INSERT INTO transactions VALUES (1)"; }',
      'crates/infra/src/boot/tests.rs': 'fn t() { let _ = "DELETE FROM accounts"; }',
    })
    const r = run([dir])
    expect(r.status).toBe(0)
    expect(r.output).toContain('基础设施账本数据表 DML 禁令通过')
  })

  it('迁移链提不出任何 CREATE TABLE 即红（拒绝以空集假绿通过）', () => {
    const dir = makeFixture({}, '-- 空迁移夹具')
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('CREATE TABLE')
  })
})
