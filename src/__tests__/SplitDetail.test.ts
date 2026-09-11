import { describe, it, expect, beforeEach } from 'vitest'
import { wireInvokeSeam } from './helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { NButton, NInput, NInputNumber } from 'naive-ui'
import SplitDetail from '@/components/SplitDetail.vue'
import { makeTransaction } from './factories'
import { formatQuantity } from '@/utils/money'

/**
 * 份额调整只读详情组件测试（ADR-0106 决策 10 / issue #1052）：
 * 只读呈现标的、**带符号**份额变动、调整日与账户，界面不体现任何写操作。
 * 接线级（菜单 / 整卡 → 取数 → 开窗）由 TransactionsView 组件测试覆盖，
 * 本文件只钉组件自身的呈现与只读形态。
 */

beforeEach(async () => {
  // 参考 store 预载走接缝 opt-in（list_accounts 由桩层规范夹具兜底，含 acc-1「现金」行）。
  await wireInvokeSeam({ refreshReferenceStores: true }).ready
})

describe('SplitDetail 份额调整只读详情（ADR-0106 决策 10 / #1052）', () => {
  it('正向 Δ：标的 + 显式「+」份额变动 + 调整日 + 账户；无可编辑输入面、无按钮', async () => {
    const wrapper = mount(SplitDetail, {
      props: {
        transaction: makeTransaction({
          id: 'txn-sp',
          kind: 'split',
          date: '2026-03-01',
          account_id: 'acc-1',
          note: '年度结转',
        }),
        split: {
          instrument_id: 'inst-sp',
          symbol: '502010',
          instrument_name: '证券基金',
          quantity: 339.76,
        },
      },
    })
    await flushPromises()
    // 详情可辨识：以 kind 标签开头（复用 #1048 convert 详情形态）
    expect(wrapper.text()).toContain('份额调整')
    expect(wrapper.text()).toContain('502010 证券基金')
    expect(wrapper.text()).toContain(`+${formatQuantity(339.76)}`)
    expect(wrapper.text()).toContain('2026-03-01')
    expect(wrapper.text()).toContain('现金')
    expect(wrapper.text()).toContain('年度结转')
    // 只读形态：无可编辑输入面、无提交/保存按钮（ADR-0106 决策 10）
    expect(wrapper.findAllComponents(NInput)).toHaveLength(0)
    expect(wrapper.findAllComponents(NInputNumber)).toHaveLength(0)
    expect(wrapper.findAllComponents(NButton)).toHaveLength(0)
  })

  it('负向 Δ（缩股）：符号保留为「-」；标的名称缺失时仅代码', async () => {
    const wrapper = mount(SplitDetail, {
      props: {
        transaction: makeTransaction({ id: 'txn-sp2', kind: 'split', date: '2026-04-01' }),
        split: {
          instrument_id: 'inst-sp',
          symbol: '600000',
          instrument_name: null,
          quantity: -20.5,
        },
      },
    })
    await flushPromises()
    expect(wrapper.text()).toContain('600000')
    expect(wrapper.text()).toContain(`-${formatQuantity(20.5)}`)
  })
})
