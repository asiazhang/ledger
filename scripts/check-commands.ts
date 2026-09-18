#!/usr/bin/env bun
// 命令注册一致性校验（issue #315 / ADR-0047 命令名腿；issue #1398 参数键名腿）：
// 命令单一来源 = `#[tauri::command]` 注解本身。命令名腿：左集 = Rust 注解命令名
// （与 src-tauri/build.rs 扫描器同源同界）；右集 = packages/api/src/index.ts 的
// invoke('命令名') / invoke("命令名") 字符串（单双引号均识别；模板字面量维持不
// 支持，既有文档化取舍，评审兜底），双向全等。TS 调用面先经注释掩码再匹配（复用
// TS 侧注释掩码单源模块 ts-comment-mask.ts 的 maskComments，#1471 / #1481 上收
// 共享模块）：注释掉的 invoke 形态不算真实调用面，不再报「Rust 无此命令」假红。参数键名腿（#1398，
// #588 类回归根治）：Rust
// 数据参数名经 lowerCamelCase 转换（转换规则与 tauri-macros 的 heck
// ToLowerCamelCase 参数绑定一致）后，与 invoke 实参对象的字面量键每命令每调用点
// 双向全等，多处调用点键集须互相一致；State/AppHandle/Window/WebviewWindow 注入
// 参数不进键集（闭集见 isInjectedType）。任一方向孤儿即非零退出并列出差异。
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-commands.ts`。
// 默认校验本仓库；测试可传位置参数指向夹具：bun scripts/check-commands.ts [commands-dir] [api-file]
// 挂载于 scripts/check.sh 质量门槛序列与 CI（build.yml frontend job）。

import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'
import { maskComments } from './ts-comment-mask.ts'

/** 单条扫描边界违规：行号 1 起算 + 违规行原文 */
export interface ScanError {
  line: number
  text: string
}

/** Rust 命令参数：原名（snake_case）+ 是否 Tauri 注入参数（注入参数不进 invoke 键集） */
export interface RustParam {
  name: string
  injected: boolean
}

/** 单条命令签名：命令名 + 参数列表 */
export interface RustCommand {
  name: string
  params: RustParam[]
}

/** scanRustSource 返回：命令名集 + 命令签名 + 扫描边界违规 */
export interface ScanResult {
  names: string[]
  commands: RustCommand[]
  errors: ScanError[]
}

/** 去掉行注释（签名收集用；签名内不出现字符串字面量，朴素剥离足够） */
function stripLineComment(line: string): string {
  const idx = line.indexOf('//')
  return idx === -1 ? line : line.slice(0, idx)
}

/**
 * 从签名文本提取参数列表：首个 `(` 起到配对 `)` 止。
 * 未平衡返回 null（调用方继续累积后续行）；泛型 `<R: Runtime>` 与返回类型不参与。
 */
function extractParamList(text: string): string | null {
  const start = text.indexOf('(')
  if (start === -1) return null
  let depth = 0
  for (let i = start; i < text.length; i++) {
    if (text[i] === '(') depth++
    else if (text[i] === ')') {
      depth--
      if (depth === 0) return text.slice(start + 1, i)
    }
  }
  return null
}

/**
 * 参数文本按顶层逗号切分：<> / () / [] 深度内的逗号不是分隔符
 * （如 State<'_, DbState>、Fn(A, B)）；`->` 的 > 不参与深度统计。
 */
function splitTopLevel(text: string): string[] {
  const pieces: string[] = []
  let current = ''
  let angle = 0
  let paren = 0
  let bracket = 0
  for (let i = 0; i < text.length; i++) {
    const c = text[i]
    if (c === '-' && text[i + 1] === '>') {
      current += '->'
      i++
      continue
    }
    if (c === '<') angle++
    else if (c === '>') angle = Math.max(0, angle - 1)
    else if (c === '(') paren++
    else if (c === ')') paren--
    else if (c === '[') bracket++
    else if (c === ']') bracket--
    else if (c === ',' && angle === 0 && paren === 0 && bracket === 0) {
      pieces.push(current)
      current = ''
      continue
    }
    current += c
  }
  pieces.push(current)
  return pieces.map((p) => p.trim()).filter((p) => p.length > 0)
}

/**
 * Tauri 注入参数闭集（issue #1398）：这些类型由 Tauri 运行时注入，不进 invoke
 * 实参键集。闭集按 2026-09 全量盘点 src-tauri/src/commands 收口：State<'_, …>
 * （88 处 + tauri:: 路径形态 2 处）、AppHandle（裸形态 + <R: Runtime> 泛型形态 +
 * tauri:: 路径形态，共 88 处）；Window/WebviewWindow 为防御性登记（现网未出现）。
 * 闭集外类型一律按数据参数进键集——未来若出现新注入形态，门禁以键名失配变红
 * 暴露，不会静默假绿（届时在此扩闭集）。
 */
const INJECTED_TYPES = new Set(['State', 'AppHandle', 'Window', 'WebviewWindow'])

function isInjectedType(typeText: string): boolean {
  const bare = typeText
    .replace(/\s+/g, ' ')
    .trim()
    .replace(/^(?:&\s*mut\s+|&|mut\s+)+/, '')
    .replace(/^(?:tauri::|crate::|self::)+/, '')
  const m = bare.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*(?:<.*)?$/s)
  return m !== null && INJECTED_TYPES.has(m[1])
}

/**
 * 解析参数列表文本：只认「参数名: 类型」形态（兼容 mut / r# 前缀，与 tauri 宏
 * 支持的 Pat::Ident 对应）；元组解构等形态不在闭集内，fail loud。
 */
function parseParams(text: string): { params: RustParam[]; errors: string[] } {
  const params: RustParam[] = []
  const errors: string[] = []
  for (const piece of splitTopLevel(text)) {
    const m = piece.match(/^(?:mut\s+)?(?:r#)?([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(.+)$/s)
    if (!m) {
      errors.push(`参数「${piece}」不是「参数名: 类型」形态，拒绝猜测`)
      continue
    }
    params.push({ name: m[1], injected: isInjectedType(m[2]) })
  }
  return { params, errors }
}

/**
 * snake_case → lowerCamelCase，转换规则与 Tauri 参数绑定一致：tauri-macros 对参数名
 * 做 heck ToLowerCamelCase——按非字母数字切词、首词全小写、后续词首字母大写、
 * 词内小写化，数字不构成词边界（top_n → topN，s3_bucket → s3Bucket）。
 * 事故对照（#588）：Rust `top_n: Option<i64>` 期望键 `topN`，snake_case 键被静默
 * 绑定为 None。
 */
export function toLowerCamelCase(name: string): string {
  const words = name.split(/[^A-Za-z0-9]+/).filter((w) => w.length > 0)
  if (words.length === 0) return ''
  return words
    .map((w, i) => (i === 0 ? w.toLowerCase() : w[0].toUpperCase() + w.slice(1).toLowerCase()))
    .join('')
}

/**
 * 扫描单个 Rust 源文本：裸 `#[tauri::command]` + 紧随 `pub fn` / `pub async fn`。
 * 扫描规则与 src-tauri/build.rs 同源同界：注解行必须紧随 fn 定义行，出现其他形态
 * （带参注解、cfg 条件、注解与 fn 之间插入属性行）即记入 errors——fail loud，
 * 未来扩展扫描规则时须同步改 build.rs 与本脚本（维护边界，见 ADR-0047）。
 * 参数提取（#1398）只读签名：fn 匹配后从首个 `(` 累积到参数列表括号平衡，
 * 不改变上述 fn 形态识别规则。
 */
export function scanRustSource(text: string): ScanResult {
  const names: string[] = []
  const commands: RustCommand[] = []
  const errors: ScanError[] = []
  const lines = text.split('\n')
  interface Pending {
    name: string
    line: number
    buffer: string
  }
  const complete = (p: Pending): boolean => {
    const inner = extractParamList(p.buffer)
    if (inner === null) return false
    const { params, errors: paramErrors } = parseParams(inner)
    for (const e of paramErrors) errors.push({ line: p.line, text: `命令 ${p.name}：${e}` })
    names.push(p.name)
    commands.push({ name: p.name, params })
    return true
  }
  let armed = false
  let pending: Pending | null = null
  for (let i = 0; i < lines.length; i++) {
    const trimmed = stripLineComment(lines[i]).trim()
    if (pending) {
      pending.buffer += ' ' + trimmed
      if (complete(pending)) pending = null
      continue
    }
    if (armed) {
      const m = trimmed.match(/^pub (?:async )?fn ([A-Za-z0-9_]+)/)
      if (m) {
        pending = { name: m[1], line: i + 1, buffer: trimmed.slice(m[0].length) }
        if (complete(pending)) pending = null
      } else {
        errors.push({ line: i + 1, text: trimmed })
      }
      armed = false
    } else if (trimmed === '#[tauri::command]') {
      armed = true
    }
  }
  if (armed) {
    errors.push({ line: lines.length, text: '（文件以注解结尾，其后无 fn 定义）' })
  }
  if (pending) {
    errors.push({
      line: lines.length,
      text: `命令 ${pending.name} 参数列表括号不闭合（文件提前结束）`,
    })
  }
  return { names, commands, errors }
}

/**
 * invoke 调用形态（命令名 = 第 2 捕获组）：invoke('命令名') / invoke("命令名")，
 * 含 invoke<T>('命令名') 泛型形态；单双引号须配对（反引用回指）。模板字面量维持
 * 不支持（既有文档化取舍，评审兜底）。
 */
const TS_INVOKE_PATTERN = /\binvoke(?:<[^>]*>)?\(\s*(['"])([^'"]+)\1/g

/** 在（已掩码注释的）文本上匹配全部 invoke 调用点，命令行号/实参由调用方按列位取原文 */
function matchInvokeCalls(masked: string): RegExpExecArray[] {
  return [...masked.matchAll(TS_INVOKE_PATTERN)]
}

/**
 * 扫描 TS 调用面文本中的 invoke 命令名。识别前先掩码注释（复用 TS 侧掩码器具，
 * 与参数键名腿同口径，#1471）——注释掉的调用不计入真实调用面。
 */
export function scanTsSource(text: string): string[] {
  return matchInvokeCalls(maskComments(text)).map((m) => m[2])
}

/** 单个 invoke 调用点：命令名 + 实参对象字面量键 + 起始行（1 起算）+ 解析失败原因 */
export interface TsInvokeCall {
  command: string
  line: number
  keys: string[]
  problems: string[]
}

/** 跳过空白与注释，返回首个有效字符下标 */
function skipTrivia(text: string, start: number): number {
  let i = start
  while (i < text.length) {
    if (/\s/.test(text[i])) {
      i++
    } else if (text[i] === '/' && text[i + 1] === '/') {
      const nl = text.indexOf('\n', i)
      if (nl === -1) return text.length
      i = nl + 1
    } else if (text[i] === '/' && text[i + 1] === '*') {
      const end = text.indexOf('*/', i + 2)
      i = end === -1 ? text.length : end + 2
    } else {
      return i
    }
  }
  return i
}

/** 跳过字符串字面量，返回结束引号后的下标；未闭合 / 模板插值返回 -1 */
function skipString(text: string, start: number): number {
  const quote = text[start]
  let i = start + 1
  while (i < text.length) {
    if (text[i] === '\\') {
      i += 2
      continue
    }
    if (text[i] === quote) return i + 1
    if (quote === '`' && text[i] === '$' && text[i + 1] === '{') return -1
    i++
  }
  return -1
}

/** 解析 invoke 命令名之后的实参部分：`)` 结束（无参形态）或对象字面量键集 */
function parseArgsObject(text: string, start: number): { keys: string[]; problems: string[] } {
  const keys: string[] = []
  const problems: string[] = []
  let i = skipTrivia(text, start)
  if (i >= text.length) {
    return { keys, problems: ['invoke 实参截断（文件提前结束），拒绝猜测'] }
  }
  if (text[i] === ')') return { keys, problems } // 无参命令单参调用形态，键集为空
  if (text[i] === ',') i = skipTrivia(text, i + 1)
  if (i >= text.length || text[i] !== '{') {
    return { keys, problems: ['实参不是对象字面量，无法静态核对参数键名'] }
  }
  // 扫描对象字面量顶层内容：字符串 / 注释跳过，嵌套深度内的逗号不分隔
  const entries: string[] = []
  let current = ''
  let depth = 0
  i++ // 吃掉 '{'
  while (i < text.length) {
    const c = text[i]
    if (c === "'" || c === '"' || c === '`') {
      const end = skipString(text, i)
      if (end === -1) {
        problems.push('实参含未闭合字符串或模板插值，无法静态核对参数键名')
        return { keys, problems }
      }
      current += text.slice(i, end)
      i = end
      continue
    }
    if (c === '/' && text[i + 1] === '/') {
      const nl = text.indexOf('\n', i)
      i = nl === -1 ? text.length : nl
      continue
    }
    if (c === '/' && text[i + 1] === '*') {
      const end = text.indexOf('*/', i + 2)
      i = end === -1 ? text.length : end + 2
      continue
    }
    if (c === '{' || c === '(' || c === '[') {
      depth++
      current += c
      i++
      continue
    }
    if (c === '}' || c === ')' || c === ']') {
      if (depth === 0) {
        if (c === '}') {
          entries.push(current)
          return { keys: finishEntries(entries, problems), problems }
        }
        problems.push(`实参对象以「${c}」意外闭合，无法静态核对参数键名`)
        return { keys, problems }
      }
      depth--
      current += c
      i++
      continue
    }
    if (c === ',' && depth === 0) {
      entries.push(current)
      current = ''
      i++
      continue
    }
    current += c
    i++
  }
  problems.push('实参对象未闭合，无法静态核对参数键名')
  return { keys, problems }
}

/** 实参条目 → 键名：显式 `key:` 或属性简写 `key`；展开 / 计算键等形态 fail loud */
function finishEntries(entries: string[], problems: string[]): string[] {
  const keys: string[] = []
  for (const raw of entries) {
    const t = raw.trim()
    if (t === '') continue
    if (t.startsWith('...')) {
      problems.push(`实参含展开语法「${t}」，无法静态核对参数键名`)
      continue
    }
    if (t.startsWith('[')) {
      problems.push(`实参含计算键「${t}」，无法静态核对参数键名`)
      continue
    }
    const m = t.match(/^([A-Za-z_$][A-Za-z0-9_$]*)\s*:/)
    if (m) {
      keys.push(m[1])
      continue
    }
    if (/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(t)) {
      keys.push(t) // 属性简写 { topN }
      continue
    }
    problems.push(`实参键无法解析「${t}」，拒绝猜测`)
  }
  return keys
}

/**
 * 扫描全部 invoke 调用点（与 scanTsSource 同一匹配边界）：命令形态在掩码文本上
 * 匹配（注释里的调用不算真实调用点，#1471），实参解析读原文——掩码等长保位，对象
 * 里的字符串/注释各由既有解析器跳过。
 */
export function scanTsInvokeCalls(text: string): TsInvokeCall[] {
  const calls: TsInvokeCall[] = []
  for (const m of matchInvokeCalls(maskComments(text))) {
    const line = text.slice(0, m.index).split('\n').length
    const { keys, problems } = parseArgsObject(text, m.index + m[0].length)
    calls.push({ command: m[2], line, keys, problems })
  }
  return calls
}

/** 递归收集目录下全部 .rs 文件（按路径排序，保证输出确定） */
function collectRustFiles(dir: string): string[] {
  const out: string[] = []
  for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    const p = join(dir, entry.name)
    if (entry.isDirectory()) out.push(...collectRustFiles(p))
    else if (entry.name.endsWith('.rs')) out.push(p)
  }
  return out
}

function main(): void {
  const repoRoot = fileURLToPath(new URL('..', import.meta.url))
  const commandsDir = process.argv[2] ?? join(repoRoot, 'src-tauri', 'src', 'commands')
  const apiFile = process.argv[3] ?? join(repoRoot, 'packages', 'api', 'src', 'index.ts')

  const problems: string[] = []
  const rustByName = new Map<string, string>() // 命令名 → 定义文件（重复定义保留首个并报错）
  const rustKeys = new Map<string, string[]>() // 命令名 → 期望键集（数据参数名 lowerCamelCase 后）
  let rustParamTotal = 0 // 含注入参数的总参数数：参数提取是否真的跑过（空集哨兵用）
  let expectedKeyTotal = 0
  for (const file of collectRustFiles(commandsDir)) {
    const { commands, errors } = scanRustSource(readFileSync(file, 'utf8'))
    for (const e of errors) {
      problems.push(
        `✗ 扫描器不认识的命令形态（${file}:${e.line}）：${e.text}\n` +
          `  扫描边界：只认裸 #[tauri::command] + 紧随 pub fn / pub async fn；` +
          `带参注解 / cfg 条件命令需同步扩展 src-tauri/build.rs 与本脚本的扫描规则（ADR-0047）`,
      )
    }
    for (const cmd of commands) {
      if (rustByName.has(cmd.name)) {
        problems.push(`✗ 命令名重复定义：${cmd.name}（${rustByName.get(cmd.name)} 与 ${file}）`)
      } else {
        rustByName.set(cmd.name, file)
      }
      const keys = cmd.params
        .filter((p) => !p.injected)
        .map((p) => toLowerCamelCase(p.name))
      if (!rustKeys.has(cmd.name)) rustKeys.set(cmd.name, keys)
      rustParamTotal += cmd.params.length
      expectedKeyTotal += keys.length
    }
  }

  const tsText = readFileSync(apiFile, 'utf8')
  const tsSet = new Set(scanTsSource(tsText))
  const tsCalls = scanTsInvokeCalls(tsText)

  // 空集 fail loud（与 build.rs 空集 panic 同界）：任一侧扫不出命令都是灾难性信号
  // （目录指错 / 扫描器失灵），不应以「0 ↔ 0 双向全等」假绿退出。参数侧同界：
  // 命令扫到了却扫不出任何参数，是参数提取失灵的信号（#1398）。
  if (rustByName.size === 0) {
    problems.push('✗ 未在命令目录扫描到任何 #[tauri::command]——目录为空或路径指错，拒绝以空集假绿通过')
  }
  if (tsSet.size === 0) {
    problems.push('✗ 未在 TS 调用面扫描到任何 invoke 调用——文件为空或路径指错，拒绝以空集假绿通过')
  }
  if (rustByName.size > 0 && rustParamTotal === 0) {
    problems.push('✗ 命令已扫到但未扫出任何参数——参数提取失灵信号，拒绝以空键集假绿通过')
  }

  const missingInTs = [...rustByName.keys()].filter((n) => !tsSet.has(n)).sort()
  const missingInRust = [...tsSet].filter((n) => !rustByName.has(n)).sort()
  if (missingInTs.length > 0) {
    problems.push(
      `✗ 仅在 Rust 注解侧（TS 调用面缺方法，packages/api/src/index.ts 补 invoke 方法）：\n` +
        missingInTs.map((n) => `  - ${n}`).join('\n'),
    )
  }
  if (missingInRust.length > 0) {
    problems.push(
      `✗ 仅在 TS 调用面（Rust 无此命令，删除调用或补后端命令）：\n` +
        missingInRust.map((n) => `  - ${n}`).join('\n'),
    )
  }

  // 参数键名腿（issue #1398）：每命令每调用点，期望键集（Rust 数据参数名
  // lowerCamelCase 后）↔ 实参字面量键双向全等；多处调用点键集须互相一致。
  const callsByCommand = new Map<string, TsInvokeCall[]>()
  for (const call of tsCalls) {
    if (!callsByCommand.has(call.command)) callsByCommand.set(call.command, [])
    callsByCommand.get(call.command)!.push(call)
    if (!rustByName.has(call.command) || !tsSet.has(call.command)) continue // 命令名腿已报，键名无从对起
    for (const p of call.problems) {
      problems.push(`✗ invoke('${call.command}')（api 第 ${call.line} 行）：${p}`)
    }
    const expected = rustKeys.get(call.command) ?? []
    const missing = expected.filter((k) => !call.keys.includes(k))
    const unknown = call.keys.filter((k) => !expected.includes(k))
    if (missing.length > 0) {
      problems.push(
        `✗ 参数键名失配：${call.command}（api 第 ${call.line} 行）实参缺键：` +
          `${missing.join(', ')}（Rust 参数键：${expected.join(', ') || '（无）'}；` +
          `Tauri 按参数名 lowerCamelCase 绑定，缺键绑定 None，#588 类回归）`,
      )
    }
    if (unknown.length > 0) {
      problems.push(
        `✗ 参数键名失配：${call.command}（api 第 ${call.line} 行）实参多键：` +
          `${unknown.join(', ')}（Rust 参数键：${expected.join(', ') || '（无）'}；` +
          `Tauri 静默丢弃未知键，snake_case 键静默绑 None，#588 类回归）`,
      )
    }
  }
  for (const [command, calls] of callsByCommand) {
    if (calls.some((c) => c.problems.length > 0)) continue // 解析失败的调用点已单独报
    if (!rustByName.has(command) || !tsSet.has(command)) continue
    const signatures = [...new Set(calls.map((c) => [...c.keys].sort().join(', ')))]
    if (signatures.length > 1) {
      problems.push(
        `✗ 参数键名不一致：${command} 的 ${calls.length} 处调用点键集不同（` +
          signatures.map((s) => `{ ${s} }`).join(' vs ') +
          `），多处调用点键集必须互相一致`,
      )
    }
  }

  if (problems.length > 0) {
    for (const p of problems) console.error(`命令注册一致性：${p}`)
    console.error(
      `❌ 命令注册一致性校验失败：${problems.length} 处问题` +
        `（命令单一来源 = #[tauri::command] 注解；命令名与参数键名两侧均须双向全等，` +
        `见 ADR-0047 与 issue #1398）`,
    )
    process.exit(1)
  }
  console.log(
    `✓ 命令注册一致性：Rust 注解 ${rustByName.size} ↔ TS 调用面 ${tsSet.size}，双向全等；` +
      `参数键名：${tsCalls.length} 处 invoke 调用点 ↔ 期望键 ${expectedKeyTotal} 个，双向全等`,
  )
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的扫描函数。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
