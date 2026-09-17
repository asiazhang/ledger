import { afterAll, describe, expect, it } from 'vitest'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { SEQ_SEAM_FILE, TOAST_BASELINE } from '../scripts/check-async-guards.ts'
import { gateScript, runGateScript } from './run-gate-script.test-helper.ts'

// 被测对象是仓库工具脚本 scripts/check-async-guards.ts（前端异步守门，issue #1039）。
// 脚本以 Bun 运行时执行（ADR-0083）：runGateScript 以 spawnSync('bun') 与门槛
// 调用同款拉起，测的就是门槛路径。
// 按测试决策只测外部可观察结果——进程退出码与输出，不测内部函数；
// 通过位置参数把扫描目标指向临时夹具仓库根（布局与生产 SCAN_ROOTS 同构）。
// 夹具基线清单自助手导出的 TOAST_BASELINE 派生（单一事实源，无双源漂移）。
const script = gateScript('check-async-guards.ts')
const run = (args: string[]) => runGateScript(script, args)

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
 * 建临时夹具：按脚本导出的 TOAST_BASELINE 生成全部条目（每文件恰好基线计数行，
 * 键相对夹具仓库根、含 src/ 前缀——与生产 SCAN_ROOTS 布局同构），再落接缝住址桩
 * （守门前置要求住址文件可达，缺失即红；withSeam=false 时省略，供住址不可达用例），
 * 最后按 overrides 追加/覆盖文件（键同样相对仓库根）。返回脚本参数（夹具仓库根）。
 */
function makeFixture(overrides: Record<string, string> = {}, withSeam = true): string[] {
  const root = mkdtempSync(join(tmpdir(), 'check-async-guards-'))
  tempDirs.push(root)
  for (const [relPath, count] of Object.entries(TOAST_BASELINE)) {
    const abs = join(root, relPath)
    mkdirSync(join(abs, '..'), { recursive: true })
    writeFileSync(abs, toastStub(count))
  }
  if (withSeam) {
    const seam = join(root, SEQ_SEAM_FILE)
    mkdirSync(join(seam, '..'), { recursive: true })
    writeFileSync(seam, 'export {}\n')
  }
  for (const [relPath, content] of Object.entries(overrides)) {
    const file = join(root, relPath)
    mkdirSync(join(file, '..'), { recursive: true })
    writeFileSync(file, content)
  }
  return [root]
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

  it('接缝住址不可达即红（搬迁未同步 SEQ_SEAM_FILE，拒绝白名单静默失效）', () => {
    // withSeam=false：不落接缝住址桩，其他源文件齐备，唯独住址缺失
    const r = run(makeFixture({}, false))
    expect(r.status).toBe(1)
    expect(r.output).toContain('住址不可达')
    expect(r.output).toContain(SEQ_SEAM_FILE)
  })

  describe('规则 1：手搓竞态序号（硬零容忍，唯一合法住址 useLoadable）', () => {
    it('let fetchSeq = 0 即红，定位文件与行号', () => {
      const r = run(
        makeFixture({
          'src/views/BadView.ts': `export function reload() {\n  let fetchSeq = 0\n  fetchSeq++\n}`,
        }),
      )
      expect(r.status).toBe(1)
      expect(r.output).toContain('手搓竞态序号')
      expect(r.output).toContain('src/views/BadView.ts:2')
    })

    it('裸名 let seq = 0（接缝外）同样红（加宽面）', () => {
      const r = run(
        makeFixture({ 'src/composables/useOther.ts': 'let seq = 0\nexport {} \n' }),
      )
      expect(r.status).toBe(1)
      expect(r.output).toContain('手搓竞态序号')
      expect(r.output).toContain('src/composables/useOther.ts:1')
    })

    it('唯一合法住址：packages/loadable/src/useLoadable.ts 内 let seq = 0 绿（#1318 随包搬迁）', () => {
      const r = run(
        makeFixture({
          [SEQ_SEAM_FILE]: 'let seq = 0\nexport const x = seq\n',
        }),
      )
      expect(r.status).toBe(0)
    })

    it('注释行不误报：// let fetchSeq = 0 绿', () => {
      const r = run(
        makeFixture({
          'src/views/CommentView.ts': '// let fetchSeq = 0\nexport {}\n',
        }),
      )
      expect(r.status).toBe(0)
    })
  })

  describe('规则 2：catch 直弹 toast 基线冻结（只减不增，全等校验）', () => {
    it('基线文件回潮（4 → 5）即红，报新增行号', () => {
      const r = run(
        makeFixture({
          'src/views/ItemsView.vue': toastStub(5),
        }),
      )
      expect(r.status).toBe(1)
      expect(r.output).toContain('回潮')
      expect(r.output).toContain('src/views/ItemsView.vue')
      expect(r.output).toContain('5')
    })

    it('基线收缩未同步（4 → 3）即红，提示下调基线', () => {
      const r = run(
        makeFixture({
          'src/views/ItemsView.vue': toastStub(3),
        }),
      )
      expect(r.status).toBe(1)
      expect(r.output).toContain('基线待收缩')
      expect(r.output).toContain('src/views/ItemsView.vue')
    })

    it('基线条目清零即红，提示删除条目（基线只减不增、不挂陈目）', () => {
      const r = run(
        makeFixture({
          'src/views/PoliciesView.vue': 'export {}\n',
        }),
      )
      expect(r.status).toBe(1)
      expect(r.output).toContain('基线待收缩')
      expect(r.output).toContain('src/views/PoliciesView.vue')
    })

    it('基线外文件新增直弹 toast 即红', () => {
      const r = run(
        makeFixture({
          'src/components/NewWidget.vue': `${TOAST_LINE}\n`,
        }),
      )
      expect(r.status).toBe(1)
      expect(r.output).toContain('回潮')
      expect(r.output).toContain('src/components/NewWidget.vue')
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
        if (relPath === 'src/views/PoliciesView.vue') continue
        const abs = join(dir, relPath)
        mkdirSync(join(abs, '..'), { recursive: true })
        writeFileSync(abs, toastStub(count))
      }
      const r = run([dir])
      expect(r.status).toBe(1)
      expect(r.output).toContain('src/views/PoliciesView.vue')
    })
  })
})
