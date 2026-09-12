import { afterAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { TOAST_BASELINE } from '../scripts/check-async-guards.ts'

// 被测对象是仓库工具脚本 scripts/check-async-guards.ts（前端异步守门，issue #1039）。
// 脚本以 Bun 运行时执行（ADR-0083）：spawnSync('bun') 与门槛调用同款，测的就是门槛路径。
// 按测试决策只测外部可观察结果——进程退出码与输出，不测内部函数；
// 通过位置参数把扫描目标指向临时夹具目录。
// 夹具基线清单自助手导出的 TOAST_BASELINE 派生（单一事实源，无双源漂移）；
// （vitest 转换后 import.meta.url 非 file: scheme，取进程 cwd = 仓库根定位脚本）
const script = join(process.cwd(), 'scripts', 'check-async-guards.ts')

interface RunResult {
  status: number
  output: string
}

function run(args: string[]): RunResult {
  const r = spawnSync('bun', [script, ...args], { encoding: 'utf8' })
  return { status: r.status ?? -1, output: (r.stdout ?? '') + (r.stderr ?? '') }
}

const tempDirs: string[] = []
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true })
})

/** 规则 2 靶形态的夹具行（与守门正则同构的单行直弹 toast） */
const TOAST_LINE = "message.error(t('app.msg.saveFailed', { message: errorMessage(e) }))"

/** 规则 2 基线条目桩内容：恰好 count 行直弹 toast（.vue/.ts 行级扫描不要求语法完整） */
function toastStub(count: number): string {
  return (
    Array.from({ length: count }, () => TOAST_LINE).join('\n') + (count > 0 ? '\n' : '')
  )
}

/**
 * 建临时夹具：按脚本导出的 TOAST_BASELINE 生成全部条目（每文件恰好基线计数行），
 * 再按 overrides 追加/覆盖文件（值为完整文件内容）。返回脚本参数（夹具扫描根）。
 */
function makeFixture(overrides: Record<string, string> = {}): string[] {
  const src = mkdtempSync(join(tmpdir(), 'check-async-guards-'))
  tempDirs.push(src)
  for (const [relPath, count] of Object.entries(TOAST_BASELINE)) {
    const abs = join(src, relPath)
    mkdirSync(join(abs, '..'), { recursive: true })
    writeFileSync(abs, toastStub(count))
  }
  for (const [relPath, content] of Object.entries(overrides)) {
    const file = join(src, relPath)
    mkdirSync(join(file, '..'), { recursive: true })
    writeFileSync(file, content)
  }
  return [src]
}

describe('check-async-guards（前端异步守门）', () => {
  it('真实仓库默认通过：手搓序号零（接缝外）且 toast 基线全等', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain('异步守门')
  })

  it('夹具全基线桩（每文件恰好基线计数）时通过', () => {
    const r = run(makeFixture())
    expect(r.status).toBe(0)
    expect(r.output).toContain('异步守门')
  })

  it('空扫描根拒绝假绿（零源文件即红）', () => {
    const dir = mkdtempSync(join(tmpdir(), 'check-async-guards-empty-'))
    tempDirs.push(dir)
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('空集')
  })

  describe('规则 1：手搓竞态序号（硬零容忍，唯一合法住址 useLoadable）', () => {
    it('let fetchSeq = 0 即红，定位文件与行号', () => {
      const r = run(
        makeFixture({
          'views/BadView.ts': `export function reload() {\n  let fetchSeq = 0\n  fetchSeq++\n}`,
        }),
      )
      expect(r.status).toBe(1)
      expect(r.output).toContain('手搓竞态序号')
      expect(r.output).toContain('views/BadView.ts:2')
    })

    it('裸名 let seq = 0（接缝外）同样红（加宽面）', () => {
      const r = run(
        makeFixture({ 'composables/useOther.ts': 'let seq = 0\nexport {} \n' }),
      )
      expect(r.status).toBe(1)
      expect(r.output).toContain('手搓竞态序号')
      expect(r.output).toContain('composables/useOther.ts:1')
    })

    it('唯一合法住址：composables/useLoadable.ts 内 let seq = 0 绿', () => {
      const r = run(
        makeFixture({
          'composables/useLoadable.ts': 'let seq = 0\nexport const x = seq\n',
        }),
      )
      expect(r.status).toBe(0)
    })

    it('注释行不误报：// let fetchSeq = 0 绿', () => {
      const r = run(
        makeFixture({
          'views/CommentView.ts': '// let fetchSeq = 0\nexport {}\n',
        }),
      )
      expect(r.status).toBe(0)
    })
  })

  describe('规则 2：catch 直弹 toast 基线冻结（只减不增，全等校验）', () => {
    it('基线文件回潮（4 → 5）即红，报新增行号', () => {
      const r = run(
        makeFixture({
          'views/ItemsView.vue': toastStub(5),
        }),
      )
      expect(r.status).toBe(1)
      expect(r.output).toContain('回潮')
      expect(r.output).toContain('views/ItemsView.vue')
      expect(r.output).toContain('5')
    })

    it('基线收缩未同步（4 → 3）即红，提示下调基线', () => {
      const r = run(
        makeFixture({
          'views/ItemsView.vue': toastStub(3),
        }),
      )
      expect(r.status).toBe(1)
      expect(r.output).toContain('基线待收缩')
      expect(r.output).toContain('views/ItemsView.vue')
    })

    it('基线条目清零即红，提示删除条目（基线只减不增、不挂陈目）', () => {
      const r = run(
        makeFixture({
          'views/PoliciesView.vue': 'export {}\n',
        }),
      )
      expect(r.status).toBe(1)
      expect(r.output).toContain('基线待收缩')
      expect(r.output).toContain('views/PoliciesView.vue')
    })

    it('基线外文件新增直弹 toast 即红', () => {
      const r = run(
        makeFixture({
          'components/NewWidget.vue': `${TOAST_LINE}\n`,
        }),
      )
      expect(r.status).toBe(1)
      expect(r.output).toContain('回潮')
      expect(r.output).toContain('components/NewWidget.vue')
    })

    it('注释行不误报：真实 4 处 + 注释 1 行仍与基线全等（绿）', () => {
      const r = run(
        makeFixture({
          'views/ItemsView.vue': `${toastStub(4)}// ${TOAST_LINE}\n`,
        }),
      )
      expect(r.status).toBe(0)
    })

    it('基线条目指向的文件缺失即红（清单漂移 fail loud）', () => {
      const dir = mkdtempSync(join(tmpdir(), 'check-async-guards-missing-'))
      tempDirs.push(dir)
      for (const [relPath, count] of Object.entries(TOAST_BASELINE)) {
        if (relPath === 'views/PoliciesView.vue') continue
        const abs = join(dir, relPath)
        mkdirSync(join(abs, '..'), { recursive: true })
        writeFileSync(abs, toastStub(count))
      }
      const r = run([dir])
      expect(r.status).toBe(1)
      expect(r.output).toContain('views/PoliciesView.vue')
    })
  })
})
