import { afterAll, describe, expect, it } from 'vitest'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { gateScript, runGateScript } from './run-gate-script.test-helper.ts'
import { hasCommandLine, repoRoot } from './has-command-line.test-helper.ts'

// 被测对象是仓库工具脚本 scripts/check-e2e-step-coverage.ts（e2e 步骤库覆盖守门，
// issue #1510 / spec #1494 决策落地）。脚本以 Bun 运行时执行（ADR-0083）：
// runGateScript 以 spawnSync('bun') 与门槛调用同款拉起，测的就是门槛路径。按
// ADR-0087 断言强度只测外部可观察结果——退出码与合并输出，不测内部函数；夹具经
// 位置参数指向临时 src-tauri 目录（布局与生产同构：tests/<目标>.rs + tests/e2e/**），
// 每条负向用例都是「制造变红」的一次真实运行。
//
// 覆盖七态：绿基线 / 删注册 / feature 新增步骤 / feature 无目标绑定（含删
// `scenarios!` 绑定——#1495 AC4 的「0 场景且退出码 0」缺口）/ 同族重复注册 /
// 跨族双注册不误报 / 两种模式同命中（歧义）；另加两道接线锁（check.sh 与 CI）。
const script = gateScript('check-e2e-step-coverage.ts')
const run = (srcTauriDir: string) => runGateScript(script, [srcTauriDir])

const tempDirs: string[] = []
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true })
})

/** 黄瓜目标夹具（生产 e2e.rs 同形：`#[path]` 步骤模块 + `filter_run` 目录绑定） */
const CUCUMBER_TARGET = [
  '#[path = "e2e/steps.rs"]',
  'mod steps;',
  '',
  'fn main() {',
  '    LedgerWorld::filter_run("tests/e2e/features", |_, _, _| true);',
  '}',
].join('\n')

/** 黄瓜步骤夹具：string / int 两类占位符 + 一条中文断言步骤 */
const CUCUMBER_STEPS = [
  '#[given(expr = "存在账户 {string}")]',
  'fn a(_: &mut World, _: String) {}',
  '#[when(expr = "创建交易 金额 {int}")]',
  'fn b(_: &mut World, _: i64) {}',
  '#[then(expr = "{string} 账户余额应为 {int}")]',
  'fn c(_: &mut World, _: String, _: i64) {}',
].join('\n')

const FEATURE = [
  'Feature: 夹具',
  '  Scenario: 夹具场景',
  '    Given 存在账户 "现金"',
  '    When 创建交易 金额 100',
  '    Then "现金" 账户余额应为 100',
].join('\n')

/** rstest-bdd 目标夹具（生产 e2e_rstest.rs 同形：`scenarios!` 绑定 + 步骤模块） */
const RSTEST_TARGET = (featurePath: string) =>
  [
    '#[path = "e2e/rstest_steps.rs"]',
    'mod rstest_steps;',
    '',
    `scenarios!("${featurePath}", fixtures = [world: World]);`,
  ].join('\n')

/** rstest-bdd 步骤夹具：类型提示形态（`{<名>:string}` / `{<名>:i64}`） */
const RSTEST_STEPS = [
  '#[rstest_bdd_macros::given("存在账户 {name:string}")]',
  'fn a(_: &mut World, _: String) {}',
  '#[rstest_bdd_macros::when("创建交易 金额 {amount:i64}")]',
  'fn b(_: &mut World, _: i64) {}',
  '#[rstest_bdd_macros::then("{name:string} 账户余额应为 {expected:i64}")]',
  'fn c(_: &mut World, _: String, _: i64) {}',
].join('\n')

/** 建临时夹具：`overrides` 以仓库相对路径覆盖基线文件，值为 null 表示删除该文件。 */
function fixture(overrides: Record<string, string | null> = {}): string {
  const dir = mkdtempSync(join(tmpdir(), 'e2e-step-coverage-'))
  tempDirs.push(dir)
  const srcTauriDir = join(dir, 'src-tauri')
  const files: Record<string, string | null> = {
    'tests/e2e.rs': CUCUMBER_TARGET,
    'tests/e2e/steps.rs': CUCUMBER_STEPS,
    'tests/e2e/features/a.feature': FEATURE,
    ...overrides,
  }
  for (const [relativePath, content] of Object.entries(files)) {
    if (content === null) continue
    const path = join(srcTauriDir, relativePath)
    mkdirSync(dirname(path), { recursive: true })
    writeFileSync(path, content)
  }
  return srcTauriDir
}

describe('e2e 步骤库覆盖守门（scripts/check-e2e-step-coverage.ts）', () => {
  it('绿基线：绑定面覆盖全部步骤时通过并自报数字', () => {
    const result = run(fixture())
    expect(result.status).toBe(0)
    expect(result.output).toContain('✓ e2e 步骤库覆盖守门')
    expect(result.output).toContain('未覆盖 0')
  })

  it('删除一条步骤注册 → 对应步骤行未覆盖即红', () => {
    const steps = CUCUMBER_STEPS.split('\n').filter((line) => !line.includes('创建交易')).join('\n')
    const result = run(fixture({ 'tests/e2e/steps.rs': steps }))
    expect(result.status).toBe(1)
    expect(result.output).toContain('未覆盖')
    expect(result.output).toContain('创建交易 金额 100')
  })

  it('feature 新增步骤而无人实现 → 红', () => {
    const feature = `${FEATURE}\n    And 删除账户 "现金"\n`
    const result = run(fixture({ 'tests/e2e/features/a.feature': feature }))
    expect(result.status).toBe(1)
    expect(result.output).toContain('未覆盖')
    expect(result.output).toContain('删除账户 "现金"')
  })

  it('feature 无任何目标绑定（删 filter_run）→ 红', () => {
    const target = CUCUMBER_TARGET.replace(/^.*filter_run.*$/m, '// 绑定被删除')
    const result = run(fixture({ 'tests/e2e.rs': target }))
    expect(result.status).toBe(1)
    expect(result.output).toContain('无目标绑定')
  })

  it('rstest 目标删 scenarios! 绑定 → 该 feature 无目标绑定即红（#1495 AC4 缺口）', () => {
    const unbound = () => RSTEST_TARGET('tests/e2e/features/a.feature')
    const bindings = {
      'tests/e2e.rs': null, // 夹具里只留 rstest 目标：删绑定后 a.feature 无任何目标绑定
      'tests/e2e_rstest.rs': unbound(),
      'tests/e2e/rstest_steps.rs': RSTEST_STEPS,
    }
    expect(run(fixture(bindings)).status).toBe(0)

    const withoutBinding = RSTEST_TARGET('tests/e2e/features/a.feature').replace(
      /^scenarios!.*$/m,
      '// 场景绑定被删除',
    )
    const result = run(fixture({ ...bindings, 'tests/e2e_rstest.rs': withoutBinding }))
    expect(result.status).toBe(1)
    expect(result.output).toContain('无目标绑定')
  })

  it('同一运行器内重复注册 → 红', () => {
    const steps = `${CUCUMBER_STEPS}\n#[when(expr = "创建交易 金额 {int}")]\nfn duplicate(_: &mut World, _: i64) {}\n`
    const result = run(fixture({ 'tests/e2e/steps.rs': steps }))
    expect(result.status).toBe(1)
    expect(result.output).toContain('同一注册重复')
  })

  it('同一函数双注册（跨运行器）不算重复也不误报歧义', () => {
    const dualSteps = [
      '#[given(expr = "存在账户 {string}")]',
      '#[rstest_bdd_macros::given("存在账户 {name:string}")]',
      'fn a(_: &mut World, _: String) {}',
      '#[when(expr = "创建交易 金额 {int}")]',
      '#[rstest_bdd_macros::when("创建交易 金额 {amount:i64}")]',
      'fn b(_: &mut World, _: i64) {}',
      '#[then(expr = "{string} 账户余额应为 {int}")]',
      '#[rstest_bdd_macros::then("{name:string} 账户余额应为 {expected:i64}")]',
      'fn c(_: &mut World, _: String, _: i64) {}',
    ].join('\n')
    const result = run(
      fixture({
        'tests/e2e/steps.rs': dualSteps,
        'tests/e2e_rstest.rs': RSTEST_TARGET('tests/e2e/features/a.feature').replace(
          'e2e/rstest_steps.rs',
          'e2e/steps.rs',
        ).replace('mod rstest_steps;', 'mod steps;'),
      }),
    )
    expect(result.status).toBe(0)
    expect(result.output).toContain('目标 2 个')
  })

  it('同一步骤行被两条模式同时匹配 → 歧义红', () => {
    const steps = `${CUCUMBER_STEPS}\n#[when(expr = "创建交易 金额 {float}")]\nfn ambiguous(_: &mut World, _: f64) {}\n`
    const result = run(fixture({ 'tests/e2e/steps.rs': steps }))
    expect(result.status).toBe(1)
    expect(result.output).toContain('歧义')
  })
})

// 接线锁（删除即变红）：门本体在 check.sh 与 CI frontend job 的接线由源码扫描守住
// ——与门禁自身接线不靠评审记忆同款（先例 check-sh-gate-off.test.ts、#959/#961）。
describe('e2e 步骤库覆盖守门接线（scripts/check.sh 与 CI frontend job）', () => {
  it('scripts/check.sh 含本门执行行', () => {
    const checkSh = readFileSync(join(repoRoot(), 'scripts', 'check.sh'), 'utf8')
    expect(hasCommandLine(checkSh, 'bun scripts/check-e2e-step-coverage.ts')).toBe(true)
  })

  it('CI frontend job 含本门执行行', () => {
    const workflow = readFileSync(join(repoRoot(), '.github', 'workflows', 'build.yml'), 'utf8')
    expect(hasCommandLine(workflow, 'bun scripts/check-e2e-step-coverage.ts')).toBe(true)
  })
})
