#!/usr/bin/env bun
// 前端 workspace 结构守门（issue #1149 / spec #1148）：pnpm 子包骨架的边界门禁，
// 为每一次包抽取提供可证伪的边界基线。本脚本不移动业务代码，只核结构。
// 规则七类：
// ① 成员登记：packages/* 下的成员目录必须登记于本脚本 PACKAGES（磁盘 ↔ 清单双向
//    全等，清单漂移 fail loud）——pnpm-workspace.yaml 的 glob 自动纳管目录，能「漏
//    登记」的只有方向表登记册；新建成员目录不登记即红（删除即变红②）。
//    另核 pnpm-workspace.yaml 须以 glob 声明 packages/*（成员可为空，目录缺失即红）。
// ② 包依赖方向：每个包只能依赖 PACKAGES 已声明且方向允许的包；依赖未登记包、或
//    依赖不在自身 deps 方向表内的包即红。方向表随包抽取逐票补充；dependencies /
//    devDependencies / optionalDependencies / peerDependencies 四类一并核对（包间
//    测试边也是真实边界，方向表显式放行）。
// ③ 跨包引用形态：包内源码跨包引用只能走包名 `@ledger/*`；禁止 `@/` 别名（指向根
//    src，从包内使用必然穿越包边界）与相对路径穿越包边界（解析后落点在包目录外）。
// ④ 深导入禁令：`@ledger/x/sub` 形态必须命中目标包 package.json `exports` 的对应
//    入口（精确键 `./sub` 或 `./*` 通配；exports 为字符串视作仅暴露 `.`）。
// ⑤ 测试支持纯净性（issue #1152）：PACKAGES 内 testSupport: true 的包只许经
//    devDependencies 被消费（根包与成员包一并核对，dependencies/optionalDependencies/
//    peerDependencies 任一出现即红）；且测试支持包自身 dependencies 必须为空
//    （替身与接缝所需运行面全部走 devDependencies）——生产依赖图零测试支持内容。
// ⑥ 上行引用禁令（issue #1156）：登记目录不得引用登记的上层目标——上行越界接缝
//    归位后把方向固化成登记项，登记项唯一事实源为 FORBIDDEN_UPWARD_IMPORTS；
//    改动登记项即变红（登记表全等断言，issue #1156 验收判据）。
//    #1314 起 src/utils 全量成包 @ledger/utils，唯一登记项（dir: 'src/utils'）随
//    搬迁对象消失而收缩清空：包层上行形态改由规则②（方向表全等）、③（@/ 别名
//    禁令）、④（深导入 exports 入口）在包边界接管，登记机制保留——后续目录级
//    边界约束（如 ADR-0118 规则⑦深模块登记表）的登记处。
// ⑦ 深模块边界登记表（issue #1323 / ADR-0118 决策 7）：不成包的深模块以「允许消费
//    方白名单」守门，登记 { 模块文件 → 允许消费方闭集 }，登记项唯一事实源为
//    DEEP_MODULE_BOUNDARIES；扫描 src/ + packages/ 源文件（排除测试文件——单测引用
//    被测对象是天然形态，白名单表达生产消费面）的 import（@/ 别名与相对路径统一
//    解析落点后比对），消费方不在白名单内即红；登记模块文件不存在即红（改名/删除
//    后拒绝规则静默失效）；删除登记项即变红（登记表全等断言 + 夹具违规即红，
//    issue #1323 验收判据）。
// 删除即变红①：本脚本核对自身接线——scripts/check.sh 与 CI frontend job
//（.github/workflows/build.yml）中必须存在实际调用行（非注释、非 echo 展示行），
// 删除接线行即红（ADR-0087 断言强度：接线型守门的负向条目）。
// 扫描边界：文本级扫描，只掩码注释（行注释与块注释）后匹配 import 形态——import
// 说明符本身是字符串，掩码字符串会连靶一起抹掉；故字符串里的伪 import 靠 import/
// from 关键词上下文排除，模板字面量动态 import（反引号形态）与字符串内的关键字
// 假阳性不可达，靠评审兜底。仅扫描 .ts / .tsx / .vue 源文件。
// 默认校验本仓库；测试传位置参数指向夹具：
// bun scripts/check-frontend-structure.ts [repo-root] [packages-manifest.json]
// arg2 = 夹具包登记表（JSON 数组，与 PACKAGES 同形）——仅供测试夹具注入，注入时
// 完全替代生产登记表（#1150 起 PACKAGES 非空，拼接会让生产条目泄漏进夹具）；
// 生产路径不传，登记表唯一事实源仍是本脚本 PACKAGES。
// 挂载于 scripts/check.sh 质量门槛序列与 CI（build.yml frontend job），
// 与结构守门检查并列。

import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

/** 成员包登记条目（单一事实源，issue #1149）：dir 相对仓库根。 */
export interface PackageEntry {
  /** 包名：`@ledger/xxx`（与包 package.json name 全等） */
  name: string
  /** 目录：相对仓库根，`packages/xxx` */
  dir: string
  /** 允许依赖的 @ledger 包名（方向表，随包抽取逐票补充） */
  deps: readonly string[]
  /** 测试支持包（issue #1152 规则⑤）：只许被 devDependencies 消费，且自身
   *  dependencies 必须为空——生产依赖图零测试支持内容；未标记者不受此约束 */
  testSupport?: boolean
  note: string
}

/**
 * 成员包登记册（issue #1149）：与 check-structure.ts 的 CRATES 同为「已验证事实
 * 固化为规格」——packages/* 下的每个成员目录在此恰有一行；每拆一个前端包追加。
 */
export const PACKAGES: readonly PackageEntry[] = [
  {
    name: '@ledger/types',
    dir: 'packages/types',
    deps: [],
    note: '纯类型包（issue #1150）：零依赖叶子，方向表恒空——类型层不依赖任何包；金额展示接缝归 @ledger/money（#1153）',
  },
  {
    name: '@ledger/api',
    dir: 'packages/api',
    deps: ['@ledger/types'],
    note: 'IPC 传输层包（issue #1155）：统一 invoke 封装与 globalBusy 唯一生产消费方归位同包，@ledger 范围内只依赖 @ledger/types；不依赖 composables / stores / components / views',
  },
  {
    name: '@ledger/storage',
    dir: 'packages/storage',
    deps: [],
    note: '存储接缝包（issue #1151）：依赖图唯一真叶子，方向表恒空——localStorage 读写的单一收口，不依赖任何包',
  },
  {
    name: '@ledger/i18n',
    dir: 'packages/i18n',
    deps: ['@ledger/storage'],
    note: '界面语言包（issue #1151 / ADR-0049）：依赖存储底座 @ledger/storage 单向成边，locales 文案资源随包走；money / errors 等上层消费方依赖本包，utils ↔ i18n 双向环消失',
  },
  {
    name: '@ledger/money',
    dir: 'packages/money',
    deps: ['@ledger/types', '@ledger/i18n'],
    note: '金额展示与换算包（issue #1153 / spec #1148）：金额/价格/数量格式化与元/分换算的单一出口，依赖 @ledger/types 与 @ledger/i18n；金额隐私开关的水合与持久化仍归应用设置 store',
  },
  {
    name: '@ledger/test-support',
    dir: 'packages/test-support',
    deps: ['@ledger/types'],
    testSupport: true,
    note: '共享测试支持包（issue #1152）：全局测试接缝（invoke/message/matchMedia/listen/返回桥替身 + 每测清理）唯一宿主，消费只经 devDependency（testSupport 标志 → 规则⑤）；参考数据夹具类型边 @ledger/types 显式放行',
  },
  {
    name: '@ledger/modal-intent',
    dir: 'packages/modal-intent',
    deps: [],
    note: '弹窗意图编排包（issue #1316 / ADR-0072 / ADR-0118）：useModalIntent 通用工厂——模态弹窗「开启 / 目标 / 关闭」编排的唯一形态，工厂零业务语义、零外部依赖（无 store、无 api、无组件，不接弹层注册表 ADR-0035）；方向表恒空，外部依赖仅 vue',
  },
  {
    name: '@ledger/row-context-menu',
    dir: 'packages/row-context-menu',
    deps: [],
    note: '行右键菜单编排包（issue #1317 / ADR-0077 / ADR-0118）：useRowContextMenu 工厂——行菜单打开/重定位/关闭/选中全部时序的单一实现；方向表恒空仅依赖 vue；不接弹层注册表（ADR-0035）与不调 preventDefault 的既有禁令随包保持，菜单选项构建与业务动作分派留视图',
  },
  {
    name: '@ledger/theme',
    dir: 'packages/theme',
    deps: ['@ledger/types', '@ledger/money'],
    note: '主题包（issue #1154 / ADR-0093 / issue #888）：语义色、中性设计令牌、组件库主题覆盖、vanilla-extract 主题合同与图表统一样式的单一来源，Theme 类型随包下移；@ledger 范围内只依赖 @ledger/types 与 @ledger/money，不依赖 stores / components / views / composables',
  },
  {
    name: '@ledger/utils',
    dir: 'packages/utils',
    deps: ['@ledger/types', '@ledger/storage', '@ledger/i18n', '@ledger/money'],
    note: '通用工具包（issue #1314 / ADR-0118 决策 2）：src/utils 叶子层全量平铺搬迁——日期/期间、分类树与图表数据形态、Chart.js 统一注册、码化错误本地化 errorMessage（ADR-0050）、字段错误、视图状态、拼音过滤等纯函数单一来源；方向表与实际 import 全等（types / storage / i18n / money），exports 逐模块子路径暴露不开运行期 barrel；不依赖 stores / components / views / composables',
  },
  {
    name: '@ledger/window-tier',
    dir: 'packages/window-tier',
    deps: ['@ledger/test-support'],
    note: '窗口分级包（issue #1315 / ADR-0088 / ADR-0118 决策 5）：宽度轴唯一事实源——单一断点两档「宽度信号 → 档位」纯映射 composable；断点常量 WINDOW_TIER_BREAKPOINT_PX 全仓唯一收口包内 src/useWindowTier.ts，vite.config.ts 构建期按源码路径提取（收口漂移 fail-loud，构建期契约保留只换坐标），CSS 媒体查询经占位符替换消费同值；生产依赖仅 vue，@ledger 方向表仅测试边 → @ledger/test-support（媒体查询换档接缝，devDependencies 消费，规则⑤）；不依赖 stores / components / views',
  },
  {
    name: '@ledger/loadable',
    dir: 'packages/loadable',
    deps: ['@ledger/utils'],
    note: '异步任务生命周期包（issue #1318 / ADR-0040 / ADR-0118 决策 4/6）：useLoadable 统一异步任务生命周期深模块——loading 置收、错误文案归一、竞态裁决（后发覆盖先发）与 invalidate 作废在途的单一实现，竞态序号唯一合法住址随包（check-async-guards 规则 1 豁免坐标同步为包内路径）；模块级 toast sink 单例随包 ESM 持有、对外只暴露 registerToastSink 注册接口，应用入口 MessageSinkBridge 经导入接线，不引入注入机制（ADR-0118 决策 6）；依赖 @ledger/utils（errorMessage）单向成边；不依赖 stores / components / views',
  },
]

/** workspace 成员 glob（pnpm-workspace.yaml 侧声明与本脚本核对同源）。 */
const MEMBER_DIR_GLOB = 'packages/*'

/** 包名空间前缀（规则③④的跨包引用识别面）。 */
const PACKAGE_NAME_PREFIX = '@ledger/'

/** 本脚本的门槛调用形态（接线核对与测试夹具共用的单一事实源）。 */
export const SCRIPT_INVOCATION = 'bun scripts/check-frontend-structure.ts'

/** 接线宿主（删除即变红①）：行首形态逐字节匹配，非注释行才算接线。 */
const WIRING_HOSTS: readonly { file: string; prefix: string; where: string }[] = [
  { file: 'scripts/check.sh', prefix: SCRIPT_INVOCATION, where: 'scripts/check.sh' },
  {
    file: '.github/workflows/build.yml',
    prefix: `run: ${SCRIPT_INVOCATION}`,
    where: '.github/workflows/build.yml frontend job',
  },
]

/** 规则③④靶形态（逐行扫描，与既有守门同形制）：说明符与 from 同行（多行 import
 *  的尾行即 from 行，行号对准说明符）；动态 import 与副作用 import 单独捕形 */
const FROM_SPECIFIER_PATTERN = /\bfrom\s*['"]([^'"]+)['"]/
const DYNAMIC_IMPORT_PATTERN = /\bimport\s*\(\s*['"]([^'"]+)['"]/
const SIDE_EFFECT_IMPORT_PATTERN = /\bimport\s+['"]([^'"]+)['"]/

/** 扫描的源文件扩展名（Vue SFC 的 script 块与 TS 源码） */
const SOURCE_EXTENSIONS = ['.ts', '.tsx', '.vue']

/** 掩码 TS/Vue 源文本中的注释（行注释与块注释）：内容替换为等长空白（保留换行与
 *  列位），使 import 扫描只落在真实代码上。刻意不掩码字符串/模板字面量——import
 *  说明符本身是字符串，掩码会连靶一起抹掉；字符串内的伪 import 靠关键词上下文
 *  排除，正则字面量内含注释起止符的误掩码只会向绿偏（漏报不误报），评审兜底。 */
export function maskComments(text: string): string {
  const out = text.split('')
  const n = text.length
  const blank = (from: number, to: number): void => {
    for (let k = from; k < to && k < n; k++) if (out[k] !== '\n') out[k] = ' '
  }
  let i = 0
  while (i < n) {
    if (text[i] === '/' && text[i + 1] === '/') {
      const stop = text.indexOf('\n', i) === -1 ? n : text.indexOf('\n', i)
      blank(i, stop)
      i = stop
    } else if (text[i] === '/' && text[i + 1] === '*') {
      const end = text.indexOf('*/', i + 2)
      const stop = end === -1 ? n : end + 2
      blank(i, stop)
      i = stop
    } else {
      i++
    }
  }
  return out.join('')
}

/** 单条 import 说明符命中：行号（1 起算）、原文行、说明符 */
export interface ImportHit {
  line: number
  text: string
  specifier: string
}

/** 扫描单个源文本（逐行、掩码注释后）的 import 说明符：返回行号与说明符清单。
 *  同一行命中多条形态时全部上报（分号连写的多语句均为真实引用）。 */
export function scanImportSpecifiers(text: string): ImportHit[] {
  const masked = maskComments(text)
  const maskedLines = masked.split('\n')
  const rawLines = text.split('\n')
  const hits: ImportHit[] = []
  for (let i = 0; i < maskedLines.length; i++) {
    const line = maskedLines[i] ?? ''
    for (const pattern of [FROM_SPECIFIER_PATTERN, DYNAMIC_IMPORT_PATTERN, SIDE_EFFECT_IMPORT_PATTERN]) {
      const m = line.match(pattern)
      if (!m) continue
      hits.push({ line: i + 1, text: (rawLines[i] ?? '').trim(), specifier: m[1] ?? '' })
    }
  }
  return hits
}

/** 递归收集目录下全部源文件（相对路径排序保证输出确定） */
function collectSourceFiles(dir: string, relBase: string): { abs: string; rel: string }[] {
  const out: { abs: string; rel: string }[] = []
  if (!existsSync(dir)) return out
  for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    const abs = join(dir, entry.name)
    const rel = relBase ? `${relBase}/${entry.name}` : entry.name
    if (entry.isDirectory()) out.push(...collectSourceFiles(abs, rel))
    else if (SOURCE_EXTENSIONS.some((ext) => entry.name.endsWith(ext))) out.push({ abs, rel })
  }
  return out
}

/** 规则④：深导入子路径是否命中目标包 exports 入口（精确键或 `./*` 通配） */
function exportsExposeSubpath(exportsField: unknown, subpath: string): boolean {
  if (typeof exportsField === 'string') return false // 字符串形态仅暴露 "."，不含子路径
  if (exportsField === null || typeof exportsField !== 'object') return false
  const keys = Object.keys(exportsField)
  if (keys.includes(`./${subpath}`)) return true
  // 简单通配 "./*"：目录级放行（包抽取逐票补精确键时收窄）
  return keys.includes('./*')
}

/** 夹具登记表装载（arg2）：JSON 数组、与 PackageEntry 同形，畸形 fail loud */
function loadFixtureManifest(path: string): PackageEntry[] {
  let parsed: unknown
  try {
    parsed = JSON.parse(readFileSync(path, 'utf8'))
  } catch (err) {
    console.error(`✗ 夹具登记表不可解析（${path}）：${String(err)}`)
    process.exit(1)
  }
  if (!Array.isArray(parsed)) {
    console.error(`✗ 夹具登记表须为 JSON 数组：${path}`)
    process.exit(1)
  }
  return parsed as PackageEntry[]
}

/** pnpm-workspace.yaml 以 glob 声明成员目录（文本级核对，与 yaml 现状同形态）：
 *  packages: 键与 - packages/* 列表项之间的注释行不参与匹配 */
function workspaceDeclaresPackagesGlob(yamlText: string): boolean {
  return /(?:^|\n)packages:[^\n]*\n(?:\s*#[^\n]*\n)*\s*-\s*['"]?packages\/\*['"]?\s*(?:\n|$)/.test(
    yamlText,
  )
}

/** 成员 package.json 的四类依赖键（方向核对统一口径） */
const DEP_KINDS = [
  'dependencies',
  'devDependencies',
  'optionalDependencies',
  'peerDependencies',
] as const

/** 规则①：磁盘成员目录 ↔ PACKAGES 双向全等（新建不登记即红、清单漂移即红） */
function checkMemberRegistration(
  repoRoot: string,
  registry: readonly PackageEntry[],
  problems: string[],
): void {
  const packagesDir = join(repoRoot, 'packages')
  if (!existsSync(packagesDir)) {
    problems.push(
      `✗ 成员登记：packages/ 目录不存在——workspace 骨架（${MEMBER_DIR_GLOB}）须有成员目录（可为空，issue #1149）`,
    )
    return
  }
  const onDisk = readdirSync(packagesDir, { withFileTypes: true })
    .filter((e) => e.isDirectory())
    .map((e) => `packages/${e.name}`)
    .sort()
  for (const dir of onDisk) {
    if (!registry.some((p) => p.dir === dir)) {
      problems.push(
        `✗ 成员登记：${dir} 未登记 PACKAGES\n` +
          `    新增前端子包后须在 scripts/check-frontend-structure.ts 的 PACKAGES 追加一行` +
          `（包名 + 方向表 deps + 注释），否则边界知识分裂、依赖方向失守（issue #1149 规则①）`,
      )
    }
  }
  for (const pkg of registry) {
    if (!onDisk.includes(pkg.dir)) {
      problems.push(
        `✗ 成员登记：PACKAGES 登记的成员目录不存在：${pkg.dir}（清单漂移 fail loud）`,
      )
    }
  }
}

/** 规则②：包依赖方向——只许依赖已登记且在自身方向表 deps 内的 @ledger 包 */
function checkDependencyDirection(
  repoRoot: string,
  registry: readonly PackageEntry[],
  problems: string[],
): void {
  for (const pkg of registry) {
    const manifestPath = join(repoRoot, pkg.dir, 'package.json')
    if (!existsSync(manifestPath)) {
      problems.push(`✗ 包依赖方向：${pkg.dir}/package.json 不存在（${pkg.name}，${pkg.note}）`)
      continue
    }
    let manifest: {
      name?: unknown
      dependencies?: unknown
      devDependencies?: unknown
      optionalDependencies?: unknown
      peerDependencies?: unknown
      exports?: unknown
    }
    try {
      manifest = JSON.parse(readFileSync(manifestPath, 'utf8'))
    } catch (err) {
      problems.push(`✗ 包依赖方向：${pkg.dir}/package.json 不可解析：${String(err)}`)
      continue
    }
    if (manifest.name !== pkg.name) {
      problems.push(
        `✗ 成员登记：PACKAGES 登记名 ${pkg.name} 与包清单 name ${String(manifest.name)} 不一致（${pkg.dir}）`,
      )
    }
    for (const kind of DEP_KINDS) {
      const deps = manifest[kind]
      if (deps === null || typeof deps !== 'object') continue
      for (const depName of Object.keys(deps as Record<string, unknown>)) {
        if (!depName.startsWith(PACKAGE_NAME_PREFIX)) continue
        const target = registry.find((p) => p.name === depName)
        if (!target) {
          problems.push(
            `✗ 包依赖方向：${pkg.name}（${pkg.dir}）${kind} 依赖未登记包 ${depName}\n` +
              `    新包须先在 scripts/check-frontend-structure.ts 的 PACKAGES 登记（issue #1149 规则②）`,
          )
          continue
        }
        if (!pkg.deps.includes(depName)) {
          problems.push(
            `✗ 包依赖方向：${pkg.name}（${pkg.dir}）${kind} 依赖 ${depName}，不在其方向表内\n` +
              `    方向表（PACKAGES[${pkg.name}].deps）随包抽取逐票补充；确属合法依赖时在方向表追加该边（issue #1149 规则②）`,
          )
        }
      }
    }
  }
}

/** 规则③④：包内源码的跨包引用形态（@/ 别名、相对穿越、深导入 exports 入口） */
function checkImportShapes(
  repoRoot: string,
  registry: readonly PackageEntry[],
  problems: string[],
): void {
  for (const pkg of registry) {
    const pkgRoot = join(repoRoot, pkg.dir)
    const files = collectSourceFiles(pkgRoot, pkg.dir)
    for (const f of files) {
      const source = readFileSync(f.abs, 'utf8')
      for (const hit of scanImportSpecifiers(source)) {
        const spec = hit.specifier
        if (spec.startsWith('@/')) {
          problems.push(
            `✗ 跨包引用形态：${f.rel}:${hit.line} 使用 @/ 别名（${spec}）\n` +
              `    ${hit.text}\n` +
              `    @/ 指向根 src，从包内使用必然穿越包边界；跨包引用只能走包名 @ledger/*，` +
              `包内引用走相对路径（issue #1149 规则③）`,
          )
          continue
        }
        if (spec.startsWith('.')) {
          const resolved = resolve(dirname(f.abs), spec)
          const relToPkg = relative(pkgRoot, resolved)
          if (relToPkg.startsWith('..') || relToPkg === '') {
            problems.push(
              `✗ 跨包引用形态：${f.rel}:${hit.line} 相对路径穿越包边界（${spec}）\n` +
                `    ${hit.text}\n` +
                `    解析落点 ${relToPkg} 越出 ${pkg.dir}；跨包引用只能走包名 @ledger/*（issue #1149 规则③）`,
            )
          }
          continue
        }
        if (spec.startsWith(PACKAGE_NAME_PREFIX)) {
          const m = /^@ledger\/([^/]+)(?:\/(.*))?$/.exec(spec)
          if (!m) {
            problems.push(
              `✗ 跨包引用形态：${f.rel}:${hit.line} 非法包名形态（${spec}）（issue #1149 规则③）`,
            )
            continue
          }
          const [, targetName, subpath] = m
          const target = registry.find((p) => p.name === `@ledger/${targetName}`)
          if (!target) {
            problems.push(
              `✗ 跨包引用形态：${f.rel}:${hit.line} 引用未登记包 ${spec}\n` +
                `    新包须先在 PACKAGES 登记（issue #1149 规则②/③）`,
            )
            continue
          }
          if (subpath === undefined) continue // 包名整引，规则④不适用
          const targetManifestPath = join(repoRoot, target.dir, 'package.json')
          if (!existsSync(targetManifestPath)) {
            problems.push(
              `✗ 深导入禁令：${f.rel}:${hit.line} 深导入 ${spec}——目标包 package.json 不存在（${target.dir}）`,
            )
            continue
          }
          let targetManifest: { exports?: unknown }
          try {
            targetManifest = JSON.parse(readFileSync(targetManifestPath, 'utf8'))
          } catch (err) {
            problems.push(
              `✗ 深导入禁令：${f.rel}:${hit.line} 目标包清单不可解析：${target.dir}/package.json（${String(err)}）`,
            )
            continue
          }
          if (!exportsExposeSubpath(targetManifest.exports, subpath)) {
            problems.push(
              `✗ 深导入禁令：${f.rel}:${hit.line} 深导入 ${spec} 未命中目标包 exports 入口\n` +
                `    ${hit.text}\n` +
                `    跨包引用必须命中目标包 package.json exports 的对应入口（精确键 ./${subpath} ` +
                `或 ./* 通配）；补导出入口或经包名整引（issue #1149 规则④）`,
            )
          }
        }
      }
    }
  }
}

/** 读清单为 JSON 对象；缺失或不可解析返回 null——对应缺口由规则②报，不在此重复 */
function tryReadManifest(path: string): Record<string, unknown> | null {
  if (!existsSync(path)) return null
  try {
    return JSON.parse(readFileSync(path, 'utf8')) as Record<string, unknown>
  } catch {
    return null
  }
}

/** 规则⑤：testSupport 包只许经 devDependencies 被消费，且自身 dependencies 为空。
 *  登记面 = PACKAGES 内 testSupport: true 的条目（单一事实源，夹具登记表同形）；
 *  核对面 = 根包 + 全部成员包的清单（生产依赖图 = 各包 dependencies 侧的并集）。
 *  对 optionalDependencies/peerDependencies 同样拦截——optional 与 peer 都会把包
 *  带进消费方的安装/解析面，dev 是唯一合法通道。 */
function checkTestSupportPurity(
  repoRoot: string,
  registry: readonly PackageEntry[],
  problems: string[],
): void {
  const testSupportNames = registry.filter((p) => p.testSupport).map((p) => p.name)
  if (testSupportNames.length === 0) return
  const forbiddenKinds = ['dependencies', 'optionalDependencies', 'peerDependencies'] as const
  const manifests: readonly { label: string; path: string }[] = [
    { label: '根包（应用壳）', path: join(repoRoot, 'package.json') },
    ...registry.map((p) => ({ label: p.name, path: join(repoRoot, p.dir, 'package.json') })),
  ]
  for (const manifest of manifests) {
    const parsed = tryReadManifest(manifest.path)
    if (!parsed) continue
    for (const kind of forbiddenKinds) {
      const deps = parsed[kind]
      if (deps === null || typeof deps !== 'object') continue
      for (const testSupport of testSupportNames) {
        if (!(testSupport in (deps as Record<string, unknown>))) continue
        problems.push(
          `✗ 测试支持纯净性：${manifest.label} 的 ${kind} 出现 ${testSupport}\n` +
            `    测试支持只许经 devDependencies 消费（issue #1152 规则⑤）：生产依赖图零测试支持内容；` +
            `把该依赖移入 devDependencies（消费方为测试代码，不影响产物构建）`,
        )
      }
    }
  }
  for (const pkg of registry) {
    if (!pkg.testSupport) continue
    const parsed = tryReadManifest(join(repoRoot, pkg.dir, 'package.json'))
    if (!parsed) continue
    const prodDeps = parsed.dependencies
    if (prodDeps !== null && typeof prodDeps === 'object' && Object.keys(prodDeps).length > 0) {
      problems.push(
        `✗ 测试支持纯净性：${pkg.name} 自身 dependencies 非空（${Object.keys(prodDeps as Record<string, unknown>).join(' ')}）\n` +
          `    测试支持包零生产依赖（issue #1152 规则⑤）：替身与接缝所需运行面全部走 devDependencies`,
      )
    }
  }
}

/** 规则⑥ 上行引用禁令登记册（单一事实源，issue #1156）：按目录登记「不得引用的
 *  上层目标」。每个条目 = 一段已固化的方向约束；条目本身即规格——删除/改动条目会
 *  让 src/__tests__/check-frontend-structure.test.ts 的登记表全等断言变红
 *  （删除即变红，issue #1156 验收判据）。 */
export interface UpwardImportRule {
  /** 被约束的源目录（相对仓库根，posix 分隔） */
  dir: string
  /** 禁止出现的 import 说明符：精确名或其 `名/子路径` */
  forbidden: readonly string[]
  note: string
}

/** 登记项（逐票补充）：#1314 起 src/utils 成包 @ledger/utils，唯一登记项
 *  （issue #1156 的 utils 四条上行禁令）随搬迁对象消失而收缩，现为空集——
 *  包层上行由规则②③④接管（见规则⑥头注）；新增目录级约束在此登记。 */
export const FORBIDDEN_UPWARD_IMPORTS: readonly UpwardImportRule[] = []

/** 说明符是否命中登记的上行目标（精确名或其 `名/子路径`，避免 `@/storesX` 误伤） */
function hitsForbiddenSpecifier(specifier: string, forbidden: readonly string[]): string | null {
  for (const target of forbidden) {
    if (specifier === target || specifier.startsWith(`${target}/`)) return target
  }
  return null
}

/** 规则⑥：登记目录不得出现登记的上行 import（文本级扫描，复用 import 捕形）。
 *  登记目录缺失即红——拒绝以空集假绿（目录改名/漂移后规则静默失效）。 */
function checkUpwardImports(repoRoot: string, problems: string[]): void {
  for (const rule of FORBIDDEN_UPWARD_IMPORTS) {
    const dir = join(repoRoot, rule.dir)
    if (!existsSync(dir)) {
      problems.push(
        `✗ 上行引用禁令：登记目录不存在：${rule.dir}（${rule.note}）\n` +
          `    目录改名/漂移后规则静默失效，须同步 FORBIDDEN_UPWARD_IMPORTS（issue #1156）`,
      )
      continue
    }
    for (const f of collectSourceFiles(dir, rule.dir)) {
      const source = readFileSync(f.abs, 'utf8')
      for (const hit of scanImportSpecifiers(source)) {
        const target = hitsForbiddenSpecifier(hit.specifier, rule.forbidden)
        if (!target) continue
        problems.push(
          `✗ 上行引用禁令：${f.rel}:${hit.line} 引用 ${hit.specifier}\n` +
            `    ${hit.text}\n` +
            `    ${rule.dir} 为叶子层，不得引用 ${target}；把接缝迁到消费侧或改入参注入` +
            `（issue #1156 规则⑥）`,
        )
      }
    }
  }
}

/** 规则⑦ 深模块边界登记条目（单一事实源，issue #1323 / ADR-0118 决策 7）：路径相对
 *  仓库根，posix 分隔。 */
export interface DeepModuleBoundary {
  /** 深模块文件路径（相对仓库根） */
  module: string
  /** 允许的消费方目录/文件闭集（相对仓库根；目录为前缀闭集） */
  allowedConsumers: readonly string[]
  note: string
}

/** 深模块边界登记册（issue #1323）：不成包的深模块（依赖壳内状态故过不了成包判据
 *  1，ADR-0118 决策 1/4）以白名单固化生产消费面；每新增一项登记追加一行。条目本身
 *  即规格——删除/改动条目会让 scripts/check-frontend-structure.test.ts 的登记表全等
 *  断言变红（删除即变红，issue #1323 验收判据）。 */
export const DEEP_MODULE_BOUNDARIES: readonly DeepModuleBoundary[] = [
  {
    module: 'src/composables/useTransactionFilter.ts',
    allowedConsumers: ['src/views'],
    note: '交易列表过滤深模块（ADR-0030/0094）：依赖壳内 pinia store（交易页会话级 store）故不成包（ADR-0118 决策 4），消费面 = 交易页与报表页',
  },
]

/** 消费方 rel 路径是否命中白名单条目（目录为前缀闭集：`src/views` 放行 `src/views/` 整棵树） */
function consumerAllowed(consumerRel: string, allowed: readonly string[]): boolean {
  return allowed.some((dir) => consumerRel === dir || consumerRel.startsWith(`${dir}/`))
}

/** import 说明符解析为仓库相对路径（`@/` 别名指向 src/；相对路径自消费方文件解析；
 *  包名与 bare 说明符不指向壳内文件，返回 null） */
function resolveShellSpecifier(repoRoot: string, consumerAbs: string, specifier: string): string {
  const abs = specifier.startsWith('@/')
    ? join(repoRoot, 'src', specifier.slice(2))
    : resolve(dirname(consumerAbs), specifier)
  return relative(repoRoot, abs)
}

/** 说明符解析落点是否命中登记模块（精确文件，或 TS 无扩展名 / 编译 .js 形态） */
function hitsDeepModule(candRel: string, moduleRel: string): boolean {
  if (candRel === moduleRel) return true
  const stem = moduleRel.replace(/\.ts$/, '')
  return candRel === stem || candRel === `${stem}.js`
}

/** 测试文件不在规则⑦扫描面：单测引用被测对象是天然形态，白名单表达生产消费面 */
function isTestFile(rel: string): boolean {
  return rel.split('/').includes('__tests__') || /\.(test|spec)\.[tj]sx?$/.test(rel)
}

/** 规则⑦：登记模块的消费方必须全在白名单内（文本级扫描 src/ + packages/ 源码树，
 *  复用 import 捕形与注释掩码）。登记模块文件缺失即红——拒绝以空集假绿（模块
 *  改名/删除后规则静默失效，同规则⑥登记目录缺失形制）。 */
function checkDeepModuleBoundaries(repoRoot: string, problems: string[]): void {
  for (const entry of DEEP_MODULE_BOUNDARIES) {
    const moduleAbs = join(repoRoot, entry.module)
    if (!existsSync(moduleAbs)) {
      problems.push(
        `✗ 深模块边界：登记模块不存在：${entry.module}（${entry.note}）\n` +
          `    模块改名/删除后规则静默失效，须同步 DEEP_MODULE_BOUNDARIES（issue #1323 规则⑦）`,
      )
      continue
    }
    for (const tree of ['src', 'packages']) {
      for (const f of collectSourceFiles(join(repoRoot, tree), tree)) {
        if (isTestFile(f.rel)) continue
        const source = readFileSync(f.abs, 'utf8')
        for (const hit of scanImportSpecifiers(source)) {
          if (!hit.specifier.startsWith('@/') && !hit.specifier.startsWith('.')) continue
          const candRel = resolveShellSpecifier(repoRoot, f.abs, hit.specifier)
          if (!hitsDeepModule(candRel, entry.module)) continue
          if (consumerAllowed(f.rel, entry.allowedConsumers)) continue
          problems.push(
            `✗ 深模块边界：${f.rel}:${hit.line} 消费 ${entry.module}\n` +
              `    ${hit.text}\n` +
              `    ${entry.module} 为深模块，消费方限于白名单（${entry.allowedConsumers.join(' ')}）；` +
              `新消费方先评估扩白名单或改接缝（issue #1323 规则⑦ / ADR-0118 决策 7）`,
          )
        }
      }
    }
  }
}

/** 接线核对（删除即变红①）：两个宿主文件须有非注释的实际调用行 */
function checkWiring(repoRoot: string, problems: string[]): void {
  for (const host of WIRING_HOSTS) {
    const abs = join(repoRoot, host.file)
    if (!existsSync(abs)) {
      problems.push(`✗ 接线核对：宿主文件不存在：${host.where}`)
      continue
    }
    const wired = readFileSync(abs, 'utf8')
      .split('\n')
      .some((line) => {
        const trimmed = line.trim()
        return !trimmed.startsWith('#') && trimmed.startsWith(host.prefix)
      })
    if (!wired) {
      problems.push(
        `✗ 接线核对：${host.where} 缺前端结构守门调用行\n` +
          `    须有 \`${host.prefix}\` 开头的实际执行行（非注释、非 echo 展示行）——` +
          `删除接线即门槛静默消失（issue #1149 删除即变红①）`,
      )
    }
  }
}

function main(): void {
  const scriptDir = dirname(fileURLToPath(import.meta.url))
  const repoRoot = process.argv[2] ?? join(scriptDir, '..')
  const fixtureManifestPath = process.argv[3]
  // 夹具注入完全替代生产登记表（issue #1150 起 PACKAGES 非空）：concat 会让生产
  // 条目泄漏进夹具仓库根、触发「清单漂移」假红；夹具自足才可隔离校验。
  const registry: readonly PackageEntry[] = fixtureManifestPath
    ? loadFixtureManifest(fixtureManifestPath)
    : PACKAGES

  const problems: string[] = []

  // 规则①前置：pnpm-workspace.yaml 须以 glob 声明 packages/*
  const workspaceYamlPath = join(repoRoot, 'pnpm-workspace.yaml')
  if (!existsSync(workspaceYamlPath)) {
    problems.push(`✗ workspace 骨架：pnpm-workspace.yaml 不存在（${repoRoot}）`)
  } else if (!workspaceDeclaresPackagesGlob(readFileSync(workspaceYamlPath, 'utf8'))) {
    problems.push(
      `✗ workspace 骨架：pnpm-workspace.yaml 未以 glob 声明 ${MEMBER_DIR_GLOB}\n` +
        `    packages: 列表须含 - packages/*（成员可为空，新增子包自动入 workspace，issue #1149）`,
    )
  }

  checkMemberRegistration(repoRoot, registry, problems)
  checkDependencyDirection(repoRoot, registry, problems)
  checkImportShapes(repoRoot, registry, problems)
  checkTestSupportPurity(repoRoot, registry, problems)
  checkUpwardImports(repoRoot, problems)
  checkDeepModuleBoundaries(repoRoot, problems)
  checkWiring(repoRoot, problems)

  if (problems.length > 0) {
    for (const p of problems) console.error(p)
    console.error(`❌ 前端结构守门失败：${problems.length} 处问题（issue #1149）`)
    process.exit(1)
  }
  console.log(
    `✓ 前端结构守门：pnpm-workspace.yaml 声明 ${MEMBER_DIR_GLOB}` +
      `· 成员登记 ${registry.length} 个（磁盘 ↔ PACKAGES 双向全等）` +
      `· 包依赖方向 ${registry.length} 包（方向表逐票补充）` +
      `· 跨包引用形态与深导入禁令扫描 ${collectSourceFiles(join(repoRoot, 'packages'), 'packages').length} 个文件` +
      `· 测试支持纯净性（${registry.filter((p) => p.testSupport).map((p) => p.name).join(' ') || '无'} 仅 devDependency 消费）` +
      `· 上行引用禁令 ${FORBIDDEN_UPWARD_IMPORTS.length} 条（${FORBIDDEN_UPWARD_IMPORTS.map((r) => r.dir).join(' ') || '无'}）` +
      `· 深模块边界 ${DEEP_MODULE_BOUNDARIES.length} 项（${DEEP_MODULE_BOUNDARIES.map((e) => e.module).join(' ') || '无'}）` +
      `· 接线核对（scripts/check.sh + CI frontend job）`,
  )
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的扫描函数与清单。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
