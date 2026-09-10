import { pushMock, makeTxn, mountView, setTxnDb } from './common'
import { describe, it, expect, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { NSelect } from 'naive-ui'
import AccountLink from '@/components/AccountLink.vue'
import type { Transaction } from '@/types'

/** 买入/卖出行出资账户双链接（issue #937 / ADR-0096）：出资账户命中的 buy/sell
 * 行账户列按转账同款「出资账户 → 投资账户」双向账户名展示，两端各自可点击下钻；
 * 不带出资账户的 buy/sell 与其余 kind 展示不变（单主账户名）。 */
describe('TransactionsView 出资账户行双向账户名（issue #937）', () => {
  beforeEach(() => {
    setTxnDb([])
  })

  /** 类型下拉（过滤行第 3 个 NSelect）直接 emit 变更（与转账行测试同模式）。 */
  async function filterKind(wrapper: ReturnType<typeof mount>, k: string | null) {
    wrapper.findAllComponents(NSelect)[2].vm.$emit('update:value', k)
    await flushPromises()
  }

  /** 出资账户买入行：投资账户 acc-1（现金）+ 出资账户 acc-2（银行）。 */
  const fundingBuy: Transaction = makeTxn(1, 'acc-1', {
    kind: 'buy',
    funding_account_id: 'acc-2',
  })

  it('买入行（带出资账户）显示「出资账户 → 投资账户」双向账户名，两端各自可点击、各自跳转对应账户', async () => {
    setTxnDb([fundingBuy])
    const wrapper = await mountView()
    await filterKind(wrapper, 'buy')
    // 双向展示：出资账户在前、投资账户在后（转账同款「转出 → 转入」风格）
    const links = wrapper.findAllComponents(AccountLink)
    expect(links.length).toBe(2)
    expect(links.map((l) => l.text())).toEqual(['银行', '现金'])
    expect(wrapper.text()).toContain('→')
    // 出资账户点击 → 下钻出资账户过滤视图
    await links[0].find('button').trigger('click')
    expect(pushMock).toHaveBeenLastCalledWith({
      name: 'transactions',
      query: { account: 'acc-2' },
    })
    // 投资账户点击 → 下钻投资账户过滤视图
    await links[1].find('button').trigger('click')
    expect(pushMock).toHaveBeenLastCalledWith({
      name: 'transactions',
      query: { account: 'acc-1' },
    })
  })

  it('卖出行（带出资账户）同样双向展示（buy/sell 同口径）', async () => {
    setTxnDb([
      makeTxn(1, 'acc-1', { kind: 'sell', funding_account_id: 'acc-2' }),
    ])
    const wrapper = await mountView()
    await filterKind(wrapper, 'sell')
    const links = wrapper.findAllComponents(AccountLink)
    expect(links.map((l) => l.text())).toEqual(['银行', '现金'])
    expect(wrapper.text()).toContain('→')
  })

  it('不带出资账户的买入行仍显示单个主账户名（余额买入语义展示不变）', async () => {
    setTxnDb([makeTxn(1, 'acc-1', { kind: 'buy' })])
    const wrapper = await mountView()
    await filterKind(wrapper, 'buy')
    const links = wrapper.findAllComponents(AccountLink)
    expect(links.length).toBe(1)
    expect(links[0].text()).toBe('现金')
    expect(wrapper.text()).not.toContain('→')
  })

  it('不带出资账户的卖出行与其余 kind 展示不变', async () => {
    setTxnDb([
      makeTxn(1, 'acc-1', { kind: 'sell' }),
      makeTxn(2, 'acc-1', { kind: 'expense' }),
    ])
    const wrapper = await mountView()
    const links = wrapper.findAllComponents(AccountLink)
    expect(links.length).toBe(2)
    for (const link of links) {
      expect(link.text()).toBe('现金')
    }
    expect(wrapper.text()).not.toContain('→')
  })
})
