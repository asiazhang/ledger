import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { S3_VENDOR_PRESETS } from '@/utils/s3-vendors'

/**
 * 真桶验收清单 ↔ 预设档位一致守门（issue #1222，父 spec #1214）。
 *
 * 界面上「已实测 / 未实测」的档位只有一个来源——预设表的 `verified` 字段；
 * 清单文档 `docs/verification/1222-s3-vendor-acceptance.md` 是人读结论。两者
 * 漂移（翻了档位没改清单，或改了清单没翻档位）会让用户在界面上读到一个与
 * 文档不符的档位，而这正是本票「档位与清单一致」的验收判据。
 *
 * 逐行对齐口径：预设 id 集合两侧全等、结论与档位逐家一致、寻址方式与预设默认
 * 一致、官方文档外链在册。文案改写的自由（措辞、增列）不受限——判据落在
 * 「预设 id / 寻址方式 / 已知限制 / 结论」四列。
 */

const DOC_PATH = join(import.meta.dirname, '..', '..', 'docs/verification/1222-s3-vendor-acceptance.md')

const TIER_VERIFIED = '已实测'
const TIER_UNVERIFIED = '未实测'
const ADDRESSING_VIRTUAL_HOST = '虚拟托管'
const ADDRESSING_PATH_STYLE = 'path-style'

/** 清单表一行：某家厂商的结论、寻址方式与已知限制。 */
interface AcceptanceRow {
  readonly addressing: string
  readonly limitation: string
  readonly conclusion: string
}

interface AcceptanceTable {
  readonly header: readonly string[]
  readonly rows: ReadonlyMap<string, AcceptanceRow>
}

/** markdown 的连续表格行分组（空行/非表格行即断开）。 */
function tableBlocks(markdown: string): string[][] {
  const blocks: string[][] = []
  let current: string[] = []
  for (const line of markdown.split('\n')) {
    if (line.trim().startsWith('|')) {
      current.push(line.trim())
    } else if (current.length > 0) {
      blocks.push(current)
      current = []
    }
  }
  if (current.length > 0) blocks.push(current)
  return blocks
}

/** 表格行 → 单元格（去掉首尾竖线与单元格空白）。 */
function cells(line: string): string[] {
  return line
    .replace(/^\|/, '')
    .replace(/\|$/, '')
    .split('|')
    .map((cell) => cell.trim())
}

/** 表头分隔行（`| --- | --- |` 一类），不参与断言。 */
function isSeparator(row: readonly string[]): boolean {
  return row.every((cell) => /^:?-{2,}:?$/.test(cell))
}

/**
 * 按列名取列号：精确命中优先，其次接受「列名 + 括号限定」的写法（如
 * `寻址方式（预设默认）`）——限定语是给人读的注解，不改变判据列的身份。
 */
function columnIndex(header: readonly string[], label: string): number {
  const exact = header.indexOf(label)
  if (exact >= 0) return exact
  return header.findIndex((cell) => cell.startsWith(`${label}（`))
}

/**
 * 取「厂商清单表」：表头同时含 `预设 id` 与 `结论` 的那一张（文档里另有环境
 * 变量表与记录表，按此判据天然排除）。找不到即抛错——清单表被删就是本守门失靶。
 */
function acceptanceTable(markdown: string): AcceptanceTable {
  for (const block of tableBlocks(markdown)) {
    const [headerLine, ...bodyLines] = block
    const header = cells(headerLine)
    if (!header.includes('预设 id') || !header.includes('结论')) continue
    const idAt = columnIndex(header, '预设 id')
    const addressingAt = columnIndex(header, '寻址方式')
    const limitationAt = columnIndex(header, '已知限制')
    const conclusionAt = columnIndex(header, '结论')
    if (addressingAt < 0 || limitationAt < 0) {
      throw new Error(
        `清单表缺列：表头需含「寻址方式」与「已知限制」（见第 3 节），实际表头 ${header.join(' / ')}`,
      )
    }
    const rows = new Map<string, AcceptanceRow>()
    for (const line of bodyLines) {
      const row = cells(line)
      if (isSeparator(row)) continue
      const id = row[idAt].replace(/`/g, '')
      if (rows.has(id)) throw new Error(`清单表出现重复的预设 id：${id}`)
      rows.set(id, {
        addressing: row[addressingAt],
        limitation: row[limitationAt],
        conclusion: row[conclusionAt],
      })
    }
    return { header, rows }
  }
  throw new Error(`未找到厂商清单表（表头需含「预设 id」与「结论」）：${DOC_PATH}`)
}

const markdown = readFileSync(DOC_PATH, 'utf8')
const table = acceptanceTable(markdown)

describe('真桶验收清单与预设档位一致', () => {
  it('清单覆盖全部预设厂商，不多不少', () => {
    const docIds = [...table.rows.keys()].sort()
    const presetIds = S3_VENDOR_PRESETS.map((preset) => preset.id).sort()
    expect(docIds).toEqual(presetIds)
  })

  it('清单结论只取「已实测 / 未实测」闭集', () => {
    const tiers = [...table.rows].map(([id, row]) => `${id}: ${row.conclusion}`)
    expect(tiers.every((entry) => entry.endsWith(` ${TIER_VERIFIED}`) || entry.endsWith(` ${TIER_UNVERIFIED}`))).toBe(
      true,
    )
  })

  it('逐家结论与预设 verified 档位一致（翻了档位必须改清单）', () => {
    for (const preset of S3_VENDOR_PRESETS) {
      const row = table.rows.get(preset.id)
      expect(row, `清单缺 ${preset.id}`).toBeDefined()
      const expected = preset.verified ? TIER_VERIFIED : TIER_UNVERIFIED
      expect(row?.conclusion, `${preset.name}（${preset.id}）档位与清单不一致`).toBe(expected)
    }
  })

  it('逐家寻址方式与预设默认一致', () => {
    for (const preset of S3_VENDOR_PRESETS) {
      const expected = preset.pathStyle ? ADDRESSING_PATH_STYLE : ADDRESSING_VIRTUAL_HOST
      expect(table.rows.get(preset.id)?.addressing, `${preset.name}（${preset.id}）寻址方式`).toBe(
        expected,
      )
    }
  })

  it('逐家都写了已知限制（未实测不得留空）', () => {
    for (const preset of S3_VENDOR_PRESETS) {
      expect(
        table.rows.get(preset.id)?.limitation,
        `${preset.name}（${preset.id}）已知限制为空`,
      ).not.toBe('')
    }
  })

  it('逐家官方文档外链在册', () => {
    for (const preset of S3_VENDOR_PRESETS) {
      expect(markdown, `${preset.name}（${preset.id}）缺官方文档外链`).toContain(preset.docsUrl)
    }
  })

  it('清单表列齐（厂商 / 预设 id / 寻址方式 / 已知限制 / 结论）', () => {
    for (const column of ['厂商', '预设 id', '寻址方式', '已知限制', '结论']) {
      expect(columnIndex(table.header, column), `清单表缺列「${column}」`).toBeGreaterThanOrEqual(0)
    }
  })
})
