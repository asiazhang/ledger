import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import NoteCopyButton from '@/components/NoteCopyButton.vue'
import { messageApi } from './helpers/message-mock'

/** 备注复制按钮（显式复制通道，见「界面文本不可选」词条）：复制走 clipboard API，
 * 反馈走消息接口——两处均为外部接缝，分别以注入替身断言（AboutSettings 复制先例同款）。 */
const writeText = vi.fn().mockResolvedValue(undefined)

beforeEach(() => {
  writeText.mockClear()
  writeText.mockResolvedValue(undefined)
  Object.assign(navigator, { clipboard: { writeText } })
})

describe('NoteCopyButton 备注复制按钮', () => {
  it('点击复制完整备注文本', async () => {
    const wrapper = mount(NoteCopyButton, { props: { note: '视频会员月费' } })
    await wrapper.find('button').trigger('click')
    await flushPromises()
    expect(writeText).toHaveBeenCalledTimes(1)
    expect(writeText).toHaveBeenCalledWith('视频会员月费')
  })

  it('成功弹「备注已复制」toast', async () => {
    const wrapper = mount(NoteCopyButton, { props: { note: 'n1' } })
    await wrapper.find('button').trigger('click')
    await flushPromises()
    expect(messageApi.success).toHaveBeenCalledTimes(1)
    expect(messageApi.success).toHaveBeenCalledWith('备注已复制')
  })

  it('失败弹错误 toast 并透传原因', async () => {
    writeText.mockRejectedValueOnce(new Error('not allowed'))
    const wrapper = mount(NoteCopyButton, { props: { note: 'n2' } })
    await wrapper.find('button').trigger('click')
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalledTimes(1)
    expect(messageApi.error.mock.calls[0][0]).toContain('复制失败')
  })

  it('无障碍标签与悬停提示为「复制备注」（ADR-0049 文案经 i18n）', () => {
    const wrapper = mount(NoteCopyButton, { props: { note: 'n3' } })
    const btn = wrapper.find('button')
    expect(btn.attributes('aria-label')).toBe('复制备注')
    expect(btn.attributes('title')).toBe('复制备注')
  })
})
