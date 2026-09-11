// #1072 码化收口新增错误码的真实码表校验（ADR-0050 决策 5：新增错误条件 =
// 后端码化构造 + errors.json 双语补齐，漏翻由 key 全等门槛拦截）。
//
// 断言两端：中文模板与后端 message 逐字一致（ADR-0050 决策 4 的对照关系）；
// en 文案与后端 message 不同形，命中即证明码表存在且插值生效（非降级透传）。
// 独立文件原因同 errors-closed-set.test.ts：errors.test.ts 的 beforeEach 注入
// 夹具会让 applyLocale 短路、跳过真实 locale bundle；本文件不注夹具。
import { afterEach, describe, expect, it } from 'vitest'
import { errorMessage } from '@/utils/errors'
import { applyLocale } from '@/i18n'

interface Case {
  code: string
  /** 后端 message（zh 模板须逐字一致） */
  message: string
  /** wire params（无插值时缺席） */
  params?: string[]
  /** en 模板期望输出 */
  en: string
}

const cases: Case[] = [
  {
    code: 'budget.period-unknown',
    message: '未知预算周期: quarterly',
    params: ['quarterly'],
    en: 'unknown budget period: quarterly',
  },
  {
    code: 'scheduled-plan.kind-unknown',
    message: '未知定时交易类型: bogus',
    params: ['bogus'],
    en: 'unknown scheduled plan kind: bogus',
  },
  {
    code: 'scheduled-plan.status-unknown',
    message: '未知计划状态: bogus',
    params: ['bogus'],
    en: 'unknown plan status: bogus',
  },
  {
    code: 'scheduled-plan.recurrence-unknown',
    message: '未知周期类型: bogus',
    params: ['bogus'],
    en: 'unknown recurrence type: bogus',
  },
  {
    code: 'scheduled-plan.occurrence-date-invalid',
    message: '无效日期',
    en: 'invalid date',
  },
  {
    code: 'scheduled-occurrence.status-unknown',
    message: '未知期次状态: bogus',
    params: ['bogus'],
    en: 'unknown occurrence status: bogus',
  },
  {
    code: 'instrument.query-required',
    message: 'query 不能为空：标的搜索为搜索式端点，请携带关键词（不做全量列表）',
    en: 'query must not be empty: instrument search is a search-style endpoint, please provide a keyword (no full listing)',
  },
  {
    code: 'db.integrity-check-failed',
    message: '数据库完整性检查失败: row 2 missing from index i',
    params: ['row 2 missing from index i'],
    en: 'database integrity check failed: row 2 missing from index i',
  },
]

describe('errors.json #1072 新增码表（ADR-0050 收口）', () => {
  afterEach(async () => {
    await applyLocale('zh-CN')
  })

  for (const c of cases) {
    it(`${c.code}：zh 模板逐字一致、en 插值命中`, async () => {
      const wire: Record<string, unknown> = { kind: 'Invalid', message: c.message, code: c.code }
      if (c.params) wire.params = c.params
      expect(errorMessage(wire), 'zh 模板与后端 message 逐字一致').toBe(c.message)
      await applyLocale('en-US')
      expect(errorMessage(wire), 'en 模板命中（非降级透传）').toBe(c.en)
    })
  }
})
