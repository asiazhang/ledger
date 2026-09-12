import { describe, it, expect, vi, beforeEach } from 'vitest'
import { wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { flushPromises, mount } from '@vue/test-utils'
import AiPromptView from '@/views/AiPromptView.vue'

const writeText = vi.fn().mockResolvedValue(undefined)

const SAMPLE_PROMPT = `# Ledger API 入口提示词

开源记账在本地 http://127.0.0.1:9527 提供 HTTP API，支持两种并列的会话形态：**AI 记账**与**数据迁移**。

1. 先 GET /api/v1/contract 发现全部端点。
2. 写交易前，先 GET /api/v1/import/knowledge 获取导入全流程约定。`

beforeEach(() => {
  wireInvokeSeam({ overrides: { get_ai_prompt: SAMPLE_PROMPT } })
  Object.assign(navigator, { clipboard: { writeText } })
  writeText.mockClear()
})

describe('AiPromptView.vue', () => {
  it('加载并展示提示词全文', async () => {
    const wrapper = mount(AiPromptView)
    await flushPromises()
    const body = wrapper.find('[data-testid="prompt-body"]')
    expect(body.text()).toContain('# Ledger API 入口提示词')
    expect(body.text()).toContain('/api/v1/import/knowledge')
  })

  it('点击复制按钮调用剪贴板写入', async () => {
    const wrapper = mount(AiPromptView)
    await flushPromises()
    await wrapper.find('button').trigger('click')
    expect(writeText).toHaveBeenCalledWith(SAMPLE_PROMPT)
  })

  it('提示词为空时复制按钮禁用', async () => {
    wireInvokeSeam({ defaults: { get_ai_prompt: '' } })
    const wrapper = mount(AiPromptView)
    await flushPromises()
    expect(wrapper.find('button').attributes('disabled')).toBeDefined()
  })

  it('页面说明包含「需保持开源记账运行」（显示名随 ADR-0076 更新）', async () => {
    const wrapper = mount(AiPromptView)
    await flushPromises()
    expect(wrapper.text()).toContain('需保持开源记账运行')
  })

  it('获取失败时展示错误提示', async () => {
    wireInvokeSeam({ overrides: { get_ai_prompt: () => Promise.reject(new Error('boom')) } })
    const wrapper = mount(AiPromptView)
    await flushPromises()
    expect(wrapper.find('[data-testid="prompt-body"]').text()).toContain('获取失败')
  })
})
