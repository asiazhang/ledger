// `closed_set!` 两枚举未知值错误码的真实码表校验（#1071）。
//
// 独立文件的原因：`errors.test.ts` 的 beforeEach 用 mergeLocaleMessage 注入夹具，
// 会让该文件内 applyLocale 因 availableLocales 已含 en-US 而短路、跳过真实 locale
// bundle；本文件不注夹具，applyLocale 走真实 en-US errors.json。
//
// 断言两端：中文模板与后端 message 逐字一致（ADR-0050 决策 4 的对照关系）；
// en 文案与后端 message 不同形，命中即证明码表存在且插值生效（非降级透传）。
// 合法值清单经 `{1}` 插值——不在码表另抄一份（ADR-0108）。
import { afterEach, describe, expect, it } from 'vitest'
import { errorMessage } from '@/utils/errors'
import { applyLocale } from '@/i18n'

describe('errors.json 闭集未知值码表（#1071）', () => {
  afterEach(async () => {
    await applyLocale('zh-CN')
  })

  it('transaction.kind-unknown：未知值 + 合法值清单插值', async () => {
    const legal = 'income/expense/transfer/refund/buy/sell/dividend/split/convert'
    const message = `未知交易类型: bonus（合法值: ${legal}）`
    const wire = {
      kind: 'Invalid',
      message,
      code: 'transaction.kind-unknown',
      params: ['bonus', legal],
    }
    expect(errorMessage(wire), 'zh 模板与后端 message 逐字一致').toBe(message)
    await applyLocale('en-US')
    expect(errorMessage(wire)).toBe(`unknown transaction kind: bonus (valid values: ${legal})`)
  })

  it('instrument.type-unknown：未知值 + 合法值清单插值', async () => {
    const legal = 'stock/fund/bond/etf/other'
    const message = `未知金融工具类型: etfs（合法值: ${legal}）`
    const wire = {
      kind: 'Invalid',
      message,
      code: 'instrument.type-unknown',
      params: ['etfs', legal],
    }
    expect(errorMessage(wire), 'zh 模板与后端 message 逐字一致').toBe(message)
    await applyLocale('en-US')
    expect(errorMessage(wire)).toBe(`unknown instrument type: etfs (valid values: ${legal})`)
  })
})
