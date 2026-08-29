import { pushMock, makeTxn, mountView, setTxnDb } from './common'
import { describe, it, expect, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { NSelect } from 'naive-ui'
import AccountLink from '@/components/AccountLink.vue'
import type { Transaction } from '@/types'

describe('TransactionsView 转账行双向账户名（issue #99）', () => {
  // 混合数据集：转账行（txn-2: acc-2 → acc-1）与普通行并存，供双向展示 / 单账户名断言
  const mixedDb: Transaction[] = [
    makeTxn(1, 'acc-1', { kind: 'expense' }),
    makeTxn(2, 'acc-2', { kind: 'transfer', to_account_id: 'acc-1' }),
    makeTxn(3, 'acc-1', { kind: 'income' }),
  ]

  beforeEach(() => {
    setTxnDb([...mixedDb])
  })

  /** 类型下拉（过滤行第 2 个 NSelect）直接 emit 变更（与 issue #98 测试同模式）。 */
  async function filterKind(wrapper: ReturnType<typeof mount>, k: string | null) {
    wrapper.findAllComponents(NSelect)[1].vm.$emit('update:value', k)
    await flushPromises()
  }

  it('转账行账户列显示「转出 → 转入」双向账户名，两个名字各自可点击、各自跳转对应账户', async () => {
    const wrapper = await mountView()
    await filterKind(wrapper, 'transfer')
    // 双向展示：两个账户名（转出 acc-2、转入 acc-1）+ 箭头分隔
    const links = wrapper.findAllComponents(AccountLink)
    expect(links.length).toBe(2)
    expect(links.map((l) => l.text())).toEqual(['银行', '现金'])
    expect(wrapper.text()).toContain('→')
    // 转出账户点击 → 跳转其过滤视图
    await links[0].find('button').trigger('click')
    expect(pushMock).toHaveBeenLastCalledWith({
      name: 'transactions',
      query: { account: 'acc-2' },
    })
    // 转入账户点击 → 跳转其过滤视图
    await links[1].find('button').trigger('click')
    expect(pushMock).toHaveBeenLastCalledWith({
      name: 'transactions',
      query: { account: 'acc-1' },
    })
  })

  it('非转账行账户列仍显示单个主账户名（可点击，带 title 提示）', async () => {
    const wrapper = await mountView()
    await filterKind(wrapper, 'income')
    const links = wrapper.findAllComponents(AccountLink)
    expect(links.length).toBe(1)
    expect(links[0].text()).toBe('现金')
    expect(links[0].attributes('title')).toBe('查看该账户的交易')
    await links[0].find('button').trigger('click')
    expect(pushMock).toHaveBeenLastCalledWith({
      name: 'transactions',
      query: { account: 'acc-1' },
    })
  })
})

