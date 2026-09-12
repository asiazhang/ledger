#!/usr/bin/env bun
// i18n 文案 key 全等校验（issue #342 / ADR-0049）：源语言 zh-CN 与其余各 locale
// 的 key 集合双向全等——任一方向孤儿（源语言独有 / 其他 locale 独有）即非零退出，
// 漏翻在合入前被拦截。仿命令集双向全等校验先例（scripts/check-commands.ts）：
// 纯函数导出供单测（scripts/check-i18n-keys.test.ts），CLI 入口可独立运行。
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-i18n-keys.ts`。
// 默认校验本仓库（locales 文案资源随 @ledger/i18n 包走，issue #1151）；测试可传位置
// 参数指向夹具目录：bun scripts/check-i18n-keys.ts [locales-dir] [rust-src-dir]
// 挂载于 scripts/check.sh 质量门槛序列与 CI。
//
// 码化错误模板覆盖守门（issue #1188 / ADR-0050）：在 key 全等之外，再枚举 Rust
// 生产代码中全部码化错误构造点（AppError::coded / codedp / coded_not_found /
// codedp_not_found 的第一参数），要求每个码在 zh-CN 与 en-US 的 errors.json 中
// 都有模板——「新增错误条件 = 后端码化构造器 + errors.json 两份补翻译，漏翻在
// CI 拦截」由此从评审约定变为机器守门。边界（已知且可接受）：
// - 只静态枚举字符串字面量与 `const NAME: &str` 一级常量引用；宏参数
//   （closed_set! 的 err_code）与 format! 拼接形态的动态码无法静态解析，
//   计入 unresolved 计数仅供人审（漏报方向安全：不会误伤存量已覆盖的码）。
// - #[cfg(test)] 块与测试路径（tests 目录 / *tests.rs）不扫：测试内构造不直达用户。
// - 序列化层统一携带的系统通用码（db.error / parse.error / io.error，ADR-0050
//   决策 2「底层驱动消息不翻译也不进码表」）无构造点，天然不入枚举，无需白名单。

import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

/** 源语言目录名（其余 locale 一律与它比对） */
export const SOURCE_LOCALE_DIR = 'zh-CN'

/**
 * 递归展开 JSON 对象为点分 key 全集（叶子 key 才计入；数组整体视为叶子）。
 */
export function flattenKeys(obj: unknown, prefix = ''): string[] {
  const keys: string[] = []
  for (const [k, v] of Object.entries(obj as Record<string, unknown>)) {
    const path = prefix ? `${prefix}.${k}` : k
    if (v !== null && typeof v === 'object' && !Array.isArray(v)) {
      keys.push(...flattenKeys(v, path))
    } else {
      keys.push(path)
    }
  }
  return keys.sort()
}

/** 双向集合差的逐 locale 失败项 */
export interface LocaleFailure {
  locale: string
  sourceOnly: string[]
  otherOnly: string[]
}

/** 双向集合差：sourceOnly = 源语言独有（其他 locale 漏翻），otherOnly = 其他 locale 独有。 */
export function diffKeySets(
  sourceKeys: string[],
  otherKeys: string[],
): { sourceOnly: string[]; otherOnly: string[] } {
  const source = new Set(sourceKeys)
  const other = new Set(otherKeys)
  return {
    sourceOnly: sourceKeys.filter((k) => !other.has(k)),
    otherOnly: otherKeys.filter((k) => !source.has(k)),
  }
}

/** 读取单个 locale 目录：聚合同目录全部 *.json 的点分 key（文件名作为顶层域前缀） */
export function collectLocaleKeys(dir: string): string[] {
  const keys: string[] = []
  for (const entry of readdirSync(dir).sort()) {
    if (!entry.endsWith('.json')) continue
    const domain = entry.slice(0, -'.json'.length)
    const parsed: unknown = JSON.parse(readFileSync(join(dir, entry), 'utf-8'))
    keys.push(...flattenKeys(parsed, domain))
  }
  return keys.sort()
}

// ---------------------------------------------------------------------------
// 码化错误模板覆盖（issue #1188 / ADR-0050）：Rust 码化构造点 → errors.json 模板
// ---------------------------------------------------------------------------

/** 码化错误构造调用形态（第一参数恒为错误码） */
const CODED_CALL_RE = /\b(codedp_not_found|coded_not_found|codedp|coded)\s*\(/g
/** `const NAME: &str = "..."` 一级常量表（跨文件聚合，供常量引用型调用点解析） */
const RUST_STR_CONST_RE = /\bconst\s+([A-Z_][A-Z0-9_]*)\s*:\s*&str\s*=\s*"([^"]*)"/g

/** 把 Rust 源码中的注释替换为空格（保留换行与字符串字面量，含转义）。 */
export function stripRustComments(src: string): string {
  const out: string[] = []
  let i = 0
  const n = src.length
  while (i < n) {
    const c = src[i]
    if (c === '"') {
      // 字符串字面量：整体保留，跳过转义对
      let j = i + 1
      while (j < n) {
        if (src[j] === '\\') {
          j += 2
          continue
        }
        if (src[j] === '"') break
        j += 1
      }
      out.push(src.slice(i, j + 1))
      i = j + 1
      continue
    }
    if (c === '/' && i + 1 < n && src[i + 1] === '/') {
      const j = src.indexOf('\n', i)
      i = j === -1 ? n : j
      continue
    }
    if (c === '/' && i + 1 < n && src[i + 1] === '*') {
      const j = src.indexOf('*/', i + 2)
      out.push(' ')
      i = j === -1 ? n : j + 2
      continue
    }
    out.push(c)
    i += 1
  }
  return out.join('')
}

/** 移除 #[cfg(test)] 修饰的 mod 块（跟踪大括号配对，块可重复出现）。 */
export function stripCfgTestBlocks(src: string): string {
  let out = src
  for (;;) {
    const m = /#\[\s*cfg\s*\(\s*test\s*\)\s*\]/.exec(out)
    if (!m || m.index === undefined) break
    const brace = out.indexOf('{', m.index + m[0].length)
    if (brace === -1) {
      out = out.slice(0, m.index)
      break
    }
    let depth = 0
    let i = brace
    for (; i < out.length; i++) {
      if (out[i] === '{') depth += 1
      else if (out[i] === '}') {
        depth -= 1
        if (depth === 0) break
      }
    }
    out = out.slice(0, m.index) + out.slice(i + 1)
  }
  return out
}

/** 测试路径判定：tests 目录 / *tests.rs（测试内构造不直达用户，不入枚举）。 */
export function isRustTestPath(relPath: string): boolean {
  const parts = relPath.split('/')
  const file = parts[parts.length - 1] ?? ''
  if (parts.slice(0, -1).some((p) => p === 'tests' || p.endsWith('_tests'))) return true
  return file.endsWith('tests.rs') || file.endsWith('_tests.rs')
}

/** collectRustCodedErrorCodes 的返回：去重排序后的码 + 无法静态解析的调用点计数 */
export interface RustCodedCodesResult {
  codes: string[]
  unresolvedCallSites: number
}

function walkRustFiles(root: string): string[] {
  const files: string[] = []
  const visit = (dir: string): void => {
    for (const entry of readdirSync(dir).sort()) {
      const full = join(dir, entry)
      if (statSync(full).isDirectory()) {
        if (entry === 'target') continue
        visit(full)
      } else if (entry.endsWith('.rs')) {
        files.push(full)
      }
    }
  }
  visit(root)
  return files
}

/**
 * 枚举 Rust 生产代码中全部码化错误码（字面量 + `const NAME: &str` 一级引用）。
 * 无法静态解析的动态构造点（宏参数、format! 拼接）计入 unresolvedCallSites，
 * 仅供人审，不作失败条件（漏报方向安全，见文件头边界说明）。
 */
export function collectRustCodedErrorCodes(rustRoot: string): RustCodedCodesResult {
  const constTable = new Map<string, string>()
  const sources: string[] = []
  for (const path of walkRustFiles(rustRoot)) {
    const rel = path.slice(rustRoot.length + 1)
    if (isRustTestPath(rel)) continue
    const src = stripCfgTestBlocks(stripRustComments(readFileSync(path, 'utf-8')))
    sources.push(src)
    for (const m of src.matchAll(RUST_STR_CONST_RE)) {
      const name = m[1]
      const value = m[2]
      if (name !== undefined && value !== undefined && !constTable.has(name)) {
        constTable.set(name, value)
      }
    }
  }
  const codes = new Set<string>()
  let unresolvedCallSites = 0
  for (const src of sources) {
    for (const m of src.matchAll(CODED_CALL_RE)) {
      // 排除构造器定义处（fn coded(...) / pub fn codedp(...)）
      const before = src.slice(Math.max(0, (m.index ?? 0) - 30), m.index)
      if (/\bfn\s+$/.test(before)) continue
      // 抓第一参数：跳过 ( 与空白，期待字符串字面量或常量标识符
      let j = (m.index ?? 0) + m[0].length
      while (j < src.length && (src[j] === ' ' || src[j] === '\t' || src[j] === '\r' || src[j] === '\n')) j += 1
      const ch = src[j]
      if (ch === '"') {
        const end = src.indexOf('"', j + 1)
        if (end !== -1) {
          const code = src.slice(j + 1, end)
          if (code) codes.add(code)
          continue
        }
      }
      const ident = /^[A-Z_][A-Z0-9_]*/.exec(src.slice(j))
      if (ident) {
        const value = constTable.get(ident[0])
        if (value !== undefined) {
          if (value) codes.add(value)
          continue
        }
      }
      unresolvedCallSites += 1
    }
  }
  return { codes: [...codes].sort(), unresolvedCallSites }
}

/** 码化模板缺失差集：枚举码有而 locale 模板没有的键 */
export function diffCodedCoverage(codes: string[], errorTemplateKeys: string[]): string[] {
  const keys = new Set(errorTemplateKeys)
  return codes.filter((c) => !keys.has(c))
}

/** compareLocalesDir 返回：源语言名、参与比对的 locale 清单、失败明细 */
export interface CompareResult {
  sourceLocale: string
  locales: string[]
  failures: LocaleFailure[]
}

/**
 * 比对 locales 目录下源语言与其余 locale 的 key 集合。
 */
export function compareLocalesDir(localesDir: string): CompareResult {
  const dirs = readdirSync(localesDir, { withFileTypes: true })
    .filter((e) => e.isDirectory())
    .map((e) => e.name)
    .sort()
  const sourceKeys = collectLocaleKeys(join(localesDir, SOURCE_LOCALE_DIR))
  const failures: LocaleFailure[] = []
  for (const locale of dirs) {
    if (locale === SOURCE_LOCALE_DIR) continue
    const diff = diffKeySets(sourceKeys, collectLocaleKeys(join(localesDir, locale)))
    if (diff.sourceOnly.length > 0 || diff.otherOnly.length > 0) {
      failures.push({ locale, ...diff })
    }
  }
  return { sourceLocale: SOURCE_LOCALE_DIR, locales: dirs, failures }
}

function main(): void {
  const localesDir = process.argv[2] ?? fileURLToPath(new URL('../packages/i18n/src/locales', import.meta.url))
  const rustRoot = process.argv[3] ?? fileURLToPath(new URL('../src-tauri', import.meta.url))
  const { sourceLocale, locales, failures } = compareLocalesDir(localesDir)
  if (failures.length === 0) {
    console.log(
      `✅ i18n key 全等：${locales.join(' / ')} 各语言 key 集合与源语言 ${sourceLocale} 全等`,
    )
  } else {
    for (const f of failures) {
      console.error(`✗ locale「${f.locale}」与源语言「${sourceLocale}」key 集合不一致：`)
      for (const key of f.sourceOnly) console.error(`  - 缺失：${key}`)
      for (const key of f.otherOnly) console.error(`  - 多余：${key}`)
    }
    process.exit(1)
  }

  // 码化错误模板覆盖守门（issue #1188 / ADR-0050）：每个码化构造码必须在
  // zh-CN 与 en-US 的 errors.json 都有模板（码内点号即 JSON 嵌套路径）。
  const { codes, unresolvedCallSites } = collectRustCodedErrorCodes(rustRoot)
  const zhErrors = flattenKeys(
    JSON.parse(readFileSync(join(localesDir, 'zh-CN', 'errors.json'), 'utf-8')) as unknown,
  )
  const enErrors = flattenKeys(
    JSON.parse(readFileSync(join(localesDir, 'en-US', 'errors.json'), 'utf-8')) as unknown,
  )
  const missing = [...diffCodedCoverage(codes, zhErrors), ...diffCodedCoverage(codes, enErrors)]
  if (missing.length > 0) {
    console.error(
      `✗ 码化错误模板缺失（ADR-0050：新增错误条件 = 码化构造器 + errors.json 两份补翻译）：`,
    )
    for (const code of new Set(missing)) console.error(`  - 缺模板：${code}`)
    process.exit(1)
  }
  const unresolvedNote =
    unresolvedCallSites > 0 ? `（另有 ${unresolvedCallSites} 处动态码构造点无法静态枚举，漏报方向安全）` : ''
  console.log(`✅ 码化错误模板覆盖：${codes.length} 个码化错误码在 zh-CN/en-US errors.json 均有模板${unresolvedNote}`)
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的比对函数
// （与 scripts/check-commands.ts 同一惯法）。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
