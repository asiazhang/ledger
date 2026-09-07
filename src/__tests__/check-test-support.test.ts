import { afterAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { judge, WHITELIST, type GateWhitelistEntry } from '../../scripts/check-test-support.ts'

// 被测对象是仓库工具脚本 scripts/check-test-support.ts（Rust 测试守门，issue #752 / ADR-0084 决策 8）。
// 脚本以 Bun 运行时执行（ADR-0083）：spawnSync('bun') 与门槛调用同款，测的就是门槛路径。
// 按测试决策只测外部可观察结果——进程退出码与输出，通过位置参数把扫描目标指向
// 临时夹具目录（形状同构 src-tauri：src/ + tests/）。
// 例外（check-structure.test.ts 先例，单一事实源）：白名单判定机制经静态导入的
// judge / WHITELIST 直接断言——白名单是脚本内常量，进程接缝无法注入变更。
// （vitest 转换后 import.meta.url 非 file: scheme，取进程 cwd = 仓库根定位脚本）
const script = join(process.cwd(), 'scripts', 'check-test-support.ts')

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

/** 工厂种子登记处夹具：禁用表集合自 INSERT INTO 提取（单一事实源的形状同构）。 */
const SEED_RS = [
  '// 测试夹具：统一测试数据库工厂种子登记处（形状同构 src/test_support/seed.rs）',
  'pub const FIXED_NOW: &str = "2026-01-01T00:00:00Z";',
  'pub fn open() { let _ = "INSERT INTO accounts"; }',
  'pub fn seed_instrument() { let _ = "INSERT INTO instruments"; }',
].join('\n')

/** 建夹具目录（形状同构 src-tauri：src/test_support/seed.rs 必在）。返回目录路径。 */
function makeFixture(files: Record<string, string>): string {
  const dir = mkdtempSync(join(tmpdir(), 'check-test-support-'))
  tempDirs.push(dir)
  mkdirSync(join(dir, 'src', 'test_support'), { recursive: true })
  writeFileSync(join(dir, 'src', 'test_support', 'seed.rs'), SEED_RS)
  for (const [name, content] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, name)), { recursive: true })
    writeFileSync(join(dir, name), content)
  }
  return dir
}

describe('check-test-support（Rust 测试守门）', () => {
  it('默认扫描本仓（门槛路径）：白名单覆盖当前存量，全绿', () => {
    const r = run()
    expect(r.status).toBe(0)
    expect(r.output).toContain('Rust 测试守门通过')
    expect(r.output).toContain('禁用种子表 5 张')
  })

  it('三条规则违规样本全部命中：直连建库、夹具裸 SQL、默认时刻字面量', () => {
    const dir = makeFixture({
      'src/ledger/tests.rs': [
        'use crate::db::{init_db, open_in_memory};',
        'fn t() {',
        '  let mut c = open_in_memory().unwrap();',
        '  init_db(&mut c).unwrap();',
        '  let sql = "INSERT INTO accounts (id) VALUES (1)";',
        '  let stamp = "2026-01-01T00:00:00Z";',
        '}',
      ].join('\n'),
      // 平行建库入口：DbState::open_in_memory 内部即两行序，同样命中（裸标识符匹配）
      'src/ledger/tests/state.rs': 'fn t() { let s = DbState::open_in_memory().unwrap(); }',
      // 顶屋 tests/ 子目录下的共享层：文件名 common.rs 但父目录非 tests，薄皮豁免不适用
      'tests/api/common.rs': 'fn seed() { let _ = "INSERT INTO instruments (id) VALUES (1)"; }',
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('src/ledger/tests.rs')
    expect(r.output).toContain('规则 1（直连建库）命中 4 处，白名单声明 0 处')
    expect(r.output).toContain('规则 2（夹具裸SQL）命中 1 处，白名单声明 0 处')
    expect(r.output).toContain('规则 3（默认时刻字面量）命中 1 处，白名单声明 0 处')
    expect(r.output).toContain('src/ledger/tests/state.rs')
    expect(r.output).toContain('tests/api/common.rs')
  })

  it('合法形态不误报：工厂本体、域薄皮种子 SQL、域时刻字面量、db 产品代码', () => {
    const dir = makeFixture({
      // 工厂本体：建库 + 种子 SQL + FIXED_NOW 全部合法（三条规则豁免）
      'src/test_support/mod.rs': [
        'pub const FIXED_NOW: &str = "2026-01-01T00:00:00Z";',
        'fn t() { db::open_in_memory(); db::init_db(); }',
        'fn seed() { let _ = "INSERT INTO accounts (id) VALUES (1)"; }',
      ].join('\n'),
      // db 产品代码：开库两行序合法（产品开库不属测试守门；无 cfg(test) 块不扫）
      'src/db/mod.rs': 'fn open_db() { crate::db::open_in_memory(); crate::db::init_db(); }',
      // 域薄皮：种子表 SQL 合法（准入规则，单域特有种子长期留薄皮）
      'src/ledger/tests/common.rs': 'fn seed_local() { let _ = "INSERT INTO accounts (id) VALUES (1)"; }',
      // 域时刻字面量（时间推进是行为输入）：不是 FIXED_NOW 值，合法
      'src/ledger/tests/trend.rs': 'fn t() { let at = "2026-01-15T12:00:00Z"; }',
      // 注释里的形态不计数（掩码后匹配）
      'src/ledger/tests/commented.rs': '// db::open_in_memory() 与 INSERT INTO instruments 已迁工厂',
    })
    const r = run([dir])
    expect(r.status).toBe(0)
    expect(r.output).toContain('Rust 测试守门通过')
  })

  it('产品文件只扫内联 #[cfg(test)] 块：块内违规命中，产品本体同形态不误报', () => {
    const dir = makeFixture({
      'src/ledger/core.rs': [
        'fn open_db() { crate::db::open_in_memory(); crate::db::init_db(); }',
        'fn seed() { let _ = "INSERT INTO instruments (id) VALUES (1)"; }',
        'const T0: &str = "2026-01-01T00:00:00Z";',
        '',
        '#[cfg(test)]',
        'mod tests {',
        '  #[test]',
        '  fn t() {',
        '    let mut c = crate::db::open_in_memory();',
        '    crate::db::init_db(&mut c);',
        '    let _ = r#"INSERT INTO instruments"#;',
        '    let stamp = "2026-01-01T00:00:00Z";',
        '  }',
        '}',
      ].join('\n'),
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('src/ledger/core.rs')
    // 计数全部来自 cfg(test) 块内：产品本体的同形态（1+1 建库、1 裸 SQL、1 字面量）未计入
    expect(r.output).toContain('规则 1（直连建库）命中 2 处，白名单声明 0 处')
    expect(r.output).toContain('规则 2（夹具裸SQL）命中 1 处，白名单声明 0 处')
    expect(r.output).toContain('规则 3（默认时刻字面量）命中 1 处，白名单声明 0 处')
  })

  describe('白名单判定机制（judge，严格相等）', () => {
    const counts = (perRule: Partial<Record<1 | 2 | 3, number>>): Map<string, Map<1 | 2 | 3, number>> =>
      new Map([['src/ledger/tests.rs', new Map(Object.entries(perRule).map(([k, v]) => [Number(k) as 1 | 2 | 3, v!]))]])
    const hitsOf = (rule: 1 | 2 | 3, n: number) =>
      Array.from({ length: n }, (_, i) => ({ rel: 'src/ledger/tests.rs', rule, line: 10 + i }))
    const entry = (e: Omit<GateWhitelistEntry, 'note'>): GateWhitelistEntry[] => [{ note: '夹具', ...e }]

    it('命中超出声明（未入白名单/超出基线）红', () => {
      const problems = judge(counts({ 1: 2 }), hitsOf(1, 2), entry({ file: 'src/ledger/tests.rs', r1: 1 }))
      expect(problems).toHaveLength(1)
      expect(problems[0]).toContain('命中 2 处，白名单声明 1 处——超出基线')
      expect(judge(counts({ 1: 1 }), hitsOf(1, 1), [])[0]).toContain('未入白名单')
    })

    it('声明超出命中（基线过期）红——迁移票必须同步缩减基线', () => {
      const problems = judge(counts({ 2: 1 }), hitsOf(2, 1), entry({ file: 'src/ledger/tests.rs', r2: 3 }))
      expect(problems).toHaveLength(1)
      expect(problems[0]).toContain('基线过期：声明 3 处、实际 1 处')
    })

    it('恰好相等绿；条目文件消失/基线过期红；全字段为零条目红', () => {
      expect(judge(counts({ 1: 2 }), hitsOf(1, 2), entry({ file: 'src/ledger/tests.rs', r1: 2 }))).toEqual([])
      // 条目文件已无命中：条目过期 + 基线过期两处提示
      const stale = judge(new Map(), [], entry({ file: 'src/ledger/tests.rs', r1: 2 }))
      expect(stale).toHaveLength(2)
      expect(stale[0]).toContain('已无任何命中——基线过期，请移除条目')
      expect(stale[1]).toContain('基线过期：声明 2 处、实际 0 处')
      expect(judge(new Map(), [], entry({ file: 'src/ledger/tests.rs' }))[0]).toContain('全字段为零')
    })

    it('本仓白名单条目形状合法（每条至少一个规则计数为正，文件路径相对 src-tauri）', () => {
      for (const e of WHITELIST) {
        expect((e.r1 ?? 0) + (e.r2 ?? 0) + (e.r3 ?? 0)).toBeGreaterThan(0)
        expect(e.file).toMatch(/^(src|tests)\//)
      }
    })
  })
})
