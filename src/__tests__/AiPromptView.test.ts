import { describe, it, expect, vi, beforeEach } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import AiPromptView from '@/views/AiPromptView.vue'

const mockInvoke = vi.mocked(invoke)
const writeText = vi.fn().mockResolvedValue(undefined)

const SAMPLE_PROMPT = `# Ledger API 系统提示词

Ledger 在本地 http://127.0.0.1:9527 提供 HTTP API。

- 先 GET /api/v1/openapi.json 发现全部端点。
- 批量写交易/导入前，先 GET /api/v1/import/knowledge 获取拆行约定。`

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'get_ai_prompt') return Promise.resolve(SAMPLE_PROMPT)
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  Object.assign(navigator, { clipboard: { writeText } })
  writeText.mockClear()
})

describe('AiPromptView.vue', () => {
  it('加载并展示提示词全文', async () => {
    const wrapper = mount(AiPromptView)
    await flushPromises()
    const body = wrapper.find('[data-testid="prompt-body"]')
    expect(body.text()).toContain('# Ledger API 系统提示词')
    expect(body.text()).toContain('/api/v1/import/knowledge')
  })

  it('点击复制按钮调用剪贴板写入', async () => {
    const wrapper = mount(AiPromptView)
    await flushPromises()
    await wrapper.find('button').trigger('click')
    expect(writeText).toHaveBeenCalledWith(SAMPLE_PROMPT)
  })

  it('提示词为空时复制按钮禁用', async () => {
    mockInvoke.mockImplementation(() => Promise.resolve(''))
    const wrapper = mount(AiPromptView)
    await flushPromises()
    expect(wrapper.find('button').attributes('disabled')).toBeDefined()
  })

  it('获取失败时展示错误提示', async () => {
    mockInvoke.mockImplementation(() => Promise.reject(new Error('boom')))
    const wrapper = mount(AiPromptView)
    await flushPromises()
    expect(wrapper.find('[data-testid="prompt-body"]').text()).toContain('获取失败')
  })
})
