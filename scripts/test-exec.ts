#!/usr/bin/env bun
// 测试执行器（issue #1112 / 父 spec #1086；e2e 执行器切 cargo-nextest：ticket
// #1496 / spec #1494）：workspace 拆成多 crate 之后，
// `cargo test --workspace` 把全部测试二进制**顺序**启动（当前清单与数量以
// `bun scripts/test-exec.ts plan` 的输出为唯一权威，本文件不复述），每个二进制
// 各自按 CPU 数开满 libtest 线程——进程启动开销与「最后一个二进制独占整机」的
// 尾部空转是纯浪费。执行面按两条入口重划：
//
// ① 并发入口（本脚本默认命令 run，scripts/test.sh 第一条命令）：一条
//    `cargo test --workspace --no-run --message-format=json-render-diagnostics`
//    构建一次，解析 cargo 报告的测试二进制清单，再由本脚本统一调度——全局
//    并行度 = min(--jobs 或 CPU 数, 待跑二进制数)，每个二进制固定
//    `RUST_TEST_THREADS=1`（只约束 libtest 线程），libtest 线程数 = 并行度，
//    不出现「各二进制各自开满 libtest 线程」的 CPU 超订（测试自起的 tokio 等
//    运行时不在控制面，属经验证据）；失败聚合报告，跑完全部才退出。
// ② nextest 入口（scripts/test.sh 第二条命令，ticket #1496 / spec #1494）：e2e
//    新目标（rstest-bdd，`harness = true`）改由 `cargo nextest run` 承载——进程级
//    per-test 调度，每个 Scenario 一个进程、并行度按 CPU 收敛（进程内 libtest 线程
//    并行对世界构造是负收益）。承接的目标以本文件 NEXTEST_TARGETS 登记为唯一声明，
//    不留在并发入口队列里重复跑；登记 ⇔ scripts/test.sh 的 nextest 命令双向全等。
// ③ 非并发入口（scripts/test.sh 第三、四条命令）：e2e 旧目标（cucumber，
//    `harness = false` 自定义 runner，不支持 nextest 的 `--list` 协议）与 doc-test
//    （测试二进制由 rustdoc 生成、不进 cargo 的 test artifact 报告）仍走 cargo 自有
//    入口。收口票 #1508 删除 cucumber 后 ② 与 ③ 的 e2e 线合流为 nextest 单入口。
//
// 覆盖守门（check 命令；scripts/test.sh 与 scripts/check.sh 同址执行，CI 亦挂）：
// 目标清单口径与 `cargo test` 默认执行面全等——lib 单测 + bin 单测 + 集成测试 +
// doc-test；example / bench 不在默认执行面（cargo 只构建 example、不跑 bench），
// 故不入清单，但发现声明即提示，防口径漂移。守门六条（任一处漂移即红，fail loud）：
//   ① 工作区全部测试目标 = 并发入口承接集合 ⊎ nextest 承接集合 ⊎ 非并发入口承接
//      集合（互斥且无遗漏；目标清单自 manifest + 目录自动发现派生，新增 target /
//      新成员 crate 自动入列）；
//   ② `[[test]] harness = false` 的声明集合 ⇔ scripts/test.sh 非并发入口的
//      `--test <name>` 名单（双向全等——新增自定义 harness 目标未登记即红，登记
//      失效或拼写漂移同样红）；
//   ③ nextest 承接登记 ⇔ scripts/test.sh 的 `cargo nextest run … --test <name>`
//      名单（双向全等，ticket #1496）：NEXTEST_TARGETS 是「必须经 nextest 进程级
//      per-test 调度」的唯一声明，删掉 nextest 运行行即红——不静默降级回并发入口
//      （那样会退回进程内串行，正是本票要消灭的形态）；登记目标必须是 harness = true
//      的集成测试。另核对 `src-tauri/.config/nextest.toml` 的 default-filter 排除
//      名单与 `harness = false` 目标清单双向全等：多排一个 libtest 目标即红（静默
//      漏跑），少排一个自定义目标会让 nextest 列举失败（fail loud）。
//   ④ 存在 doc-test 目标 ⇔ scripts/test.sh 有 `cargo test --workspace --doc`
//      （doc 命令必须带 workspace 范围——退化成 `cargo test --doc` 会让成员 crate
//      的 doctest 静默漏跑而守门仍绿）；且 scripts/test.sh 必须调用并发入口
//      的**运行**命令 `bun scripts/test-exec.ts [run]`（删除即红；`… check` 自检行
//      不算运行接线）；
//     ②③④ 与⑤同口径：只认**命令位置**的命令——非注释行里的说明文字（引号内的
//      `echo "…bun scripts/test-exec.ts…"`）不算接线，否则把真命令全包成
//      echo 字符串就能让入口一起假绿（#1112 第三轮审查实测）。
//   ⑤ 门禁自身接线：scripts/check.sh 与 CI frontend job 必须调用 `check`——接线
//      在管线里、不由单元测试构建，按先例 #959/#961 以源码扫描守门（删除接线即红）；
//   ⑥ 执行器等价性：测试运行期不得依赖 cargo 注入的环境变量（现状零命中，见
//      cargoRuntimeEnvProblems 注）。
// run 命令另有第七道交叉核对：`cargo test --no-run` 实际构建出的测试二进制集合
// 必须与①的目标清单全等——发现逻辑与 cargo 真实行为漂移即红，不给「清单看着对、
// 实际漏跑」留口子。unsupported manifest 形态（auto* 开关、lib harness=false、
// 非尾随 `*` 的成员 glob）一律拒绝而非猜测。
//
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/test-exec.ts [run|check|plan] [--jobs N]`，
// 默认 run；测试可传 `--root <夹具根>` 指向夹具（夹具只需 manifest + 目录形态，
// 不调用 cargo）。挂载于 scripts/test.sh（本地测试入口）与 scripts/check.sh
// 质量门槛序列 + CI frontend job（覆盖守门部分）；CI 的测试执行面自 #1496 起
// 与本地同口径（cargo-nextest + 旧 cucumber 目标各一条，见 build.yml backend job）。

import { spawn } from 'node:child_process'
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import { cpus } from 'node:os'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
// 复用结构守门的 Rust 词法掩码（注释掩去、字面量保留）——不新建第二份词法器。
import { maskNonCode } from './check-structure.ts'

/** 根包（Rust workspace 根 = tauri 应用包）目录名，相对仓库根。 */
const SRC_TAURI_DIR_NAME = 'src-tauri'

/** 两个测试入口的宿主脚本（覆盖守门读取其命令行）。 */
const TEST_SH_REL = join('scripts', 'test.sh')

/** 并发入口在 scripts/test.sh 里的调用标记（删除即红）。 */
export const PARALLEL_ENTRY_MARKER = 'scripts/test-exec.ts'

/** 非并发入口逐目标参数与 doc-test 开关（scripts/test.sh 命令行判据）。 */
export const DELEGATED_TARGET_FLAG = '--test'
export const DOC_FLAG = '--doc'

/**
 * nextest 入口（scripts/test.sh 的 `cargo nextest run …` 命令）承载的目标登记
 * （ticket #1496 / spec #1494）：迁移期「必须经 nextest 进程级 per-test 调度」的
 * 集成测试目标唯一声明处。删掉 scripts/test.sh 的 nextest 运行行（或把登记目标挪回
 * 并发入口）即红——并发入口固定 `RUST_TEST_THREADS=1` 会把 Scenario 重新串成进程内
 * 单线程，正是本票要消灭的形态。逐域迁移把场景并入同一目标，故本清单不随票增长；
 * 收口票 #1508 删除 cucumber、本地与 CI 统一 nextest 单入口后本登记退役。
 */
export const NEXTEST_TARGETS: readonly string[] = ['e2e_rstest']

/** nextest 仓库配置（相对仓库根）：default-filter 排除名单的核对对象。 */
export const NEXTEST_CONFIG_REL = join(SRC_TAURI_DIR_NAME, '.config', 'nextest.toml')

/** nextest 运行命令词（scripts/test.sh 命令行判据）：`cargo nextest run …`。 */
export const NEXTEST_SUBCOMMAND = 'nextest'
export const NEXTEST_RUN_SUBCOMMAND = 'run'

/** workspace 范围参数：`--workspace` 或 `--all`（精确 token 匹配，`--all-targets`
 * / `--all-features` 不算；与 check-structure.ts 的 WORKSPACE_COMMAND_FILES 同口径）。 */
export const WORKSPACE_FLAG = '--workspace'
export const ALL_FLAG = '--all'

/** cargo `cargo test` 默认执行面的目标种类；`doc` 由 cargo 自有入口承接。 */
export type TargetKind = 'lib' | 'bin' | 'test' | 'doc'

/** 一个测试目标（doc 目标的 target 名 = lib 目标名）。 */
export interface DiscoveredTarget {
  package: string
  target: string
  kind: TargetKind
  /** 仅集成测试目标有意义：`[[test]] harness = false`（自定义 runner）。 */
  harnessFalse: boolean
  /** 目标所属包根（绝对路径）——测试进程的 cwd 与 cargo 口径一致（见 runKey 注）。 */
  cwd: string
}

/** 展示用键（包名 + 目标名）：守门报错与 plan 输出用，不参与集合相等核对。 */
export function targetKey(t: { package: string; target: string }): string {
  return `${t.package}::${t.target}`
}

/**
 * 执行面核对键（包名 + 目标名 + 种类）：cargo 允许同一包内 lib 与 bin / 集成
 * target 同名（`src/lib.rs` + `src/main.rs` 的常见形态），只用「包 + 名」会撞键
 * ——静态清单与 run 的交叉核对都会假绿，实际只跑掉一个二进制（issue #1112
 * Spec 审查发现）。故一切集合运算都用带 kind 的键。
 */
export function runKey(t: { package: string; target: string; kind: TargetKind }): string {
  return `${t.package}::${t.target}::${t.kind}`
}

/** 目标发现结果：目标清单 + 形态问题（不支持形态一律显式拒绝而非猜测）。 */
interface Discovery {
  targets: DiscoveredTarget[]
  problems: string[]
  /** example / bench 目标（不在 `cargo test` 默认执行面，仅提示）。 */
  outOfFace: string[]
}

// ── manifest 解析（窄形态文本扫描，形态同 check-structure.ts 家族） ──────────
// 只认本仓实际存在的键形态：字符串 / 布尔 / 字符串数组 / `[[table]]` 条目。
// 需要正则与真 TOML 解析器才能覆盖的形态（多行字符串、内联表）不出现即不支持，
// 出现时发现阶段会因目标数对不上而在 run 命令的交叉核对里变红。

interface TargetDecl {
  name?: string
  path?: string
  test?: boolean
  harness?: boolean
  doctest?: boolean
}

interface ManifestTables {
  package: { name?: string }
  lib: TargetDecl | null
  bins: TargetDecl[]
  tests: TargetDecl[]
  benches: TargetDecl[]
  examples: TargetDecl[]
  workspace: { members?: string[] }
  /** auto* 开关：发现逻辑不辖，出现即拒绝。 */
  unsupportedKeys: string[]
}

const ARRAY_DECL_TABLES: Record<string, 'bins' | 'tests' | 'benches' | 'examples'> = {
  bin: 'bins',
  test: 'tests',
  bench: 'benches',
  example: 'examples',
}

const UNSUPPORTED_KEYS = ['autobins', 'autotests', 'autobenches', 'autoexamples']

/** 去掉行内注释：`#` 在引号外才算注释起点（字符串里的 `#` 保留）。 */
function stripComment(line: string): string {
  let quote: string | null = null
  for (let i = 0; i < line.length; i += 1) {
    const ch = line[i]
    if (quote !== null) {
      if (ch === quote) quote = null
      continue
    }
    if (ch === '"' || ch === "'") {
      quote = ch
      continue
    }
    if (ch === '#') return line.slice(0, i)
  }
  return line
}

/** 方括号配平数（多行数组续行判定）。 */
function bracketBalance(line: string): number {
  let depth = 0
  let quote: string | null = null
  for (const ch of line) {
    if (quote !== null) {
      if (ch === quote) quote = null
      continue
    }
    if (ch === '"' || ch === "'") quote = ch
    else if (ch === '[') depth += 1
    else if (ch === ']') depth -= 1
  }
  return depth
}

/** 解析基本值：字符串 / 布尔 / 字符串数组 / 其余原样返回。 */
function parseValue(raw: string): unknown {
  const text = raw.trim()
  const quoted = text.match(/^(["'])(.*)\1$/)
  if (quoted !== null) return quoted[2]
  if (text === 'true') return true
  if (text === 'false') return false
  if (text.startsWith('[') && text.endsWith(']')) {
    return splitArrayItems(text.slice(1, -1)).map((item) => parseValue(item))
  }
  return text
}

/** 按顶层逗号切分数组项（引号内的逗号不算分隔）。 */
function splitArrayItems(body: string): string[] {
  const items: string[] = []
  let current = ''
  let quote: string | null = null
  for (const ch of body) {
    if (quote !== null) {
      current += ch
      if (ch === quote) quote = null
      continue
    }
    if (ch === '"' || ch === "'") {
      quote = ch
      current += ch
      continue
    }
    if (ch === ',') {
      items.push(current)
      current = ''
      continue
    }
    current += ch
  }
  items.push(current)
  return items.map((i) => i.trim()).filter((i) => i !== '')
}

function parseManifest(text: string): ManifestTables {
  const out: ManifestTables = {
    package: {},
    lib: null,
    bins: [],
    tests: [],
    benches: [],
    examples: [],
    workspace: {},
    unsupportedKeys: [],
  }
  let table: string | null = null
  let decl: TargetDecl | null = null
  const lines = text.split('\n')
  for (let i = 0; i < lines.length; i += 1) {
    let line = stripComment(lines[i] ?? '').trim()
    if (line === '') continue
    while (bracketBalance(line) > 0 && i + 1 < lines.length) {
      i += 1
      line += ` ${stripComment(lines[i] ?? '').trim()}`
    }

    const arrayHeader = line.match(/^\[\[\s*([A-Za-z0-9_-]+)\s*\]\]$/)
    if (arrayHeader !== null) {
      const key = ARRAY_DECL_TABLES[arrayHeader[1] ?? '']
      if (key === undefined) {
        table = null
        decl = null
      } else {
        decl = {}
        table = key
        out[key].push(decl)
      }
      continue
    }
    const tableHeader = line.match(/^\[\s*([A-Za-z0-9_-]+)\s*\]$/)
    if (tableHeader !== null) {
      const name = tableHeader[1] ?? ''
      decl = null
      table = name
      if (name === 'lib') out.lib = {}
      continue
    }
    if (line.startsWith('[')) {
      // 点分表头（`[workspace.lints.clippy]` 等）与不认识的表：本段无受关注键。
      decl = null
      table = null
      continue
    }
    const kv = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(.+)$/)
    if (kv === null) continue
    const key = kv[1] ?? ''
    const value = parseValue(kv[2] ?? '')
    if (UNSUPPORTED_KEYS.includes(key)) out.unsupportedKeys.push(key)
    const target = decl ?? (table === 'lib' ? out.lib : null)
    if (target !== null) {
      applyTargetKey(target, key, value)
      continue
    }
    if (table === 'package' && key === 'name' && typeof value === 'string') out.package.name = value
    if (table === 'workspace' && key === 'members' && Array.isArray(value)) {
      out.workspace.members = value.filter((v): v is string => typeof v === 'string')
    }
  }
  return out
}

/** 目标声明键（字符串 / 布尔）逐条落位；非受关注键忽略。 */
function applyTargetKey(target: TargetDecl, key: string, value: unknown): void {
  if (key === 'name' && typeof value === 'string') target.name = value
  else if (key === 'path' && typeof value === 'string') target.path = value
  else if (key === 'test' && typeof value === 'boolean') target.test = value
  else if (key === 'harness' && typeof value === 'boolean') target.harness = value
  else if (key === 'doctest' && typeof value === 'boolean') target.doctest = value
}

// ── 目标发现（manifest 声明 + 目录自动发现，口径 = cargo test 默认执行面） ────

function readManifest(dir: string): ManifestTables | null {
  const path = join(dir, 'Cargo.toml')
  if (!existsSync(path)) return null
  return parseManifest(readFileSync(path, 'utf8'))
}

function listDir(dir: string): string[] {
  if (!existsSync(dir)) return []
  return readdirSync(dir).sort()
}

function isDirectory(path: string): boolean {
  return existsSync(path) && statSync(path).isDirectory()
}

/** 目标名归一：cargo 对未显式命名的 target 取文件/目录名。 */
function stemName(file: string): string {
  return file.replace(/\.rs$/, '')
}

/** 展开 `[workspace] members`：只支持「前缀 + 尾随 `*`」的单层 glob 与裸目录。 */
function expandMemberGlobs(srcTauriDir: string, patterns: string[]): { dirs: string[]; problems: string[] } {
  const dirs: string[] = []
  const problems: string[] = []
  for (const pattern of patterns) {
    if (pattern.endsWith('/*')) {
      const base = pattern.slice(0, -2)
      const baseDir = join(srcTauriDir, base)
      if (!isDirectory(baseDir)) {
        problems.push(`✗ 目标发现：members glob 基目录不存在：${base}（${pattern}）`)
        continue
      }
      for (const entry of listDir(baseDir)) {
        const entryDir = join(baseDir, entry)
        if (isDirectory(entryDir) && existsSync(join(entryDir, 'Cargo.toml'))) dirs.push(entryDir)
      }
      continue
    }
    if (pattern.includes('*')) {
      problems.push(`✗ 目标发现：未支持的 members glob 形态：${pattern}（只支持「前缀 + 尾随 *」单层 glob）`)
      continue
    }
    const dir = join(srcTauriDir, pattern)
    if (!existsSync(join(dir, 'Cargo.toml'))) {
      problems.push(`✗ 目标发现：members 目录缺 Cargo.toml：${pattern}`)
      continue
    }
    dirs.push(dir)
  }
  return { dirs, problems }
}

interface PackageTargets {
  targets: DiscoveredTarget[]
  problems: string[]
  outOfFace: string[]
}

function discoverPackage(dir: string, rel: string, tables: ManifestTables): PackageTargets {
  const targets: DiscoveredTarget[] = []
  const problems: string[] = []
  const outOfFace: string[] = []
  const packageName = tables.package.name
  if (packageName === undefined) {
    problems.push(`✗ 目标发现：${rel}/Cargo.toml 缺 [package] name——目标无法归属包，拒绝继续`)
    return { targets, problems, outOfFace }
  }
  for (const key of tables.unsupportedKeys) {
    problems.push(
      `✗ 目标发现：${rel}/Cargo.toml 出现 auto* 开关 \`${key}\`——自动发现面被改写，` +
        `发现逻辑不辖该形态（拒绝猜测）。请扩展 scripts/test-exec.ts 的目标发现后重跑`,
    )
  }

  // lib：显式 [lib] 表或 src/lib.rs 自动发现；`test = false` 关单测、`doctest = false` 关 doc-test。
  const libDeclared = tables.lib !== null
  const libPresent = libDeclared || existsSync(join(dir, 'src', 'lib.rs'))
  if (libPresent) {
    const libName = tables.lib?.name ?? packageName.replace(/-/g, '_')
    if (tables.lib?.harness === false) {
      problems.push(
        `✗ 目标发现：${rel} 的 [lib] 声明 harness = false——自定义 harness 的 lib 单测` +
          `不属 libtest 面，需显式归入非并发入口（当前发现逻辑不支持）`,
      )
    } else if (tables.lib?.test !== false) {
      targets.push({ package: packageName, target: libName, kind: 'lib', harnessFalse: false, cwd: dir })
    }
    if (tables.lib?.doctest !== false) {
      targets.push({ package: packageName, target: libName, kind: 'doc', harnessFalse: false, cwd: dir })
    }
  }

  // bin：显式 [[bin]] 或 src/main.rs、src/bin/*.rs、src/bin/*/main.rs 自动发现。
  const binNames = new Map<string, TargetDecl>()
  for (const decl of tables.bins) binNames.set(decl.name ?? stemName(decl.path ?? ''), decl)
  for (const candidate of [
    { name: packageName, path: join('src', 'main.rs') },
  ].concat(
    listDir(join(dir, 'src', 'bin')).flatMap((entry) => {
      const entryPath = join(dir, 'src', 'bin', entry)
      if (entry.endsWith('.rs')) return [{ name: stemName(entry), path: join('src', 'bin', entry) }]
      if (isDirectory(entryPath) && existsSync(join(entryPath, 'main.rs'))) {
        return [{ name: entry, path: join('src', 'bin', entry, 'main.rs') }]
      }
      return []
    }),
  )) {
    if (!existsSync(join(dir, candidate.path))) continue
    if (!binNames.has(candidate.name)) binNames.set(candidate.name, { name: candidate.name, path: candidate.path })
  }
  for (const [name, decl] of binNames) {
    if (decl.harness === false) {
      problems.push(`✗ 目标发现：${rel} 的 [[bin]] ${name} 声明 harness = false——自定义 harness 不属并发面，需显式归入非并发入口`)
      continue
    }
    if (decl.test !== false) {
      targets.push({ package: packageName, target: name, kind: 'bin', harnessFalse: false, cwd: dir })
    }
  }

  // 集成测试：显式 [[test]] 或 tests/*.rs、tests/*/main.rs 自动发现。
  const testDecls = new Map<string, TargetDecl>()
  for (const decl of tables.tests) testDecls.set(decl.name ?? stemName(decl.path ?? ''), decl)
  for (const candidate of listDir(join(dir, 'tests')).flatMap((entry) => {
      const entryPath = join(dir, 'tests', entry)
      if (entry.endsWith('.rs')) return [{ name: stemName(entry) }]
      if (isDirectory(entryPath) && existsSync(join(entryPath, 'main.rs'))) return [{ name: entry }]
      return []
    })) {
    if (!testDecls.has(candidate.name)) testDecls.set(candidate.name, { name: candidate.name })
  }
  for (const [name, decl] of testDecls) {
    if (decl.test === false) continue
    targets.push({
      package: packageName,
      target: name,
      kind: 'test',
      harnessFalse: decl.harness === false,
      cwd: dir,
    })
  }

  // example / bench 不在 `cargo test` 默认执行面（只构建 example、不跑 bench），仅提示。
  for (const key of ['benches', 'examples'] as const) {
    for (const decl of tables[key]) {
      const name = decl.name ?? stemName(decl.path ?? '')
      if (name !== '') outOfFace.push(`${rel}/${key === 'benches' ? 'bench' : 'example'}:${name}`)
    }
    for (const entry of listDir(join(dir, key))) {
      if (entry.endsWith('.rs')) outOfFace.push(`${rel}/${key === 'benches' ? 'bench' : 'example'}:${stemName(entry)}`)
    }
  }

  return { targets, problems, outOfFace }
}

/** 发现工作区全部测试目标（口径 = `cargo test` 默认执行面）。 */
export function discoverTargets(rootDir: string): Discovery {
  const srcTauriDir = join(rootDir, SRC_TAURI_DIR_NAME)
  const problems: string[] = []
  const targets: DiscoveredTarget[] = []
  const outOfFace: string[] = []

  const rootTables = readManifest(srcTauriDir)
  if (rootTables === null) {
    return {
      targets,
      problems: [`✗ 目标发现：Rust 根 manifest 不存在：${SRC_TAURI_DIR_NAME}/Cargo.toml`],
      outOfFace,
    }
  }
  const packageDirs: { dir: string; rel: string }[] = []
  if (rootTables.package.name !== undefined) {
    packageDirs.push({ dir: srcTauriDir, rel: SRC_TAURI_DIR_NAME })
  }
  const memberPatterns = rootTables.workspace.members
  if (memberPatterns === undefined) {
    if (packageDirs.length === 0) {
      problems.push(`✗ 目标发现：${SRC_TAURI_DIR_NAME}/Cargo.toml 既无 [package] 也无 [workspace] members`)
    }
  } else {
    const expanded = expandMemberGlobs(srcTauriDir, memberPatterns)
    problems.push(...expanded.problems)
    for (const dir of expanded.dirs) {
      packageDirs.push({ dir, rel: relative(rootDir, dir) })
    }
  }

  const seen = new Set<string>()
  for (const { dir, rel } of packageDirs) {
    const tables = readManifest(dir)
    if (tables === null) {
      problems.push(`✗ 目标发现：成员目录缺 Cargo.toml：${rel}`)
      continue
    }
    const found = discoverPackage(dir, rel, tables)
    problems.push(...found.problems)
    outOfFace.push(...found.outOfFace)
    for (const target of found.targets) {
      const key = runKey(target)
      if (seen.has(key)) continue
      seen.add(key)
      targets.push(target)
    }
  }
  return { targets, problems, outOfFace }
}

// ── 两个入口的覆盖守门 ───────────────────────────────────────────────────

/** shell 控制符：每个字符都是新命令的起点（`&&` / `||` / `;` / 管道 / 子 shell 括号）。 */
const SHELL_CONTROL_CHARS = new Set(['&', '|', ';', '(', ')'])

/**
 * 命令名前可出现的 shell 前缀词（保留字 / 内建）：`if bun …` / `command bun …` /
 * `env VAR=1 bun …` 都是合法调用形态，剥掉后仍算命令位置（避免偏严假红）。
 */
const COMMAND_PREFIX_WORDS = new Set([
  'command',
  'exec',
  'env',
  'nohup',
  'time',
  'if',
  'then',
  'else',
  'elif',
  'do',
  'while',
  'until',
  '!',
])

/** `VAR=value` 赋值前缀（`VAR=x bun …`）同样不改变命令位置。 */
const ENV_ASSIGNMENT_PREFIX = /^[A-Za-z_][A-Za-z0-9_]*=/

/** 命令行首的书写装饰（YAML 列表项 / `run:` 键），不含命令语义。 */
const COMMAND_LINE_DECORATIONS = new Set(['-', 'run:'])

/**
 * 把一行 shell/YAML 文本切成词：引号只脱壳，引号内的空格与控制符并入同一个词——
 * `echo "bun scripts/test-exec.ts"` 因此是「命令词 echo + 一个说明文字词」，不会被
 * 误当成 `bun` 接线；而 `bun "scripts/test-exec.ts" check` 脱壳后仍是 `bun` 接线。
 * 引号外的 `& | ; ( )` 一律切成独立控制符 token（`check;` 也能归到 `check`）。
 */
function tokenizeShellLine(line: string): string[] {
  const tokens: string[] = []
  let current = ''
  let started = false
  const flush = (): void => {
    if (started) tokens.push(current)
    current = ''
    started = false
  }
  for (let i = 0; i < line.length; i += 1) {
    const c = line[i] as string
    if (c === "'" || c === '"') {
      let j = i + 1
      while (j < line.length) {
        const inner = line[j] as string
        if (c === '"' && inner === '\\') {
          current += line.slice(j, j + 2)
          j += 2
          continue
        }
        if (inner === c) break
        current += inner
        j += 1
      }
      started = true
      i = j
      continue
    }
    if (c === ' ' || c === '\t') {
      flush()
      continue
    }
    if (SHELL_CONTROL_CHARS.has(c)) {
      flush()
      tokens.push(c)
      continue
    }
    current += c
    started = true
  }
  flush()
  return tokens
}

/** 剥掉行首装饰、前缀词与环境赋值，返回命令位置起的 argv；全被剥光则为空。 */
function stripCommandPrefixes(tokens: string[]): string[] {
  for (let start = 0; start < tokens.length; start += 1) {
    const token = tokens[start] as string
    if (
      COMMAND_LINE_DECORATIONS.has(token) ||
      COMMAND_PREFIX_WORDS.has(token) ||
      ENV_ASSIGNMENT_PREFIX.test(token)
    ) {
      continue
    }
    return tokens.slice(start)
  }
  return []
}

/**
 * 把脚本内容切成**命令位置**的 argv 列表：只看非注释行，行内按 shell 分隔符切段，
 * 逐段剥前缀。判据必须落在命令位置——`echo "bun scripts/test-exec.ts"` 这类说明文字
 * 虽然含命令字样，命令词却是 `echo`，不构成接线。上一版只判「非注释行含子串」，把
 * scripts/test.sh 三条真命令全包成 `echo "…"` 后三个入口一起假绿（#1112 第三轮审查）。
 */
function shellCommands(content: string): string[][] {
  const commands: string[][] = []
  for (const rawLine of content.split('\n')) {
    const line = rawLine.trim()
    if (line === '' || line.startsWith('#')) continue
    let segment: string[] = []
    const flush = (): void => {
      const argv = stripCommandPrefixes(segment)
      if (argv.length > 0) commands.push(argv)
      segment = []
    }
    for (const token of tokenizeShellLine(line)) {
      if (SHELL_CONTROL_CHARS.has(token)) flush()
      else segment.push(token)
    }
    flush()
  }
  return commands
}

/** scripts/test.sh 命令行里的三入口判据。 */
export interface EntryWiring {
  /** `cargo test … --test <name>` 的 name 名单。 */
  tests: Set<string>
  /** `cargo nextest run … --test <name>` 的 name 名单（nextest 入口承接目标）。 */
  nextest: Set<string>
  /** 是否有 `cargo test … --doc`（doc 入口存在性）。 */
  doc: boolean
  /**
   * doc 入口的命令行是否带 workspace 范围（`--workspace` / `--all`）。
   * 只查 `--doc` 存在不够：退化成 `cargo test --doc`（非虚拟 workspace 下默认
   * 只作用根包）会让成员 crate 的 doctest 静默漏跑而守门仍然绿。
   */
  docWorkspace: boolean
  /**
   * 是否**运行**并发入口（`bun scripts/test-exec.ts [run]`）。入口自检行
   * （`… check`）与 `… plan` 不算运行接线——否则把运行行删掉、只留自检行也能假绿。
   */
  parallel: boolean
}

/**
 * 解析 scripts/test.sh：只看**命令位置**的命令——注释、说明文字（引号里的命令字样）
 * 与 echo 参数都不算接线。每条命令按命令词匹配：`cargo test …` 才贡献
 * `--test <name>` / `--doc` 判据，`cargo nextest run …` 才贡献 nextest 承接名单，
 * `bun … scripts/test-exec.ts` 才贡献并发入口判据。
 */
export function parseEntryWiring(content: string): EntryWiring {
  const tests = new Set<string>()
  const nextest = new Set<string>()
  let doc = false
  let docWorkspace = false
  let parallel = false
  for (const argv of shellCommands(content)) {
    if (argv[0] === 'bun') {
      // 并发入口：`bun [前置参数] scripts/test-exec.ts [run]`（引号形态由词切分归一）；
      // `check` / `plan` 是守门与清单命令，不跑测试，不算运行接线。
      const marker = argv.indexOf(PARALLEL_ENTRY_MARKER, 1)
      if (marker !== -1) {
        const subcommand = argv.slice(marker + 1).find((t) => !t.startsWith('-'))
        if (subcommand === undefined || subcommand === 'run') parallel = true
      }
      continue
    }
    if (
      argv[0] === 'cargo' &&
      argv[1] === NEXTEST_SUBCOMMAND &&
      argv[2] === NEXTEST_RUN_SUBCOMMAND
    ) {
      const args = argv.slice(3)
      for (let i = 0; i < args.length; i += 1) {
        if (args[i] === DELEGATED_TARGET_FLAG) {
          const name = args[i + 1] ?? ''
          if (name !== '') nextest.add(name)
        }
      }
      continue
    }
    if (argv[0] !== 'cargo' || argv[1] !== 'test') continue
    const args = argv.slice(2)
    if (args.includes(DOC_FLAG)) {
      doc = true
      // workspace 范围必须落在同一条 `cargo test` 命令里；`--all` 是别名，精确 token
      // 匹配（`--all-targets` / `--all-features` 不算，与 check-structure.ts 同口径）。
      if (args.includes(WORKSPACE_FLAG) || args.includes(ALL_FLAG)) docWorkspace = true
    }
    for (let i = 0; i < args.length; i += 1) {
      if (args[i] === DELEGATED_TARGET_FLAG) {
        const name = args[i + 1] ?? ''
        if (name !== '') tests.add(name)
      }
    }
  }
  return { tests, nextest, doc, docWorkspace, parallel }
}

export interface CoverageResult {
  problems: string[]
  targets: DiscoveredTarget[]
  /** 并发入口承接（lib 单测 + bin 单测 + harness=true 集成测试）。 */
  parallel: DiscoveredTarget[]
  /** nextest 入口承接的集成测试（NEXTEST_TARGETS 登记，进程级 per-test 调度）。 */
  nextest: DiscoveredTarget[]
  /** 非并发入口承接的自定义 harness 集成测试（`--test <name>`）。 */
  delegated: DiscoveredTarget[]
  /** cargo 自有入口承接的 doc-test。 */
  doc: DiscoveredTarget[]
  outOfFace: string[]
}

/**
 * 门禁自身的接线宿主（守门规则④）：覆盖守门挂在 `scripts/check.sh` 质量门槛序列
 * 与 CI frontend job。两处接线在本机单元测试里不会被管线执行（check.sh 要 cargo、
 * CI 要远端），按 AGENTS.md「接线在本机 CI 不构建的分支内时以源码扫描守门替代」
 * （先例 #959/#961）落成源码扫描——删除接线即 `check` 退出码非零，而 `check` 正是
 * 单元测试与实际管线的共同观察面。
 */
const GATE_WIRING_HOSTS: readonly { rel: string; label: string }[] = [
  { rel: join('scripts', 'check.sh'), label: 'scripts/check.sh 质量门槛序列' },
  { rel: join('.github', 'workflows', 'build.yml'), label: 'CI frontend job' },
]

/**
 * 源码扫描：**命令位置**是否出现「`bun` → `scripts/test-exec.ts` → `check`」的命令词序。
 * 按 shell 词切分而非逐字匹配整行——加引号、多空白、`bun --smol` 之类的前置参数、
 * `check` 之后的额外参数、`command bun …` / `if bun …` / `VAR=x bun …` 都不假红；
 * 删掉调用或换成别的子命令（plan/run）即判定未接线。
 *
 * 关键约束：`bun` 必须落在命令位置（见 shellCommands）。否则 `echo "…（bun
 * scripts/test-exec.ts check）…"` 这类**说明文字**会被误判成接线——实测把真实
 * check.sh 的命令改成 `plan` 后，仅剩的 echo 标签行仍能假绿（#1112 审查自证）。
 */
export function gateWiredIn(content: string): boolean {
  const needle = [PARALLEL_ENTRY_MARKER, 'check']
  for (const argv of shellCommands(content)) {
    if (argv[0] !== 'bun') continue
    let cursor = 0
    for (let i = 1; i < argv.length; i += 1) {
      if (argv[i] === needle[cursor]) cursor += 1
      if (cursor === needle.length) return true
    }
  }
  return false
}

/** 门禁接线守门（守门规则④）：两块宿主都必须源码扫描到 `check` 调用。 */
function gateWiringProblems(rootDir: string): string[] {
  const problems: string[] = []
  for (const host of GATE_WIRING_HOSTS) {
    const path = join(rootDir, host.rel)
    if (!existsSync(path)) {
      problems.push(`✗ 覆盖守门接线：${host.label} 宿主文件不存在：${host.rel}`)
      continue
    }
    if (!gateWiredIn(readFileSync(path, 'utf8'))) {
      problems.push(
        `✗ 覆盖守门接线：${host.label}（${host.rel}）未调用守门命令（\`bun ${PARALLEL_ENTRY_MARKER} check\`）——` +
          `覆盖守门不在该管线执行，接线删除即红（源码扫描，先例 #959/#961）`,
      )
    }
  }
  return problems
}

/**
 * 执行器等价性守门（守门规则⑤）：执行器直接 spawn 测试二进制，**不**复制 cargo
 * 运行期注入的环境（`CARGO_MANIFEST_DIR` / `CARGO_PKG_*` / `OUT_DIR` / 动态库搜索
 * 路径）。只对齐其中一部分会给「已等价」的假信心，故取 fail loud 处置：测试运行期
 * （`env::var` / `env::var_os`；编译期 `env!` 不受影响）不得读这些变量，命中即红，
 * 由引入者显式扩展执行器并更新本守门。构建脚本的构建期读取不属测试运行期，跳过。
 * 扫描前经结构守门的 Rust 词法掩码（`maskNonCode(…, keepLiterals=true)`）掩去
 * 注释、保留字面量——注释里提到这些变量名不误报，字面量本身才是判据。
 */
const CARGO_RUNTIME_ENV_PATTERN = /\benv::var(?:_os)?\s*\(\s*"(CARGO_[A-Z0-9_]+|OUT_DIR)"/g

function cargoRuntimeEnvProblems(rootDir: string): string[] {
  const problems: string[] = []
  const walk = (dir: string): void => {
    if (!isDirectory(dir)) return
    for (const entry of readdirSync(dir).sort()) {
      const path = join(dir, entry)
      if (isDirectory(path)) {
        if (entry === 'target') continue
        walk(path)
        continue
      }
      if (!entry.endsWith('.rs') || entry === 'build.rs') continue
      const masked = maskNonCode(readFileSync(path, 'utf8'), true)
      for (const match of masked.matchAll(CARGO_RUNTIME_ENV_PATTERN)) {
        const line = masked.slice(0, match.index ?? 0).split('\n').length
        problems.push(
          `✗ 执行器等价性：${relative(rootDir, path)}:${line} 运行期读 cargo 注入环境变量 \`${match[1]}\`——` +
            `执行器只对齐 cwd 与 RUST_TEST_THREADS，不复制该环境，测试将读到未定义值。` +
            `请改走 cwd 相对定位，或先扩展 scripts/test-exec.ts 的 runChild 并更新本守门（issue #1112）`,
        )
      }
    }
  }
  walk(join(rootDir, SRC_TAURI_DIR_NAME))
  return problems
}

/** nextest 仓库配置里 `binary(<name>)` 排除项的捕获式（组 1 = 是否带 `not`）。 */
const NEXTEST_BINARY_RE = /(not\s+)?binary\(\s*([A-Za-z0-9_-]+)\s*\)/g

/**
 * 覆盖守门规则③后半（ticket #1496）：`src-tauri/.config/nextest.toml` 的
 * default-filter 排除名单必须与 `harness = false` 目标清单**双向全等**——
 * 多排一个 libtest 目标 = 该目标被静默移出 nextest 调度面（新目标/共享目标漏跑即红）；
 * 少排一个自定义 harness 目标 = nextest 列举它时 fail loud，同样按红处置：
 * 「排除名单即自定义 harness 清单」是单一事实源，两处漂移都不许。
 */
function nextestConfigProblems(rootDir: string, customHarnessNames: string[]): string[] {
  const problems: string[] = []
  const path = join(rootDir, NEXTEST_CONFIG_REL)
  if (!existsSync(path)) {
    problems.push(
      `✗ 覆盖守门：nextest 仓库配置不存在：${NEXTEST_CONFIG_REL}——e2e 新目标经 nextest` +
        ` 调度（登记：${NEXTEST_TARGETS.join(' / ')}），配置缺失无法核对排除名单`,
    )
    return problems
  }
  const filterLine = readFileSync(path, 'utf8')
    .split('\n')
    .find((line) => /^\s*default-filter\s*=/.test(line))
  if (filterLine === undefined) {
    problems.push(
      `✗ 覆盖守门：${NEXTEST_CONFIG_REL} 缺 \`default-filter\`——自定义 harness 目标` +
        `（${customHarnessNames.join(' / ') || '（无）'}）会被 nextest 纳入列举并失败；` +
        `排除名单须与 \`harness = false\` 目标清单全等`,
    )
    return problems
  }
  const excluded = new Set<string>()
  for (const match of filterLine.matchAll(NEXTEST_BINARY_RE)) {
    const name = match[2] ?? ''
    if (match[1] === undefined) {
      problems.push(
        `✗ 覆盖守门：${NEXTEST_CONFIG_REL} 的 default-filter 含未取反的 \`binary(${name})\`` +
          `——除它以外的目标会被静默移出 nextest 调度面（默认过滤器只允许 \`not binary(<自定义 harness>)\` 形态）`,
      )
      continue
    }
    excluded.add(name)
  }
  for (const name of customHarnessNames) {
    if (!excluded.has(name)) {
      problems.push(
        `✗ 覆盖守门：自定义 harness 目标 ${name} 不在 ${NEXTEST_CONFIG_REL} 的 default-filter` +
          ` 排除名单——nextest 列举自定义 harness 会失败；排除名单须与 \`harness = false\` 目标清单全等`,
      )
    }
  }
  for (const name of excluded) {
    if (!customHarnessNames.includes(name)) {
      problems.push(
        `✗ 覆盖守门：${NEXTEST_CONFIG_REL} 的 default-filter 排除了 ${name}，但它不是` +
          ` \`harness = false\` 目标——该目标会被 nextest 静默漏跑`,
      )
    }
  }
  return problems
}

/** 覆盖守门：目标清单 ⇔ 三个入口的并集（互斥且无遗漏）。 */
export function checkCoverage(rootDir: string): CoverageResult {
  const discovery = discoverTargets(rootDir)
  const problems = [...discovery.problems]
  const delegated = discovery.targets.filter((t) => t.kind === 'test' && t.harnessFalse)
  const delegatedNames = new Set(delegated.map((t) => t.target))
  const doc = discovery.targets.filter((t) => t.kind === 'doc')

  if (discovery.targets.length === 0 && problems.length === 0) {
    problems.push('✗ 覆盖守门：工作区测试目标清单为空——拒绝以空集假绿通过')
  }

  // nextest 承接登记（规则③前半）：登记名必须是真实存在的 harness = true 集成测试目标。
  const nextest: DiscoveredTarget[] = []
  for (const name of NEXTEST_TARGETS) {
    const target = discovery.targets.find((t) => t.kind === 'test' && t.target === name)
    if (target === undefined) {
      problems.push(
        `✗ 覆盖守门：登记为 nextest 承接的目标 \`${name}\` 不在工作区测试目标清单里` +
          `（目标被删或登记漂移）——nextest 调度面与登记不一致`,
      )
      continue
    }
    if (target.harnessFalse) {
      problems.push(
        `✗ 覆盖守门：登记为 nextest 承接的 ${targetKey(target)} 是 \`harness = false\`` +
          ` 自定义 runner——nextest 不能列举它（列举即 fail loud），应归非并发入口`,
      )
      continue
    }
    nextest.push(target)
  }
  const nextestKeys = new Set(nextest.map((t) => runKey(t)))
  const parallel = discovery.targets.filter(
    (t) => t.kind !== 'doc' && !(t.kind === 'test' && t.harnessFalse) && !nextestKeys.has(runKey(t)),
  )

  const wiring = parseEntryWiring(readFileSync(join(rootDir, TEST_SH_REL), 'utf8'))
  // 规则③前半：登记 ⇔ scripts/test.sh 的 nextest 运行命令名单（双向全等）。
  for (const target of nextest) {
    if (!wiring.nextest.has(target.target)) {
      problems.push(
        `✗ 覆盖守门：${targetKey(target)} 登记为 nextest 承接，但 ${TEST_SH_REL} 无` +
          ` \`cargo ${NEXTEST_SUBCOMMAND} ${NEXTEST_RUN_SUBCOMMAND} … ${DELEGATED_TARGET_FLAG} ${target.target}\`` +
          `——nextest 入口被删/改写即红（不静默降级回并发入口：那会退回进程内单线程串行）`,
      )
    }
  }
  const nextestNames = new Set(nextest.map((t) => t.target))
  for (const name of wiring.nextest) {
    if (!nextestNames.has(name)) {
      problems.push(
        `✗ 覆盖守门：${TEST_SH_REL} 的 \`cargo ${NEXTEST_SUBCOMMAND} ${NEXTEST_RUN_SUBCOMMAND} …` +
          ` ${DELEGATED_TARGET_FLAG} ${name}\` 不对应任何登记为 nextest 承接的目标（登记失效或拼写漂移）`,
      )
    }
  }
  problems.push(...nextestConfigProblems(rootDir, [...delegatedNames]))

  for (const target of delegated) {
    if (!wiring.tests.has(target.target)) {
      problems.push(
        `✗ 覆盖守门：${targetKey(target)} 声明 harness = false（自定义 runner），` +
          `但 ${TEST_SH_REL} 的非并发入口没有 \`${DELEGATED_TARGET_FLAG} ${target.target}\`——` +
          `三个入口的覆盖范围都不含它，新增自定义 harness 目标被静默漏跑`,
      )
    }
  }
  for (const name of wiring.tests) {
    if (!delegatedNames.has(name)) {
      problems.push(
        `✗ 覆盖守门：${TEST_SH_REL} 的 \`${DELEGATED_TARGET_FLAG} ${name}\` 不对应任何` +
          ` harness = false 目标（登记失效或拼写漂移：非并发入口的每一项都必须是自定义 harness 目标）`,
      )
    }
  }
  if (doc.length > 0 && !wiring.doc) {
    problems.push(
      `✗ 覆盖守门：工作区有 ${doc.length} 个 doc-test 目标，但 ${TEST_SH_REL} 缺` +
        ` \`cargo test --workspace ${DOC_FLAG}\`——doc-test 静默漏跑`,
    )
  }
  if (doc.length > 0 && wiring.doc && !wiring.docWorkspace) {
    problems.push(
      `✗ 覆盖守门：${TEST_SH_REL} 的 \`${DOC_FLAG}\` 缺 workspace 范围` +
        `（\`${WORKSPACE_FLAG}\` 或 \`${ALL_FLAG}\`）——非虚拟 workspace 下只跑根包的` +
        ` doctest，其余成员 crate 的 doc-test 静默漏跑（守门仍绿）`,
    )
  }
  if (doc.length === 0 && wiring.doc) {
    problems.push(`✗ 覆盖守门：${TEST_SH_REL} 有 \`${DOC_FLAG}\`，但工作区无 doc-test 目标（登记失效）`)
  }
  if (!wiring.parallel) {
    problems.push(
      `✗ 覆盖守门：${TEST_SH_REL} 未调用并发入口（${PARALLEL_ENTRY_MARKER}）——` +
        `单元测试与集成测试未由统一执行器调度（删除接线即红）`,
    )
  }
  problems.push(...gateWiringProblems(rootDir))
  problems.push(...cargoRuntimeEnvProblems(rootDir))

  return {
    problems,
    targets: discovery.targets,
    parallel,
    nextest,
    delegated,
    doc,
    outOfFace: discovery.outOfFace,
  }
}

function summarizeCoverage(result: CoverageResult): string {
  const count = (kind: TargetKind): number => result.parallel.filter((t) => t.kind === kind).length
  const docPkgs = new Set(result.doc.map((t) => t.package)).size
  return (
    `✓ 测试执行覆盖守门：目标 ${result.targets.length} 个 = ` +
    `并发入口 ${result.parallel.length}（lib 单测 ${count('lib')} + bin 单测 ${count('bin')} + 集成测试 ${count('test')}）` +
    ` ⊎ nextest 入口 ${result.nextest.length}（${result.nextest.map((t) => t.target).join(' / ') || '（无）'}，进程级 per-test）` +
    ` ⊎ 非并发入口 ${result.delegated.length + result.doc.length}` +
    `（cargo 自有 runner：${result.delegated.map((t) => t.target).join(' / ') || '（无）'}` +
    ` + doc-test ${result.doc.length} 个，覆盖 ${docPkgs} 个包）` +
    (result.outOfFace.length > 0 ? ` · 默认执行面外 ${result.outOfFace.length} 个（example/bench，不跑）` : '')
  )
}

// ── 并发执行 ─────────────────────────────────────────────────────────────

interface ChildResult {
  code: number
  stdout: string
  stderr: string
}

function runChild(
  command: string,
  args: string[],
  opts: { cwd: string; env?: NodeJS.ProcessEnv; streamStderr?: boolean },
): Promise<ChildResult> {
  return new Promise((resolvePromise) => {
    const child = spawn(command, args, { cwd: opts.cwd, env: opts.env ?? process.env })
    let stdout = ''
    let stderr = ''
    child.stdout?.setEncoding('utf8')
    child.stderr?.setEncoding('utf8')
    child.stdout?.on('data', (chunk: string) => {
      stdout += chunk
    })
    child.stderr?.on('data', (chunk: string) => {
      stderr += chunk
      if (opts.streamStderr === true) process.stderr.write(chunk)
    })
    child.on('error', (error) => {
      stderr += `${String(error)}\n`
    })
    child.on('close', (code) => {
      resolvePromise({ code: code ?? -1, stdout, stderr })
    })
  })
}

/** 构建一次（cargo test --no-run），返回 cargo 报告的可执行测试二进制键集合。 */
interface CargoMessage {
  reason?: string
  package_id?: string
  target?: { name?: string; kind?: string[] }
  profile?: { test?: boolean }
  executable?: string | null
  success?: boolean
}

function parseCargoMessage(line: string): CargoMessage | null {
  if (!line.startsWith('{')) return null
  try {
    return JSON.parse(line) as CargoMessage
  } catch {
    return null
  }
}

async function buildTestBinaries(
  cargo: string,
  srcTauriDir: string,
): Promise<{ executables: Map<string, string>; problems: string[]; ms: number }> {
  const start = Date.now()
  const result = await runChild(
    cargo,
    ['test', '--workspace', '--no-run', '--message-format=json-render-diagnostics'],
    { cwd: srcTauriDir, streamStderr: true },
  )
  const ms = Date.now() - start
  const problems: string[] = []
  const executables = new Map<string, string>()
  let buildSucceeded = result.code === 0
  for (const line of result.stdout.split('\n')) {
    const message = parseCargoMessage(line)
    if (message === null) continue
    if (message.reason === 'build-finished') buildSucceeded = message.success === true
    if (message.reason !== 'compiler-artifact') continue
    if (message.profile?.test !== true) continue
    const executable = message.executable
    if (executable === null || executable === undefined) continue
    const packageName = packageNameFromId(message.package_id ?? '')
    const targetName = message.target?.name ?? ''
    if (packageName === '' || targetName === '') continue
    const kind = artifactTargetKind(message.target?.kind ?? [])
    if (kind === null) continue
    executables.set(`${packageName}::${targetName}::${kind}`, executable)
  }
  if (!buildSucceeded) {
    problems.push('✗ 构建失败：`cargo test --workspace --no-run` 非零退出（编译错误见上方 cargo 输出）')
  }
  return { executables, problems, ms }
}

/** cargo package_id → 包名（`path+file:///…#<name>@<version>` 形态）。 */
function packageNameFromId(id: string): string {
  const hash = id.indexOf('#')
  const tail = hash === -1 ? id : id.slice(hash + 1)
  const at = tail.indexOf('@')
  return at === -1 ? tail : tail.slice(0, at)
}

/** cargo artifact 的 target.kind → 目标种类（与发现侧同一口径，核对键才可比）。 */
function artifactTargetKind(kinds: string[]): TargetKind | null {
  if (kinds.includes('test')) return 'test'
  if (kinds.includes('bin')) return 'bin'
  if (kinds.includes('lib') || kinds.includes('rlib') || kinds.includes('cdylib')) return 'lib'
  if (kinds.includes('staticlib') || kinds.includes('dylib') || kinds.includes('proc-macro')) return 'lib'
  return null
}

/** 固定并发度的任务池：全局并行度由执行器统一控制（不依赖各二进制的 libtest 线程数）。 */
async function runPool<T>(items: T[], jobs: number, worker: (item: T) => Promise<void>): Promise<void> {
  let cursor = 0
  const width = Math.max(1, Math.min(jobs, items.length))
  await Promise.all(
    Array.from({ length: width }, async () => {
      for (;;) {
        const index = cursor
        cursor += 1
        if (index >= items.length) return
        await worker(items[index] as T)
      }
    }),
  )
}

interface TargetOutcome {
  key: string
  ms: number
  code: number
  output: string
  passed: number
  failed: number
  ignored: number
}

const TEST_RESULT_RE =
  /^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; \d+ measured; \d+ filtered out; finished in ([\d.]+)s$/gm

function parseTestResult(output: string): { passed: number; failed: number; ignored: number } {
  let passed = 0
  let failed = 0
  let ignored = 0
  for (const match of output.matchAll(TEST_RESULT_RE)) {
    passed = Number(match[1])
    failed = Number(match[2])
    ignored = Number(match[3])
  }
  return { passed, failed, ignored }
}

function formatSeconds(ms: number): string {
  return `${(ms / 1000).toFixed(2)}s`
}

interface RunOptions {
  rootDir: string
  jobs: number
}

async function runAll(options: RunOptions): Promise<number> {
  const { rootDir, jobs } = options
  const coverage = checkCoverage(rootDir)
  if (coverage.problems.length > 0) printProblems(coverage.problems, '❌ 测试执行覆盖守门失败')
  else console.log(summarizeCoverage(coverage))

  const srcTauriDir = join(rootDir, SRC_TAURI_DIR_NAME)
  const cargo = process.env.CARGO ?? 'cargo'
  console.log(
    `▶ 构建测试二进制（一次构建全部 target）：cargo test --workspace --no-run` +
      `（缓存命中时秒级返回；冷构建的编译耗时见 cargo 输出）`,
  )
  const build = await buildTestBinaries(cargo, srcTauriDir)
  console.log(`  · 构建阶段 ${formatSeconds(build.ms)}`)
  const problems = [...coverage.problems, ...build.problems]

  // 第四道交叉核对：cargo 实际构建出的测试二进制集合 ⇔ 目标发现清单（lib/bin/集成测试）。
  const expected = new Map(
    coverage.targets
      .filter((t) => t.kind !== 'doc')
      .map((t) => [runKey(t), t] as const),
  )
  for (const key of expected.keys()) {
    if (!build.executables.has(key)) {
      problems.push(
        `✗ 执行面漂移：目标清单里的 ${key} 未被 cargo 构建（cargo test --workspace --no-run 无对应可执行文件）`,
      )
    }
  }
  for (const key of build.executables.keys()) {
    if (!expected.has(key)) {
      problems.push(
        `✗ 执行面漂移：cargo 构建出清单外的测试二进制 ${key}——目标发现逻辑漏项，` +
          `该目标会被静默漏跑（请扩展 scripts/test-exec.ts 的目标发现）`,
      )
    }
  }
  if (problems.length > 0) {
    printProblems(problems, '❌ 测试执行前检查失败')
  }

  const delegatedKeys = new Set(coverage.delegated.map((t) => runKey(t)))
  const nextestKeys = new Set(coverage.nextest.map((t) => runKey(t)))
  const queue = [...expected.keys()]
    .filter((key) => !delegatedKeys.has(key) && !nextestKeys.has(key))
    .sort()
  const cpuCount = cpus().length
  const width = Math.max(1, Math.min(jobs, queue.length))
  console.log(
    `▶ 并发执行 ${queue.length} 个测试二进制（全局并行度 ${width} = ` +
      `min(并行度上限 ${jobs}（默认 CPU 数 ${cpuCount}）, 待跑二进制数 ${queue.length})；` +
      `每个二进制 RUST_TEST_THREADS=1（只约束 libtest 线程），libtest 线程数 = ${width}）`,
  )
  if (jobs > cpuCount) {
    console.log(
      `  · 提示：--jobs ${jobs} 超过 CPU 数 ${cpuCount}——这是显式超订（默认取 CPU 数），` +
        `刻意压测/限流时才这样传`,
    )
  }
  if (coverage.delegated.length > 0) {
    console.log(
      `  · 非并发入口承接 ${coverage.delegated.length} 个（${coverage.delegated.map((t) => t.target).join(' / ')}）+ ` +
        `doc-test ${coverage.doc.length} 个，由 ${TEST_SH_REL} 的 cargo 自有入口执行`,
    )
  }
  if (coverage.nextest.length > 0) {
    console.log(
      `  · nextest 入口承接 ${coverage.nextest.length} 个（${coverage.nextest.map((t) => t.target).join(' / ')}），` +
        `由 ${TEST_SH_REL} 的 \`cargo ${NEXTEST_SUBCOMMAND} ${NEXTEST_RUN_SUBCOMMAND}\` 进程级 per-test 调度（不在本入口队列，避免重复跑）`,
    )
  }

  const start = Date.now()
  const outcomes: TargetOutcome[] = []
  await runPool(queue, jobs, async (key) => {
    const target = expected.get(key)
    const executable = build.executables.get(key)
    if (target === undefined || executable === undefined) return
    const itemStart = Date.now()
    const result = await runChild(executable, [], {
      // cwd 与 cargo 口径一致（各包根，不是 workspace 根）：成员 crate 的测试若
      // 用相对路径，直接以 workspace 根为 cwd 会读到不同位置（Spec 审查发现）。
      // cargo 运行期注入的环境（CARGO_MANIFEST_DIR / CARGO_PKG_* / OUT_DIR /
      // 动态库搜索路径）**不复制**——部分复制会制造「已等价」的假信心；现状等价
      // 由守门规则⑤兜底（测试运行期读这些变量即红，见 cargoRuntimeEnvProblems）。
      cwd: target.cwd,
      env: { ...process.env, RUST_TEST_THREADS: '1' },
    })
    const ms = Date.now() - itemStart
    const counts = parseTestResult(`${result.stdout}\n${result.stderr}`)
    outcomes.push({
      key,
      ms,
      code: result.code,
      output: `${result.stdout}${result.stderr}`,
      ...counts,
    })
    const mark = result.code === 0 ? '✓' : '✗'
    const detail = result.code === 0 ? `${counts.passed} passed` : `exit ${result.code}`
    console.log(`  ${mark} ${key} ${formatSeconds(ms)}（${detail}${counts.ignored > 0 ? `, ${counts.ignored} ignored` : ''}）`)
  })
  const wallMs = Date.now() - start

  const failed = outcomes.filter((o) => o.code !== 0).sort((a, b) => a.key.localeCompare(b.key))
  if (failed.length > 0) {
    for (const outcome of failed) {
      console.error(`\n──── ✗ ${outcome.key}（exit ${outcome.code}，${formatSeconds(outcome.ms)}）────`)
      console.error(outcome.output.trimEnd())
    }
  }

  const sumMs = outcomes.reduce((acc, o) => acc + o.ms, 0)
  const slowest = outcomes.reduce<TargetOutcome | null>(
    (acc, o) => (acc === null || o.ms > acc.ms ? o : acc),
    null,
  )
  const totals = outcomes.reduce(
    (acc, o) => ({
      passed: acc.passed + o.passed,
      failed: acc.failed + o.failed,
      ignored: acc.ignored + o.ignored,
    }),
    { passed: 0, failed: 0, ignored: 0 },
  )
  const passCount = outcomes.length - failed.length
  console.log(
    `\n${failed.length === 0 ? '✅' : '❌'} 并发入口结果：${passCount}/${outcomes.length} 个二进制通过` +
      `（${totals.passed} passed / ${totals.failed} failed / ${totals.ignored} ignored）`,
  )
  console.log(
    `  · 墙钟 ${formatSeconds(wallMs)}（并行度 ${width}）；各二进制耗时之和 ${formatSeconds(sumMs)}；` +
      `串行等价 ≈ ${formatSeconds(sumMs + build.ms)}；加速 ${(sumMs / Math.max(wallMs, 1)).toFixed(2)}x` +
      (slowest !== null ? `；最长 ${slowest.key} ${formatSeconds(slowest.ms)}` : ''),
  )
  return failed.length === 0 ? 0 : 1
}

function printProblems(problems: string[], title: string): never {
  for (const problem of problems) console.error(problem)
  console.error(`${title}：${problems.length} 处问题（issue #1112）`)
  process.exit(1)
}

// ── CLI ─────────────────────────────────────────────────────────────────

interface CliOptions {
  command: 'run' | 'check' | 'plan'
  rootDir: string
  jobs: number
}

function parseArgs(argv: string[], defaultRoot: string): CliOptions {
  let command: CliOptions['command'] = 'run'
  let rootDir = defaultRoot
  let jobs = Number(process.env.LEDGER_TEST_JOBS ?? '') || cpus().length
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i]
    if (arg === 'run' || arg === 'check' || arg === 'plan') command = arg
    else if (arg === '--jobs') {
      i += 1
      const value = Number(argv[i] ?? '')
      if (!Number.isInteger(value) || value < 1) {
        console.error(`✗ --jobs 需要一个正整数，收到：${argv[i] ?? '（缺参数）'}`)
        process.exit(2)
      }
      jobs = value
    } else if (arg === '--root') {
      i += 1
      rootDir = resolve(argv[i] ?? '')
    } else {
      console.error(`✗ 未知参数：${arg}（用法：bun scripts/test-exec.ts [run|check|plan] [--jobs N] [--root DIR]）`)
      process.exit(2)
    }
  }
  return { command, rootDir, jobs }
}

async function main(): Promise<void> {
  const defaultRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
  const options = parseArgs(process.argv.slice(2), defaultRoot)
  if (options.command === 'plan') {
    const discovery = discoverTargets(options.rootDir)
    if (discovery.problems.length > 0) printProblems(discovery.problems, '❌ 目标发现失败')
    for (const target of discovery.targets) {
      const surface =
        target.kind === 'doc'
          ? '  doc-test（非并发入口）'
          : target.harnessFalse
            ? '  harness=false（非并发入口）'
            : NEXTEST_TARGETS.includes(target.target)
              ? '  nextest 入口（进程级 per-test）'
              : ''
      console.log(`${target.kind.padEnd(5)} ${targetKey(target)}${surface}`)
    }
    return
  }
  if (options.command === 'check') {
    const coverage = checkCoverage(options.rootDir)
    if (coverage.problems.length > 0) printProblems(coverage.problems, '❌ 测试执行覆盖守门失败')
    console.log(summarizeCoverage(coverage))
    return
  }
  process.exitCode = await runAll(options)
}

// 仅直接运行时执行 main；被其他工具 import 时只取导出的发现/守门逻辑。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main()
}
