import { afterAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

// 被测对象是仓库工具脚本 scripts/check-i18n-keys.ts（i18n key 全等校验门槛）。
// 脚本以 Bun 运行时执行（ADR-0083）：spawnSync('bun') 与门槛调用同款，测的就是门槛路径。
// 按测试决策只测外部可观察结果——进程退出码与输出，不测内部函数；
// 通过位置参数把扫描目标指向临时夹具目录（仿 check-commands.test.ts 先例）。
// 注意：目录名即域前缀（common.json 内层不再重复域名）。
const script = join(process.cwd(), 'scripts', 'check-i18n-keys.ts')

const tmpDirs: string[] = []

function makeLocaleDir(root: string, name: string, files: Record<string, unknown>): void {
  const dir = join(root, name)
  mkdirSync(dir, { recursive: true })
  for (const [file, content] of Object.entries(files)) {
    writeFileSync(join(dir, file), JSON.stringify(content))
  }
}

function makeFixture(zhFiles: Record<string, unknown>, enFiles: Record<string, unknown>): string {
  const root = mkdtempSync(join(tmpdir(), 'i18n-keys-'))
  tmpDirs.push(root)
  makeLocaleDir(root, 'zh-CN', zhFiles)
  makeLocaleDir(root, 'en-US', enFiles)
  return root
}

function run(localesDir: string) {
  const r = spawnSync('bun', [script, localesDir], { encoding: 'utf8' })
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
