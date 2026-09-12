// #1072 码化收口与后续新增错误码的真实码表校验（ADR-0050 决策 5：新增错误条件 =
// 后端码化构造 + errors.json 双语补齐，漏翻由 key 全等门槛拦截；#1053 的份额调整
// 重放两码随本文件一并钉住）。
//
// 断言两端：中文模板与后端 message 逐字一致（ADR-0050 决策 4 的对照关系）；
// en 文案与后端 message 不同形，命中即证明码表存在且插值生效（非降级透传）。
// 独立文件原因同 errors-closed-set.test.ts：errors.test.ts 的 beforeEach 注入
// 夹具会让 applyLocale 短路、跳过真实 locale bundle；本文件不注夹具。
import { afterEach, describe, expect, it } from 'vitest'
import { errorMessage } from '@/utils/errors'
import { applyLocale } from '@ledger/i18n'

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
    // 检查结果只是插值参数原样透传：用合成细节串，不耦合 SQLite 版本的输出文本
    // （真实结果形态由 Rust 侧 db/tests/integrity.rs 用实际 pragma 输出断言）。
    message: '数据库完整性检查失败: integrity detail',
    params: ['integrity detail'],
    en: 'database integrity check failed: integrity detail',
  },
  {
    // #1053 份额调整重放的旧载荷挂起码（与 convert-fields-missing 同规）。
    code: 'transaction.split-fields-missing',
    message:
      '该份额调整操作缺少同步所需的份额调整字段（产生自较早版本），无法在本机重放；请在来源设备上删除并重新录入该份额调整后再次同步',
    en: 'this share adjustment lacks the fields required for sync replay (produced by an earlier version) and cannot be replayed here; delete and re-enter the share adjustment on the source device, then sync again',
  },
  {
    // #1053 份额调整重放的快照发散挂起码：四参插值（源端持仓 / 成本、本机持仓 / 成本）。
    code: 'transaction.split-restatement-mismatch',
    message: '份额调整重放校验不一致（源端 最终持仓 999、总成本 1；本机重建 290、46000），已挂起等待处理',
    params: ['999', '1', '290', '46000'],
    en: 'share-adjustment replay check mismatch (source final holding 999, total cost 1; local rebuild 290, 46000); parked for review',
  },
  // #1078 现金分红：kind 变更守卫与形态守卫（ADR-0109）。
  {
    code: 'trade.dividend-kind-change-forbidden',
    message: '不可将交易类型改为或改出「分红」：换交易类型的纠错只有「删除后重建」一条路',
    en: 'transaction type cannot be changed to or from "dividend": changing the type is only correctable by deleting and re-creating',
  },
  {
    code: 'trade.dividend-instrument-required',
    message: '分红必须指定标的',
    en: 'a dividend must reference an instrument',
  },
  {
    code: 'trade.dividend-instrument-not-found',
    message: '分红标的不存在: inst-x',
    params: ['inst-x'],
    en: 'dividend instrument does not exist: inst-x',
  },
  {
    code: 'trade.dividend-to-instrument-forbidden',
    message: '分红是单标的现金收入，不能携带转入标的',
    en: 'a dividend is a single-instrument cash inflow and cannot carry a target instrument',
  },
  {
    code: 'trade.dividend-to-account-forbidden',
    message: '分红不跨账户，不能携带转入账户',
    en: 'a dividend does not move between accounts and cannot carry a target account',
  },
  {
    code: 'trade.dividend-quantity-forbidden',
    message: '分红无份额变动，不可提供数量',
    en: 'a dividend has no share change; quantity is not accepted',
  },
  {
    code: 'trade.dividend-price-forbidden',
    message: '分红无成交单价，不可提供单价',
    en: 'a dividend has no unit price; price_cents is not accepted',
  },
  {
    code: 'trade.dividend-fee-forbidden',
    message: '分红不接受手续费',
    en: 'a dividend does not accept a fee',
  },
  {
    code: 'trade.dividend-amount-positive',
    message: '分红金额必须大于 0',
    en: 'the dividend amount must be greater than 0',
  },
  {
    code: 'trade.dividend-currency-mismatch',
    message: '分红币种（CNY）必须与到账账户币种（USD）一致',
    params: ['CNY', 'USD'],
    en: 'the dividend currency (CNY) must match the destination account currency (USD)',
  },
  {
    code: 'trade.dividend-account-not-found',
    message: '分红到账账户不存在或已删除: acc-x',
    params: ['acc-x'],
    en: 'the dividend destination account does not exist or was deleted: acc-x',
  },
]

describe('errors.json 新增码表（ADR-0050 收口）', () => {
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
