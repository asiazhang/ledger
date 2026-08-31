// 拼音可搜下拉（issue #198 试点）：统一模糊搜索语义的前端纯函数实现。
//
// 统一语义契约（ADR-0027，规格真源为 issue #195）：
// 输入按空白切词，词条之间 AND；每个词条对每个字段（下拉：选项 label）判定——
// 命中 = 原文连续子串（大小写不敏感）∨ 词条是该字段拼音首字母串的子序列
//（大小写不敏感）。
//
// 首字母串生成规则：中文字取拼音首字母、ASCII 字母/数字小写原样保留、
// 标点与空格跳过（「ABC银行」→ `abcyh`）；多音字由 pinyin-pro 词组消歧
//（「银行」→ `yh`）。
//
// 混合输入（如「招zsyh」）无需特判：含汉字的词条对纯 ASCII 首字母串的
// 子序列匹配必然失败，自然落到原文子串路径，两条路径任一命中即算。
// 后端 Rust 侧同规格实现在 `src-tauri/src/commands/search/text.rs`。

import { pinyin } from 'pinyin-pro'
import type { SelectOption } from 'naive-ui'

// ---------------------------------------------------------------------------
// 拼音首字母
// ---------------------------------------------------------------------------

/** 逐字调用 pinyin-pro 的高频场景命中有限，按 label 缓存首字母串
 * （实体下拉选项量级小、label 稳定，Map 缓存即可，不做淘汰）。 */
const initialsCache = new Map<string, string>()

/** 生成拼音首字母缩写（小写）。中文字取拼音首字母（词组消歧），ASCII 字母/
 * 数字小写保留，其余字符（标点、空格等）跳过。 */
export function pinyinInitials(text: string): string {
  const cached = initialsCache.get(text)
  if (cached !== undefined) return cached

  const raw = pinyin(text, { pattern: 'first', toneType: 'none', type: 'array' }).join('')
  let out = ''
  for (const ch of raw) {
    if (/[a-z0-9]/i.test(ch)) out += ch.toLowerCase()
  }
  initialsCache.set(text, out)
  return out
}

// ---------------------------------------------------------------------------
// 统一语义匹配
// ---------------------------------------------------------------------------

/** 大小写不敏感的子序列判定：`pattern` 的每个字符按原顺序出现在 `target` 中
 * （允许跳字，间隔不限）。空 pattern 恒命中。两侧均按 code point 迭代比较，
 * astral 字符（emoji、生僻字）不错配。 */
export function isSubsequence(pattern: string, target: string): boolean {
  const targetChars = [...target.toLowerCase()]
  let cursor = 0
  for (const p of pattern.toLowerCase()) {
    let found = false
    while (cursor < targetChars.length) {
      if (targetChars[cursor] === p) {
        found = true
        cursor++
        break
      }
      cursor++
    }
    if (!found) return false
  }
  return true
}

/** 词条对单个字段（选项 label）判定（统一语义契约）：
 * 原文连续子串（大小写不敏感）∨ 该字段拼音首字母串的子序列（大小写不敏感）。 */
function termMatchesText(term: string, text: string): boolean {
  return (
    text.toLowerCase().includes(term.toLowerCase()) ||
    isSubsequence(term, pinyinInitials(text))
  )
}

/** 完整输入判定：按空白切词（词条之间 AND），空输入恒命中（恢复完整列表）。 */
export function matchLabel(pattern: string, label: string): boolean {
  return pattern
    .split(/\s+/)
    .filter(Boolean)
    .every((term) => termMatchesText(term, label))
}

// ---------------------------------------------------------------------------
// NSelect filter 收口
// ---------------------------------------------------------------------------

/** NSelect `filter` prop 签名（`SelectFilter`）：对选项 label 做统一语义判定。
 * 非 string/number 的 label（渲染函数）无法参与文本匹配，恒显示。 */
export function pinyinFilter(pattern: string, option: SelectOption): boolean {
  const { label } = option
  if (typeof label === 'string') return matchLabel(pattern, label)
  if (typeof label === 'number') return matchLabel(pattern, String(label))
  return true
}
