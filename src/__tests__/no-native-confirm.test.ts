import { describe, it, expect } from 'vitest'
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

/**
 * 原生对话框模态确认清零守门（issue #652 / ADR-0078 决策 1）：
 * 模态危险确认一律应用内弹窗（AppDangerConfirmModal 分级封装），系统原生
 * `confirm()` 在本仓库无存量、不容新增——新增危险操作确认走共享封装。
 * plugin-dialog 依赖保留：`open` / `save` 的文件选择不受影响。
 * 守门为文本级扫描：import 子句中的 `confirm` 标识符即红（动态 import 解构等
 * 间接形态文本不可达，靠评审兜底，与 check-structure.ts 同款口径）。
 */

const SRC_ROOT = join(import.meta.dirname, '..')

/** 递归收集 src/ 下参与扫描的源文件（ts / vue）。 */
function collectFiles(dir: string, acc: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name)
    if (entry.isDirectory()) collectFiles(full, acc)
    else if (entry.name.endsWith('.ts') || entry.name.endsWith('.vue')) acc.push(full)
  }
  return acc
}

/** import 子句（含静态与动态 import()）中是否从 plugin-dialog 引入 confirm。 */
function importsConfirmFromPluginDialog(code: string): boolean {
  // 静态 import 子句：具名导入中出现 confirm 标识符即候选（含换行形态）
  const moduleSpec = ['@tauri-apps', 'plugin-dialog'].join('/')
  const staticRe =
    /import\s*\{[^}]*\bconfirm\b[^}]*\}\s*from\s*['"]([\w@/-]+)['"]/g
  // 动态：import 该模块后解构 confirm 属潜在旁路，一并拦截
  // （测试文件对 open 的动态引入不含 confirm，不误伤）。
  const dynamicMatch = code.match(
    /import\(\s*['"]([\w@/-]+)['"]\s*\)(?:\s*\.\s*then[^;]*)?;/g,
  )
  if (dynamicMatch) {
    for (const m of dynamicMatch) {
      if (m.includes(moduleSpec) && /\bconfirm\b/.test(m)) return true
    }
  }
  for (const m of code.matchAll(staticRe)) {
    if (m[1] === moduleSpec) return true
  }
  return false
}

describe('原生 confirm 清零（issue #652 / ADR-0078 决策 1）', () => {
  it('src/ 全树不再从 @tauri-apps/plugin-dialog 引入 confirm（open/save 不受影响）', () => {
    const offenders = collectFiles(SRC_ROOT)
      .map((path) => ({ path, code: readFileSync(path, 'utf8') }))
      .filter(({ code }) => importsConfirmFromPluginDialog(code))
      .map(({ path }) => path)
    expect(offenders, `以下文件仍从 plugin-dialog 引入 confirm：${offenders.join(', ')}`).toEqual(
      [],
    )
  })
})
