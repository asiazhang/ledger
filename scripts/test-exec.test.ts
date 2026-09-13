import { afterAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'

// 被测对象是仓库工具脚本 scripts/test-exec.ts（测试执行器与两入口覆盖守门，
// issue #1112）。脚本以 Bun 运行时执行（ADR-0083）：spawnSync('bun') 与门槛调用
// 同款，测的就是门槛路径。按测试决策只测外部可观察结果——进程退出码与输出，
// 不测内部函数；夹具经 `--root` 指向临时工作区（只需 manifest + 目录形态，
// 守门不调用 cargo，仓与 CI 上均零依赖）。
const script = join(process.cwd(), 'scripts', 'test-exec.ts')
const repoRoot = process.cwd()

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

/** 夹具工作区：根包（lib + workspace glob）+ 成员 alpha（lib + 集成测试 api/e2e）。 */
interface FixtureOverrides {
  rootManifest?: string
  alphaManifest?: string
  /** test.sh 内容；缺省 = 合规双入口形态。 */
  testSh?: string
  /** 追加的成员文件（相对夹具根，含目录）。 */
  files?: Record<string, string>
  /** 为 true 时删除两个 lib 目标（用于 --doc 登记失效夹具）。 */
  withoutLib?: boolean
}

const DEFAULT_TEST_SH = [
  '#!/bin/sh',
  'set -eu',
  'bun scripts/test-exec.ts',
  '( cd src-tauri && cargo test --workspace --test e2e )',
  '( cd src-tauri && cargo test --workspace --doc )',
  '',
].join('\n')

function makeFixture(overrides: FixtureOverrides = {}): string {
  const root = mkdtempSync(join(tmpdir(), 'test-exec-'))
  tempDirs.push(root)
  const srcTauri = join(root, 'src-tauri')
  const alpha = join(srcTauri, 'crates', 'alpha')
  mkdirSync(join(alpha, 'src'), { recursive: true })
  mkdirSync(join(alpha, 'tests'), { recursive: true })
  mkdirSync(join(srcTauri, 'src'), { recursive: true })
  mkdirSync(join(root, 'scripts'), { recursive: true })

  writeFileSync(
    join(srcTauri, 'Cargo.toml'),
    overrides.rootManifest ??
      ['[package]', 'name = "root-app"', 'version = "0.0.0"', 'edition = "2021"', '', '[workspace]', 'members = ["crates/*"]', ''].join('\n'),
  )
  if (overrides.withoutLib !== true) {
    writeFileSync(join(srcTauri, 'src', 'lib.rs'), 'pub fn boot() {}\n')
    writeFileSync(join(alpha, 'src', 'lib.rs'), 'pub fn alpha() {}\n')
  }
  writeFileSync(
    join(alpha, 'Cargo.toml'),
    overrides.alphaManifest ??
      [
        '[package]',
        'name = "alpha"',
        'version = "0.0.0"',
        'edition = "2021"',
        '',
        '[[test]]',
        'name = "e2e"',
        'path = "tests/e2e.rs"',
        'harness = false',
        '',
      ].join('\n'),
  )
  writeFileSync(join(alpha, 'tests', 'e2e.rs'), 'fn main() {}\n')
  writeFileSync(join(alpha, 'tests', 'api.rs'), '#[test]\nfn api() {}\n')
  writeFileSync(join(root, 'scripts', 'test.sh'), overrides.testSh ?? DEFAULT_TEST_SH)
  for (const [rel, content] of Object.entries(overrides.files ?? {})) {
    const abs = join(root, rel)
    mkdirSync(dirname(abs), { recursive: true })
    writeFileSync(abs, content)
  }
  return root
}

describe('测试执行器两入口覆盖守门（issue #1112）', () => {
  it('合规夹具：目标清单与两入口并集全等 → 通过', () => {
    const r = run(['check', '--root', makeFixture()])
    expect(r.status).toBe(0)
    // 6 = 根包 lib + doc、alpha lib + doc、alpha 集成 api + e2e；e2e 归非并发入口。
    expect(r.output).toContain('目标 6 个')
    expect(r.output).toContain('并发入口 3')
    expect(r.output).toContain('集成测试 1')
    expect(r.output).toContain('e2e')
  })

  it('真实仓库通过：目标清单与两条入口全等', () => {
    const r = run(['check'])
    expect(r.status).toBe(0)
    expect(r.output).toContain('测试执行覆盖守门')
  })

  it('同包内 lib 与集成 target 同名不撞键（执行面核对键含 kind）', () => {
    // cargo 允许同包内 lib 与集成 target 同名；只用「包::名」会把两者合成一个，
    // 静态清单与 run 交叉核对双双假绿、实际少跑一个二进制。
    const r = run([
      'check',
      '--root',
      makeFixture({ files: { 'src-tauri/crates/alpha/tests/alpha.rs': '#[test]\nfn same_name() {}\n' } }),
    ])
    expect(r.status).toBe(0)
    // 7 = 根包 lib + doc、alpha lib + doc、alpha 集成 api / alpha / e2e。
    expect(r.output).toContain('目标 7 个')
    expect(r.output).toContain('lib 单测 2')
    expect(r.output).toContain('集成测试 2')
  })

  it('新增 harness=false 测试目标未登记非并发入口 → 红（目标静默漏跑即红）', () => {
    const r = run([
      'check',
      '--root',
      makeFixture({
        alphaManifest: [
          '[package]',
          'name = "alpha"',
          'version = "0.0.0"',
          'edition = "2021"',
          '',
          '[[test]]',
          'name = "e2e"',
          'path = "tests/e2e.rs"',
          'harness = false',
          '',
          '[[test]]',
          'name = "custom"',
          'path = "tests/custom.rs"',
          'harness = false',
          '',
        ].join('\n'),
        files: { 'src-tauri/crates/alpha/tests/custom.rs': 'fn main() {}\n' },
      }),
    ])
    expect(r.status).toBe(1)
    expect(r.output).toContain('alpha::custom')
    expect(r.output).toContain('静默漏跑')
  })

  it('非并发入口未登记 e2e（删 --test e2e）→ 红', () => {
    const r = run([
      'check',
      '--root',
      makeFixture({
        testSh: ['#!/bin/sh', 'set -eu', 'bun scripts/test-exec.ts', '( cd src-tauri && cargo test --workspace --doc )', ''].join('\n'),
      }),
    ])
    expect(r.status).toBe(1)
    expect(r.output).toContain('alpha::e2e')
    expect(r.output).toContain('--test e2e')
  })

  it('非并发入口登记了非自定义 harness 目标（--test api 漂移）→ 红', () => {
    const r = run([
      'check',
      '--root',
      makeFixture({
        testSh: [
          '#!/bin/sh',
          'set -eu',
          'bun scripts/test-exec.ts',
          '( cd src-tauri && cargo test --workspace --test e2e )',
          '( cd src-tauri && cargo test --workspace --test api )',
          '( cd src-tauri && cargo test --workspace --doc )',
          '',
        ].join('\n'),
      }),
    ])
    expect(r.status).toBe(1)
    expect(r.output).toContain('--test api')
    expect(r.output).toContain('不对应任何 harness = false 目标')
  })

  it('缺 cargo test --doc → 红（doc-test 静默漏跑即红）', () => {
    const r = run([
      'check',
      '--root',
      makeFixture({
        testSh: ['#!/bin/sh', 'set -eu', 'bun scripts/test-exec.ts', '( cd src-tauri && cargo test --workspace --test e2e )', ''].join('\n'),
      }),
    ])
    expect(r.status).toBe(1)
    expect(r.output).toContain('doc-test')
  })

  it('工作区无 doc-test 目标却在非并发入口登记 --doc → 红（登记失效）', () => {
    const r = run([
      'check',
      '--root',
      makeFixture({
        withoutLib: true,
        alphaManifest: [
          '[package]',
          'name = "alpha"',
          'version = "0.0.0"',
          'edition = "2021"',
          '',
          '[[test]]',
          'name = "e2e"',
          'path = "tests/e2e.rs"',
          'harness = false',
          '',
        ].join('\n'),
      }),
    ])
    expect(r.status).toBe(1)
    expect(r.output).toContain('无 doc-test 目标')
  })

  it('scripts/test.sh 删掉并发入口调用 → 红（接线删除即红）', () => {
    const r = run([
      'check',
      '--root',
      makeFixture({
        testSh: ['#!/bin/sh', 'set -eu', '( cd src-tauri && cargo test --workspace --test e2e )', '( cd src-tauri && cargo test --workspace --doc )', ''].join('\n'),
      }),
    ])
    expect(r.status).toBe(1)
    expect(r.output).toContain('未调用并发入口')
  })

  it('manifest 出现 auto* 开关（发现面被改写）→ 红（拒绝猜测）', () => {
    const r = run([
      'check',
      '--root',
      makeFixture({
        alphaManifest: ['[package]', 'name = "alpha"', 'version = "0.0.0"', 'edition = "2021"', 'autotests = false', '', '[[test]]', 'name = "e2e"', 'path = "tests/e2e.rs"', 'harness = false', ''].join('\n'),
      }),
    ])
    expect(r.status).toBe(1)
    expect(r.output).toContain('autotests')
  })

  it('members 出现未支持的 glob 形态 → 红', () => {
    const r = run([
      'check',
      '--root',
      makeFixture({
        rootManifest: ['[package]', 'name = "root-app"', 'version = "0.0.0"', 'edition = "2021"', '', '[workspace]', 'members = ["crates/**"]', ''].join('\n'),
      }),
    ])
    expect(r.status).toBe(1)
    expect(r.output).toContain('未支持的 members glob')
  })
})

describe('测试执行接线（删除即变红）', () => {
  it('scripts/test.sh 同时含并发入口与非并发入口（e2e + doc-test）', () => {
    const testSh = readFileSync(join(repoRoot, 'scripts', 'test.sh'), 'utf8')
    const lines = testSh.split('\n').filter((l) => !l.trim().startsWith('#'))
    // 断言命令行形态而非子串：`echo scripts/test-exec.ts` 之类不构成接线。
    expect(lines.some((l) => /^bun\s+scripts\/test-exec\.ts\s*$/.test(l.trim()))).toBe(true)
    expect(
      lines.some(
        (l) => /^\(\s*cd src-tauri && cargo test\s+--workspace\s+--test e2e\s*\)$/.test(l.trim()),
      ),
    ).toBe(true)
    expect(
      lines.some((l) => /^\(\s*cd src-tauri && cargo test\s+--workspace\s+--doc\s*\)$/.test(l.trim())),
    ).toBe(true)
  })

  it('覆盖守门挂载于 scripts/check.sh 与 CI（frontend job）', () => {
    const checkSh = readFileSync(join(repoRoot, 'scripts', 'check.sh'), 'utf8')
    expect(
      checkSh
        .split('\n')
        .some((l) => !l.trim().startsWith('#') && /^bun\s+scripts\/test-exec\.ts check$/.test(l.trim())),
    ).toBe(true)
    const workflow = readFileSync(join(repoRoot, '.github', 'workflows', 'build.yml'), 'utf8')
    expect(
      workflow
        .split('\n')
        .some((l) => /^run:\s*bun\s+scripts\/test-exec\.ts check$/.test(l.trim())),
    ).toBe(true)
  })
})
