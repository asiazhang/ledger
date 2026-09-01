#!/usr/bin/env node
// 命令注册一致性校验（issue #315 / ADR-0047）：命令单一来源 = `#[tauri::command]` 注解本身。
// 左集 = Rust 注解命令名（与 src-tauri/build.rs 扫描器同源同界）；右集 = src/api/index.ts
// 的 invoke('命令名') 字符串。双向全等，任一方向孤儿即非零退出并列出差异。
// 默认校验本仓库；测试可传位置参数指向夹具：node scripts/check-commands.js [commands-dir] [api-file]
// 挂载于 scripts/check.sh 质量门槛序列与 CI（build.yml frontend job）。

import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

/**
 * 扫描单个 Rust 源文本：裸 `#[tauri::command]` + 紧随 `pub fn` / `pub async fn`。
 * 扫描规则与 src-tauri/build.rs 同源同界：注解行必须紧随 fn 定义行，出现其他形态
 * （带参注解、cfg 条件、注解与 fn 之间插入属性行）即记入 errors——fail loud，
 * 未来扩展扫描规则时须同步改 build.rs 与本脚本（维护边界，见 ADR-0047）。
 * @returns {{ names: string[], errors: Array<{line: number, text: string}> }} 行号 1 起算
 */
export function scanRustSource(text) {
  const names = []
  const errors = []
  let armed = false
  const lines = text.split('\n')
  for (let i = 0; i < lines.length; i++) {
    const trimmed = lines[i].trim()
    if (armed) {
      const m = trimmed.match(/^pub (?:async )?fn ([A-Za-z0-9_]+)/)
      if (m) {
        names.push(m[1])
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
  return { names, errors }
}

/**
 * 扫描 TS 调用面文本中的 invoke('命令名')（含 invoke<T>('命令名') 泛型形态）。
 * 只认单引号字符串字面量（api/index.ts 统一风格）。
 */
export function scanTsSource(text) {
  return [...text.matchAll(/\binvoke(?:<[^>]*>)?\(\s*'([^']+)'/g)].map((m) => m[1])
}

/** 递归收集目录下全部 .rs 文件（按路径排序，保证输出确定） */
function collectRustFiles(dir) {
  const out = []
  for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    const p = join(dir, entry.name)
    if (entry.isDirectory()) out.push(...collectRustFiles(p))
    else if (entry.name.endsWith('.rs')) out.push(p)
  }
  return out
}

function main() {
  const repoRoot = fileURLToPath(new URL('..', import.meta.url))
  const commandsDir = process.argv[2] ?? join(repoRoot, 'src-tauri', 'src', 'commands')
  const apiFile = process.argv[3] ?? join(repoRoot, 'src', 'api', 'index.ts')

  const problems = []
  const rustByName = new Map() // 命令名 → 定义文件（重复定义保留首个并报错）
  for (const file of collectRustFiles(commandsDir)) {
    const { names, errors } = scanRustSource(readFileSync(file, 'utf8'))
    for (const e of errors) {
      problems.push(
        `✗ 扫描器不认识的命令形态（${file}:${e.line}）：${e.text}\n` +
          `  扫描边界：只认裸 #[tauri::command] + 紧随 pub fn / pub async fn；` +
          `带参注解 / cfg 条件命令需同步扩展 src-tauri/build.rs 与本脚本的扫描规则（ADR-0047）`,
      )
    }
    for (const name of names) {
      if (rustByName.has(name)) {
        problems.push(`✗ 命令名重复定义：${name}（${rustByName.get(name)} 与 ${file}）`)
      } else {
        rustByName.set(name, file)
      }
    }
  }

  const tsSet = new Set(scanTsSource(readFileSync(apiFile, 'utf8')))

  // 空集 fail loud（与 build.rs 空集 panic 同界）：任一侧扫不出命令都是灾难性信号
  // （目录指错 / 扫描器失灵），不应以「0 ↔ 0 双向全等」假绿退出。
  if (rustByName.size === 0) {
    problems.push('✗ 未在命令目录扫描到任何 #[tauri::command]——目录为空或路径指错，拒绝以空集假绿通过')
  }
  if (tsSet.size === 0) {
    problems.push('✗ 未在 TS 调用面扫描到任何 invoke 调用——文件为空或路径指错，拒绝以空集假绿通过')
  }

  const missingInTs = [...rustByName.keys()].filter((n) => !tsSet.has(n)).sort()
  const missingInRust = [...tsSet].filter((n) => !rustByName.has(n)).sort()
  if (missingInTs.length > 0) {
    problems.push(
      `✗ 仅在 Rust 注解侧（TS 调用面缺方法，src/api/index.ts 补 invoke 方法）：\n` +
        missingInTs.map((n) => `  - ${n}`).join('\n'),
    )
  }
  if (missingInRust.length > 0) {
    problems.push(
      `✗ 仅在 TS 调用面（Rust 无此命令，删除调用或补后端命令）：\n` +
        missingInRust.map((n) => `  - ${n}`).join('\n'),
    )
  }

  if (problems.length > 0) {
    for (const p of problems) console.error(`命令注册一致性：${p}`)
    console.error(
      `❌ 命令注册一致性校验失败：${problems.length} 处问题` +
        `（命令单一来源 = #[tauri::command] 注解，Rust 注册集与 TS 调用面须双向全等，见 ADR-0047）`,
    )
    process.exit(1)
  }
  console.log(`✓ 命令注册一致性：Rust 注解 ${rustByName.size} ↔ TS 调用面 ${tsSet.size}，双向全等`)
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的扫描函数。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
