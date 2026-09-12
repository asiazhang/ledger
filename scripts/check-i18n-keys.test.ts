import { afterAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

// 被测对象是仓库工具脚本 scripts/check-i18n-keys.ts（i18n key 全等校验门槛 +
// 码化错误模板覆盖守门，issue #1188 / ADR-0050）。
// 脚本以 Bun 运行时执行（ADR-0083）：spawnSync('bun') 与门槛调用同款，测的就是门槛路径。
// 按测试决策只测外部可观察结果——进程退出码与输出，不测内部函数；
// 通过位置参数把扫描目标指向临时夹具目录（仿 check-commands.test.ts 先例）。
// 注意：目录名即域前缀（common.json 内层不再重复域名）。
// 夹具布局：<root>/locales/{zh-CN,en-US}（固定含空 errors.json，码化覆盖守门的比对对象）
// + <root>/rust（Rust 扫描根，默认无码化构造点）。
const script = join(process.cwd(), 'scripts', 'check-i18n-keys.ts')

const tmpDirs: string[] = []

function makeLocaleDir(root: string, name: string, files: Record<string, unknown>): void {
  const dir = join(root, name)
  mkdirSync(dir, { recursive: true })
  for (const [file, content] of Object.entries(files)) {
    writeFileSync(join(dir, file), JSON.stringify(content))
  }
}

interface FixtureOptions {
  rustSrc?: string
  zhErrors?: Record<string, unknown>
  enErrors?: Record<string, unknown>
}

function makeFixture(
  zhFiles: Record<string, unknown>,
  enFiles: Record<string, unknown>,
  options: FixtureOptions = {},
): string {
  const root = mkdtempSync(join(tmpdir(), 'i18n-keys-'))
  tmpDirs.push(root)
  const locales = join(root, 'locales')
  makeLocaleDir(locales, 'zh-CN', { 'errors.json': options.zhErrors ?? {}, ...zhFiles })
  makeLocaleDir(locales, 'en-US', { 'errors.json': options.enErrors ?? {}, ...enFiles })
  const rust = join(root, 'rust')
  mkdirSync(rust, { recursive: true })
  writeFileSync(join(rust, 'lib.rs'), options.rustSrc ?? '// 无码化构造点\n')
  return root
}

function run(root: string) {
  const r = spawnSync('bun', [script, join(root, 'locales'), join(root, 'rust')], {
    encoding: 'utf8',
  })
  return { status: r.status ?? -1, output: (r.stdout ?? '') + (r.stderr ?? '') }
}

afterAll(() => {
  for (const d of tmpDirs) rmSync(d, { recursive: true, force: true })
})

describe('i18n key 全等校验（check.sh 质量门槛）', () => {
  it('两语言 key 集合全等时通过', () => {
    const dir = makeFixture(
      { 'common.json': { save: '保存', nested: { ok: '确定' } }, 'tx.json': { title: '交易' } },
      { 'common.json': { save: 'Save', nested: { ok: 'OK' } }, 'tx.json': { title: 'Transactions' } },
    )
    const { status, output } = run(dir)
    expect(status).toBe(0)
    expect(output).toContain('全等')
  })

  it('源语言独有 key（漏翻）→ 失败并列出缺失项', () => {
    const dir = makeFixture(
      { 'common.json': { save: '保存', cancel: '取消' } },
      { 'common.json': { save: 'Save' } },
    )
    const { status, output } = run(dir)
    expect(status).toBe(1)
    expect(output).toContain('common.cancel')
  })

  it('其他 locale 独有 key（多余）→ 失败并列出多余项', () => {
    const dir = makeFixture(
      { 'common.json': { save: '保存' } },
      { 'common.json': { save: 'Save', extra: 'Extra' } },
    )
    const { status, output } = run(dir)
    expect(status).toBe(1)
    expect(output).toContain('common.extra')
  })

  it('多域多文件：任一域有差异即失败（双向都查）', () => {
    const dir = makeFixture(
      { 'common.json': { ok: '确定' }, 'tx.json': { a: '甲', b: '乙' } },
      { 'common.json': { ok: 'OK', zh: '多余' }, 'tx.json': { a: 'A' } },
    )
    const { status, output } = run(dir)
    expect(status).toBe(1)
    expect(output).toContain('tx.b')
    expect(output).toContain('common.zh')
  })
})

describe('码化错误模板覆盖守门（issue #1188 / ADR-0050）', () => {
  it('码化构造码在两语言 errors.json 均有模板 → 通过', () => {
    const dir = makeFixture(
      { 'common.json': { save: '保存' } },
      { 'common.json': { save: 'Save' } },
      {
        rustSrc: `fn f() { AppError::coded("fx.rate-missing", "缺少汇率"); }\n`,
        zhErrors: { fx: { 'rate-missing': '缺少汇率' } },
        enErrors: { fx: { 'rate-missing': 'exchange rate missing' } },
      },
    )
    const { status, output } = run(dir)
    expect(status).toBe(0)
    expect(output).toContain('码化错误模板覆盖')
  })

  it('缺模板的码化构造码 → 失败并列出缺失码', () => {
    const dir = makeFixture(
      { 'common.json': { save: '保存' } },
      { 'common.json': { save: 'Save' } },
      {
        rustSrc: `fn f() { AppError::codedp_not_found("trade.detail-not-found", "交易不存在"); }\n`,
      },
    )
    const { status, output } = run(dir)
    expect(status).toBe(1)
    expect(output).toContain('trade.detail-not-found')
  })

  it('仅 en 缺模板 → 失败（双向校验）', () => {
    const dir = makeFixture(
      { 'common.json': { save: '保存' } },
      { 'common.json': { save: 'Save' } },
      {
        rustSrc: `fn f() { AppError::coded("account.not-found", "账户不存在"); }\n`,
        zhErrors: { account: { 'not-found': '账户不存在: {0}' } },
      },
    )
    const { status, output } = run(dir)
    expect(status).toBe(1)
    expect(output).toContain('account.not-found')
  })

  it('const 常量引用型构造点解析为常量值参与校验', () => {
    const dir = makeFixture(
      { 'common.json': { save: '保存' } },
      { 'common.json': { save: 'Save' } },
      {
        rustSrc: `const RATE_CODE: &str = "fx.rate-missing";\nfn f() { AppError::coded(RATE_CODE, "缺少汇率"); }\n`,
      },
    )
    const { status, output } = run(dir)
    expect(status).toBe(1)
    expect(output).toContain('fx.rate-missing')
  })

  it('注释、#[cfg(test)] 块与测试路径内的构造点不入枚举', () => {
    const dir = makeFixture(
      { 'common.json': { save: '保存' } },
      { 'common.json': { save: 'Save' } },
      {
        rustSrc: [
          '// AppError::coded("comment.only", "注释不算")',
          '/* AppError::coded("block.only", "块注释不算") */',
          '#[cfg(test)]',
          'mod tests {',
          '  fn t() { AppError::coded("testmod.only", "测试不算"); }',
          '}',
          'fn real() { AppError::coded("covered.code", "正文算"); }',
        ].join('\n'),
        zhErrors: { covered: { code: '有模板' } },
        enErrors: { covered: { code: 'covered' } },
      },
    )
    const rustTests = join(dir, 'rust', 'tests')
    mkdirSync(rustTests, { recursive: true })
    writeFileSync(join(rustTests, 'steps.rs'), 'fn t() { AppError::coded("testfile.only", "测试文件不算"); }\n')
    const { status, output } = run(dir)
    expect(status).toBe(0)
    expect(output).toContain('码化错误模板覆盖')
  })
})
