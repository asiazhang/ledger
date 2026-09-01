#!/usr/bin/env node
// i18n 文案 key 全等校验（issue #342 / ADR-0049）：源语言 zh-CN 与其余各 locale
// 的 key 集合双向全等——任一方向孤儿（源语言独有 / 其他 locale 独有）即非零退出，
// 漏翻在合入前被拦截。仿命令集双向全等校验先例（scripts/check-commands.js）：
// 纯函数导出供单测（src/__tests__/check-i18n-keys.test.ts），CLI 入口可独立运行。
// 默认校验本仓库；测试可传位置参数指向夹具目录：node scripts/check-i18n-keys.js [locales-dir]
// 挂载于 scripts/check.sh 质量门槛序列与 CI。

import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'

/** 源语言目录名（其余 locale 一律与它比对） */
export const SOURCE_LOCALE_DIR = 'zh-CN'

/**
 * 递归展开 JSON 对象为点分 key 全集（叶子 key 才计入；数组整体视为叶子）。
 * @returns {string[]} 排序后的点分 key 列表
 */
export function flattenKeys(obj, prefix = '') {
  const keys = []
  for (const [k, v] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${k}` : k
    if (v !== null && typeof v === 'object' && !Array.isArray(v)) {
      keys.push(...flattenKeys(v, path))
    } else {
      keys.push(path)
    }
  }
  return keys.sort()
}

/**
 * 双向集合差：sourceOnly = 源语言独有（其他 locale 漏翻），otherOnly = 其他 locale 独有。
 * @returns {{ sourceOnly: string[], otherOnly: string[] }}
 */
export function diffKeySets(sourceKeys, otherKeys) {
  const source = new Set(sourceKeys)
  const other = new Set(otherKeys)
  return {
    sourceOnly: sourceKeys.filter((k) => !other.has(k)),
    otherOnly: otherKeys.filter((k) => !source.has(k)),
  }
}

/** 读取单个 locale 目录：聚合同目录全部 *.json 的点分 key（文件名作为顶层域前缀） */
export function collectLocaleKeys(dir) {
  const keys = []
  for (const entry of readdirSync(dir).sort()) {
    if (!entry.endsWith('.json')) continue
    const domain = entry.slice(0, -'.json'.length)
    const parsed = JSON.parse(readFileSync(join(dir, entry), 'utf-8'))
    keys.push(...flattenKeys(parsed, domain))
  }
  return keys.sort()
}

/**
 * 比对 locales 目录下源语言与其余 locale 的 key 集合。
 * @returns {{ sourceLocale: string, locales: string[], failures: Array<{locale: string, sourceOnly: string[], otherOnly: string[]}> }}
 */
export function compareLocalesDir(localesDir) {
  const dirs = readdirSync(localesDir, { withFileTypes: true })
    .filter((e) => e.isDirectory())
    .map((e) => e.name)
    .sort()
  const sourceKeys = collectLocaleKeys(join(localesDir, SOURCE_LOCALE_DIR))
  const failures = []
  for (const locale of dirs) {
    if (locale === SOURCE_LOCALE_DIR) continue
    const diff = diffKeySets(sourceKeys, collectLocaleKeys(join(localesDir, locale)))
    if (diff.sourceOnly.length > 0 || diff.otherOnly.length > 0) {
      failures.push({ locale, ...diff })
    }
  }
  return { sourceLocale: SOURCE_LOCALE_DIR, locales: dirs, failures }
}

function main() {
  const localesDir = process.argv[2] ?? new URL('../src/i18n/locales', import.meta.url).pathname
  const { sourceLocale, locales, failures } = compareLocalesDir(localesDir)
  if (failures.length === 0) {
    console.log(`✅ i18n key 全等：${locales.join(' / ')} 各语言 key 集合与源语言 ${sourceLocale} 全等`)
    return
  }
  for (const f of failures) {
    console.error(`✗ locale「${f.locale}」与源语言「${sourceLocale}」key 集合不一致：`)
    for (const key of f.sourceOnly) console.error(`  - 缺失：${key}`)
    for (const key of f.otherOnly) console.error(`  - 多余：${key}`)
  }
  process.exit(1)
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的比对函数
// （与 scripts/check-commands.js 同一惯法）。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
