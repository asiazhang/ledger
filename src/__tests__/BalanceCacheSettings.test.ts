import { describe, it, expect, beforeEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { messageApi } from '@ledger/test-support/message-mock'
import { findButton } from '@ledger/test-support/dom'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import type { BalanceCacheAudit } from '@ledger/types'

import BalanceCacheSettings from '@/components/settings/BalanceCacheSettings.vue'

/** 有漂移的报告（含「缓存行缺失」形态：cached_cents 为 null）。 */
const driftReport: BalanceCacheAudit = {
  accounts_checked: 19,
  drifts: [
    { account_id: 'acc-1', account_name: '老婆的且慢', cached_cents: null, actual_cents: -4100000 },
    { account_id: 'acc-2', account_name: '且慢', cached_cents: 9602657, actual_cents: 8765093 },
  ],
  repaired: true,
}

/** 零漂移的报告。 */
const cleanReport: BalanceCacheAudit = {
  accounts_checked: 19,
  drifts: [],
  repaired: false,
}

describe('BalanceCacheSettings.vue', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('点击一键修复调用 audit_balance_cache，逐户差异就地展示（含缓存缺失形态）', async () => {
    wireInvokeSeam({ defaults: { audit_balance_cache: driftReport } })
    const wrapper = mount(BalanceCacheSettings)
    await findButton(wrapper, '一键修复')!.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('audit_balance_cache')
    const html = wrapper.html()
    expect(html).toContain('修复完成：已校准 2 个账户')
    // 缓存缺失不显示成 0：独立文案呈现
    expect(html).toContain('（缓存行缺失）')
    // 两行账户名与实时重算值在场
    expect(html).toContain('老婆的且慢')
    expect(html).toContain('且慢')
    expect(messageApi.success).toHaveBeenCalled()
  })

  it('零漂移：呈现「无需修复」而非成功计数，且不渲染差异表', async () => {
    wireInvokeSeam({ defaults: { audit_balance_cache: cleanReport } })
    const wrapper = mount(BalanceCacheSettings)
    await findButton(wrapper, '一键修复')!.trigger('click')
    await flushPromises()
    const html = wrapper.html()
    expect(html).toContain('无需修复')
    expect(html).toContain('全部 19 个账户')
    expect(html).not.toContain('修复完成：已校准')
    expect(wrapper.find('[data-testid="balance-cache-drifts"]').exists()).toBe(false)
  })

  it('命令异常：错误反馈，且不呈现修复完成报告', async () => {
    wireInvokeSeam({
      overrides: { audit_balance_cache: () => Promise.reject(new Error('database is locked')) },
    })
    const wrapper = mount(BalanceCacheSettings)
    await findButton(wrapper, '一键修复')!.trigger('click')
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalled()
    expect(wrapper.html()).not.toContain('修复完成')
  })
})
