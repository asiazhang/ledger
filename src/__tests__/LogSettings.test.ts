import { describe, it, expect } from 'vitest'
import { mockInvoke } from '@ledger/test-support/invoke-mock'
import { messageApi } from '@ledger/test-support/message-mock'
import { findButton } from '@ledger/test-support/dom'
import { mount, flushPromises } from '@vue/test-utils'
import { NSelect } from 'naive-ui'
import LogSettings from '@/components/settings/LogSettings.vue'

// 日志卡片（issue #930）：自 AboutSettings 迁入「通用」Tab，IPC 契约与文案逐字不变
// （get_log_level / set_log_level / open_log_dir，spec #608 / #611）。

describe('LogSettings.vue — 日志等级下拉（spec #611）', () => {
  async function mountWithLogLevel(level: string) {
    // 挂载即 onMounted 拉取持久化档位（get_log_level）
    mockInvoke.mockResolvedValueOnce({ level })
    const wrapper = mount(LogSettings)
    await flushPromises()
    return wrapper
  }

  it('挂载后读取持久化档位并回显到下拉', async () => {
    const wrapper = await mountWithLogLevel('debug')
    expect(mockInvoke).toHaveBeenCalledWith('get_log_level')
    expect(wrapper.findComponent(NSelect).props('value')).toBe('debug')
  })

  it('渲染日志等级标签、下拉与静态提示（含 RUST_LOG 说明）', async () => {
    const wrapper = await mountWithLogLevel('info')
    expect(wrapper.findComponent(NSelect).exists()).toBe(true)
    expect(wrapper.text()).toContain('日志等级')
    // 静态提示说明 RUST_LOG 本次启动内优先（spec #608 接缝 3 / AC5）
    expect(wrapper.text()).toContain('RUST_LOG')
  })

  it('改动下拉触发 set_log_level 并回写当前档位', async () => {
    const wrapper = await mountWithLogLevel('info')
    mockInvoke.mockResolvedValueOnce(undefined)
    wrapper.findComponent(NSelect).vm.$emit('update:value', 'warn')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('set_log_level', { level: 'warn' })
    expect(wrapper.findComponent(NSelect).props('value')).toBe('warn')
  })

  it('改动失败时回显保持原档位并提示', async () => {
    const wrapper = await mountWithLogLevel('info')
    mockInvoke.mockRejectedValueOnce('设置日志等级失败：后端错误')
    wrapper.findComponent(NSelect).vm.$emit('update:value', 'trace')
    await flushPromises()
    expect(wrapper.findComponent(NSelect).props('value')).toBe('info')
    expect(messageApi.error).toHaveBeenCalledTimes(1)
    expect(messageApi.error.mock.calls[0][0]).toContain('设置日志等级失败')
  })
})

describe('LogSettings.vue — 打开日志目录（issue #283）', () => {
  function findOpenLogButton(wrapper: ReturnType<typeof mount>) {
    const btn = findButton(wrapper, '打开日志目录', { exact: true })
    expect(btn, '组件应渲染「打开日志目录」按钮').toBeTruthy()
    return btn!
  }

  it('成功路径：点击按钮以新命令名 open_log_dir 调用 IPC 一次，无错误提示', async () => {
    // 挂载时 get_log_level 成功（不触发错误提示），清空计数后再统计 open_log_dir
    mockInvoke.mockResolvedValue({ level: 'info' })
    const wrapper = mount(LogSettings)
    await flushPromises()
    mockInvoke.mockClear()
    mockInvoke.mockResolvedValue(undefined)
    await findOpenLogButton(wrapper).trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledTimes(1)
    expect(mockInvoke).toHaveBeenCalledWith('open_log_dir')
    expect(messageApi.error).not.toHaveBeenCalled()
  })

  it('失败路径：错误提示原样透传后端中文错误，前缀不双层', async () => {
    const backendError = '打开日志目录失败：权限不足'
    // 挂载时 get_log_level 成功（避免加载失败提前触发错误提示），再仅令 open_log_dir 失败
    mockInvoke.mockResolvedValue({ level: 'info' })
    const wrapper = mount(LogSettings)
    await flushPromises()
    mockInvoke.mockClear()
    mockInvoke.mockRejectedValue(backendError)
    await findOpenLogButton(wrapper).trigger('click')
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalledTimes(1)
    expect(messageApi.error).toHaveBeenCalledWith(backendError)
  })
})
