import { describe, it, expect } from 'vitest'
import { mockInvoke, wireInvokeSeam } from './helpers/invoke-mock'
import { messageApi } from './helpers/message-mock'
import { findButton } from './helpers/dom'
import { mount, flushPromises } from '@vue/test-utils'
import type { NotePinyinRepairReport } from '@ledger/types'

import SearchDataSettings from '@/components/settings/SearchDataSettings.vue'

const convergedReport: NotePinyinRepairReport = {
  backfilled: 3,
  converged: true,
  failure: null,
}

describe('SearchDataSettings.vue', () => {
  it('点击一键修复调用 repair_note_pinyin，收敛报告就地展示（回填行数）', async () => {
    wireInvokeSeam({ defaults: { repair_note_pinyin: convergedReport } })
    const wrapper = mount(SearchDataSettings)
    await findButton(wrapper, '一键修复')!.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('repair_note_pinyin')
    const html = wrapper.html()
    expect(html).toContain('修复完成：已回填 3 行')
    expect(html).toContain('积压清零')
    expect(messageApi.success).toHaveBeenCalled()
    expect(messageApi.warning).not.toHaveBeenCalled()
  })

  it('零回填且收敛：呈现「无需修复」而非成功回填计数', async () => {
    wireInvokeSeam({ defaults: { repair_note_pinyin: { backfilled: 0, converged: true, failure: null } } })
    const wrapper = mount(SearchDataSettings)
    await findButton(wrapper, '一键修复')!.trigger('click')
    await flushPromises()
    const html = wrapper.html()
    expect(html).toContain('无需修复')
    expect(html).toContain('本次回填 0 行')
    expect(messageApi.success).toHaveBeenCalled()
  })

  it('失败报告：warning 呈现失败阶段（本地化）与原因，不静默', async () => {
    wireInvokeSeam({
      defaults: {
        repair_note_pinyin: {
          backfilled: 1,
          converged: false,
          failure: { stage: 'write', message: 'database is locked' },
        },
      },
    })
    const wrapper = mount(SearchDataSettings)
    await findButton(wrapper, '一键修复')!.trigger('click')
    await flushPromises()
    const html = wrapper.html()
    expect(html).toContain('修复未全部完成')
    expect(html).toContain('失败阶段：写入')
    expect(html).toContain('database is locked')
    expect(messageApi.warning).toHaveBeenCalled()
    expect(messageApi.success).not.toHaveBeenCalled()
  })

  it('命令本身异常（非报告失败）：错误反馈', async () => {
    wireInvokeSeam({
      overrides: { repair_note_pinyin: () => Promise.reject(new Error('boom')) },
    })
    const wrapper = mount(SearchDataSettings)
    await findButton(wrapper, '一键修复')!.trigger('click')
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalled()
    expect(wrapper.html()).not.toContain('修复完成')
  })
})
