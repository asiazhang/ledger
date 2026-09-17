import { afterAll, describe, expect, it } from 'vitest'
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { STYLE_BLOCK_WHITELIST } from '../scripts/check-style-blocks.ts'
import { gateScript, runGateScript } from './run-gate-script.test-helper.ts'

// 被测对象是仓库工具脚本 scripts/check-style-blocks.ts（样式块守门，issue #888 /
// ADR-0093）。脚本以 Bun 运行时执行（ADR-0083）：runGateScript 以 spawnSync('bun')
// 与门槛调用同款拉起，测的就是门槛路径。按测试决策只测外部可观察结果——进程退出码
// 与输出（ADR-0087 断言强度），不测内部函数；通过位置参数把扫描目标指向临时夹具仓库根
// （布局与生产扫描面同构：白名单键含 src/ 前缀、相对仓库根），spawnSync 的 cwd 同步
// 指向夹具根——守门的路径归一化以 cwd 为基准，夹具运行时归一化须落在夹具上。
// 夹具文件清单自助手导出的 STYLE_BLOCK_WHITELIST 派生（单一事实源，无双源漂移，
// TOAST_BASELINE 同款纪律）。
const script = gateScript('check-style-blocks.ts')
const run = (args: string[], cwd?: string, scriptPath = script) =>
  runGateScript(scriptPath, args, { cwd })

const tempDirs: string[] = []
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true })
})

/** 夹具 SFC：带 <style> 块的存量携带形态（守门经 vue/compiler-sfc 解析，须为合法 SFC） */
const STUB_WITH_STYLE = [
  '<template>',
  '  <div class="stub" />',
  '</template>',
  '',
  '<style scoped>',
  '.stub {',
  '  color: red;',
  '}',
  '</style>',
  '',
].join('\n')

/** 建临时夹具：按脚本导出的 STYLE_BLOCK_WHITELIST 生成全部条目文件（每文件带一个
 *  <style> 块——白名单即存量携带者登记表，全量桩即夹具绿基线）；omit 指定的条目
 *  不落盘（文件删除/改名后未同步白名单的陈目形态）。返回夹具仓库根。 */
function makeFixture(omit?: string): string {
  // 夹具根必须**解析后**再当 cwd 与扫描根传入（issue #1368）：macOS 的 os.tmpdir()
  // 是 /var/folders/…，而 /var 是指向 /private/var 的符号链接，子进程里
  // process.cwd() 拿到的是解析后的 /private/var/…——两串不同源时守门的
  // relative(process.cwd(), file) 会算出 ../../../var/folders/…，白名单对比全数失配，
  // 夹具绿基线恒红（Linux 的 /tmp 非符号链接，故 CI 掩盖了此缺陷）。
  // 生产调用不受影响：门槛不带扫描根参数，默认根由脚本自身路径推出，与 cwd 同源。
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'check-style-blocks-')))
  tempDirs.push(root)
  for (const rel of STYLE_BLOCK_WHITELIST) {
    if (rel === omit) continue
    const abs = join(root, rel)
    mkdirSync(join(abs, '..'), { recursive: true })
    writeFileSync(abs, STUB_WITH_STYLE)
  }
  return root
}

/** 生成脚本变体：从白名单删除 entry 条目（文本级删除，锚点未命中即抛错
 *  ——变体失效时报红而非假绿）。条目行格式（两格缩进 + 单引号 + 逗号）由
 *  tsconfig.scripts.json 工程的类型检查与仓库格式约定保稳，漂移时在本处显式报错。
 *  返回变体脚本绝对路径（写入仓库 node_modules 下：Bun 依赖解析随文件位置向上
 *  走，脱离仓库树则 vue/compiler-sfc 不可达；node_modules 已 gitignore，不污染
 *  工作树状态，afterAll 统一清理）。 */
function makeWhitelistMutant(entry: string): string {
  const needle = `  '${entry}',\n`
  const source = readFileSync(script, 'utf8')
  if (!source.includes(needle)) {
    throw new Error(`白名单变体锚点未命中：${needle.trim()}——脚本格式漂移，请同步本测试`)
  }
  const dir = mkdtempSync(join(process.cwd(), 'node_modules', 'check-style-blocks-mutant-'))
  tempDirs.push(dir)
  const mutant = join(dir, 'check-style-blocks.ts')
  writeFileSync(mutant, source.replace(needle, ''))
  return mutant
}

describe('check-style-blocks（样式块守门）', () => {
  it('真实仓库默认通过：白名单外零 <style> 块且全条目可达（现行仓库绿）', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain('样式块守门')
  })

  it('夹具全量桩（每条目文件带 <style> 块）时通过', () => {
    const root = makeFixture()
    const r = run([root], root)
    expect(r.status).toBe(0)
    expect(r.output).toContain('样式块守门')
  })

  it('白名单条目指向的文件缺失即红：陈目报出路径（清单漂移 fail loud，#1360）', () => {
    const omitted = STYLE_BLOCK_WHITELIST[0]
    const root = makeFixture(omitted)
    const r = run([root], root)
    expect(r.status).toBe(1)
    expect(r.output).toContain('不可达')
    expect(r.output).toContain(omitted)
    // 只有被 omit 的那一条不可达（issue #1368）：夹具路径同源时归一化必然落在夹具上，
    // 报出多条即归一化再次失配（曾经在 macOS 上以「全部不可达」假红）。
    expect(r.output.match(/不可达/g)).toHaveLength(1)
  })

  it('删除白名单条目即红：文件仍带 <style> 块时回潮分支拦截（只减不增，#1360）', () => {
    const entry = STYLE_BLOCK_WHITELIST[STYLE_BLOCK_WHITELIST.length - 1]
    const root = makeFixture() // 文件仍在且仍带 <style> 块，唯独清单失去该条目
    const r = run([root], root, makeWhitelistMutant(entry))
    expect(r.status).toBe(1)
    expect(r.output).toContain('新增 <style> 块')
    expect(r.output).toContain(entry)
  })
})
