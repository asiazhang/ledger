import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { messageApi } from '@ledger/test-support/message-mock'
import { findButton, findButtonByTestId, findBodyButtonByTestId } from '@ledger/test-support/dom'
import { mount, flushPromises } from '@vue/test-utils'
import type { DataLocationChangeOutcome, DataLocationInfo } from '@ledger/types'

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
  save: vi.fn(),
}))

import { open } from '@tauri-apps/plugin-dialog'
import DataLocationSettings from '@/components/settings/DataLocationSettings.vue'
import AppDangerConfirmModal from '@/components/AppDangerConfirmModal.vue'

const mockOpen = vi.mocked(open)

// 剧本剪贴板（issue #653）：断言写入内容与成功/失败提示分支（父 spec 测试决策：
// 组件测试中 mock 剪贴板对象，断言写入内容与成功提示）。
const writeText = vi.fn().mockResolvedValue(undefined)

const baseInfo: DataLocationInfo = {
  active_dir: '/Users/me/Library/Application Support/ledger',
  configured_dir: null,
  pending_restart: false,
  fallback_reason: null,
}

beforeEach(() => {
  mockOpen.mockReset()
  writeText.mockClear()
  Object.assign(navigator, { clipboard: { writeText } })
})

describe('DataLocationSettings.vue', () => {
  it('正常状态：展示当前生效的完整路径，无待重启提示、无回退警示', async () => {
    wireInvokeSeam({ defaults: { get_data_location_info: baseInfo } })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('get_data_location_info')
    const html = wrapper.html()
    expect(html).toContain('/Users/me/Library/Application Support/ledger')
    expect(html).not.toContain('待重启生效')
    expect(html).not.toContain('已回退')
  })

  it('已更改待重启生效：给出明确的下次启动生效提示并展示意图目录', async () => {
    wireInvokeSeam({
      overrides: {
        get_data_location_info: () => ({
          ...baseInfo,
          active_dir: '/Users/me/Library/Application Support/ledger',
          configured_dir: '/Volumes/Sync/ledger-data',
          pending_restart: true,
        }),
      },
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    const html = wrapper.html()
    expect(html).toContain('/Volumes/Sync/ledger-data')
    expect(html).toContain('待重启生效')
  })

  it('存在回退警示：显著提示已回退到默认位置且原库未动', async () => {
    wireInvokeSeam({
      overrides: {
        get_data_location_info: () => ({
          ...baseInfo,
          fallback_reason: '配置位置无法打开：权限不足',
        }),
      },
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    const html = wrapper.html()
    expect(html).toContain('已回退')
    expect(html).toContain('原库仍在原地未动')
    expect(html).toContain('权限不足')
  })

  it('更改按钮触发目录选择并以 adoptExisting=false 提交，成功后刷新展示', async () => {
    const committed: DataLocationChangeOutcome = {
      requires_choice: false,
      committed: true,
      target_dir: '/Volumes/Sync/ledger-data',
    }
    mockOpen.mockResolvedValue('/Volumes/Sync/ledger-data')
    let called = 0
    wireInvokeSeam({
      defaults: { submit_data_location_change: committed },
      overrides: {
        get_data_location_info: () => {
          called += 1
          return called > 1
            ? { ...baseInfo, pending_restart: true, configured_dir: '/Volumes/Sync/ledger-data' }
            : baseInfo
        },
      },
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButton(wrapper, '更改')!.trigger('click')
    await flushPromises()
    expect(mockOpen).toHaveBeenCalledWith(
      expect.objectContaining({ directory: true, multiple: false }),
    )
    expect(mockInvoke).toHaveBeenCalledWith('submit_data_location_change', {
      targetDir: '/Volumes/Sync/ledger-data',
      adoptExisting: false,
    })
    expect(wrapper.html()).toContain('下次启动')
  })

  it('目标已有同名库：先弹 warning 级二选一弹窗（按钮语义显式），接管后以 adoptExisting=true 二次提交', async () => {
    const choice: DataLocationChangeOutcome = { requires_choice: true, committed: false, target_dir: null }
    const committed: DataLocationChangeOutcome = {
      requires_choice: false,
      committed: true,
      target_dir: '/Volumes/Sync/ledger-data',
    }
    mockOpen.mockResolvedValue('/Volumes/Sync/ledger-data')
    let submits = 0
    wireInvokeSeam({
      defaults: { get_data_location_info: baseInfo },
      overrides: {
        submit_data_location_change: () => {
          submits += 1
          return Promise.resolve(submits === 1 ? choice : committed)
        },
      },
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButton(wrapper, '更改')!.trigger('click')
    await flushPromises()

    // 二选一确认弹窗（issue #652 / ADR-0078）：warning 级，按钮文案即语义
    expect(findBodyButtonByTestId('danger-confirm')!.text()).toContain('接管该库')
    expect(findBodyButtonByTestId('danger-confirm')!.classes()).toContain('n-button--warning-type')
    expect(findBodyButtonByTestId('danger-cancel')!.text()).toContain('取消换位')
    expect(document.body.textContent).toContain('原位置库文件仍会保留')
    expect(mockInvoke).toHaveBeenNthCalledWith(2, 'submit_data_location_change', {
      targetDir: '/Volumes/Sync/ledger-data',
      adoptExisting: false,
    })

    await findBodyButtonByTestId('danger-confirm')!.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenNthCalledWith(3, 'submit_data_location_change', {
      targetDir: '/Volumes/Sync/ledger-data',
      adoptExisting: true,
    })
    expect(messageApi.success).toHaveBeenCalled()
    void wrapper
  })

  it('二选一取消「取消换位」：不再提交，状态保持不变', async () => {
    const choice: DataLocationChangeOutcome = { requires_choice: true, committed: false, target_dir: null }
    mockOpen.mockResolvedValue('/Volumes/Sync/ledger-data')
    let submits = 0
    wireInvokeSeam({
      defaults: { get_data_location_info: baseInfo },
      overrides: {
        submit_data_location_change: () => {
          submits += 1
          return Promise.resolve(choice)
        },
      },
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButton(wrapper, '更改')!.trigger('click')
    await flushPromises()
    await findBodyButtonByTestId('danger-cancel')!.trigger('click')
    await flushPromises()
    expect(submits).toBe(1)
    expect(messageApi.info).toHaveBeenCalled()
    expect(wrapper.html()).not.toContain('待重启生效')
  })

  it('✕/ESC 关闭弹窗同归取消路径：清挂起二次提交，零提交零误接管', async () => {
    const choice: DataLocationChangeOutcome = { requires_choice: true, committed: false, target_dir: null }
    mockOpen.mockResolvedValue('/Volumes/Sync/ledger-data')
    let submits = 0
    wireInvokeSeam({
      defaults: { get_data_location_info: baseInfo },
      overrides: {
        submit_data_location_change: () => {
          submits += 1
          return Promise.resolve(choice)
        },
      },
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButton(wrapper, '更改')!.trigger('click')
    await flushPromises()
    // 模拟 ESC/✕ 关闭（AppModal 关闭意图 → update:show(false)）
    wrapper.findComponent(AppDangerConfirmModal).vm.$emit('update:show', false)
    await flushPromises()
    expect(submits).toBe(1)
    expect(messageApi.info).toHaveBeenCalled()

    // 悬挂已清：下次更改重新走完整流程，不误续接上一次的 adoptExisting=true
    await findButton(wrapper, '更改')!.trigger('click')
    await flushPromises()
    expect(submits).toBe(2)
    expect(wrapper.findComponent(AppDangerConfirmModal).props('show')).toBe(true)
  })

  it('目录选择被取消时不提交任何更改', async () => {
    wireInvokeSeam({ defaults: { get_data_location_info: baseInfo } })
    mockOpen.mockResolvedValue(null)
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButton(wrapper, '更改')!.trigger('click')
    await flushPromises()
    expect(mockInvoke).not.toHaveBeenCalledWith(
      'submit_data_location_change',
      expect.anything(),
    )
  })

  it('校验失败（命令层拒绝）：错误反馈，不崩溃、保持当前状态', async () => {
    wireInvokeSeam({
      defaults: { get_data_location_info: baseInfo },
      overrides: {
        submit_data_location_change: () =>
          Promise.reject(new Error('目标目录不可写（/Volumes/RO）：只读文件系统')),
      },
    })
    mockOpen.mockResolvedValue('/Volumes/RO')
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButton(wrapper, '更改')!.trigger('click')
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalled()
    // 状态未被假装成已提交：仍然没有待重启提示
    expect(wrapper.html()).not.toContain('待重启生效')
    expect(wrapper.html()).toContain('/Users/me/Library/Application Support/ledger')
  })

  it('未配置自定义位置时「恢复默认」不生效；已配置时走同一路径可用', async () => {
    wireInvokeSeam({ defaults: { get_data_location_info: baseInfo } })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    // 未配置意图目录 ⇔ 处于默认位置：点击不产生任何命令调用。
    await findButton(wrapper, '恢复默认')!.trigger('click')
    await flushPromises()
    expect(mockInvoke).not.toHaveBeenCalledWith(
      'restore_default_data_location',
      expect.anything(),
    )

    // 已配置自定义位置后按钮可用，走与更改相同的提交流。
    mockOpen.mockResolvedValue('/Volumes/Sync/ledger-data')
    wireInvokeSeam({
      defaults: { get_data_location_info: { ...baseInfo, configured_dir: '/Volumes/Sync/ledger-data' } },
    })
    await findButton(wrapper, '更改')!.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('submit_data_location_change',
      expect.objectContaining({ targetDir: '/Volumes/Sync/ledger-data' }),
    )
  })

  it('信息读取失败：错误文案诚实呈现，不用「读取中…」假装正常', async () => {
    wireInvokeSeam({
      overrides: { get_data_location_info: () => Promise.reject(new Error('IPC 失败')) },
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    expect(wrapper.html()).toContain('读取数据存储位置失败')
    expect(wrapper.text()).not.toContain('读取中…')
  })

  it('恢复默认走同一路径：restore_default_data_location + 二选一确认复用', async () => {
    // 已配置自定义位置（configured_dir 非空）时「恢复默认」才可用。
    const current: DataLocationInfo = { ...baseInfo, configured_dir: '/Volumes/Sync/ledger-data' }
    const choice: DataLocationChangeOutcome = { requires_choice: true, committed: false, target_dir: null }
    const committed: DataLocationChangeOutcome = {
      requires_choice: false,
      committed: true,
      target_dir: '/Users/me/Library/Application Support/ledger',
    }
    let submits = 0
    wireInvokeSeam({
      defaults: { get_data_location_info: current },
      overrides: {
        restore_default_data_location: () => {
          submits += 1
          return Promise.resolve(submits === 1 ? choice : committed)
        },
      },
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButton(wrapper, '恢复默认')!.trigger('click')
    await flushPromises()
    // 目标已有同名库 → 二选一弹窗（与应用内确认同型）确认后续接
    await findBodyButtonByTestId('danger-confirm')!.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenNthCalledWith(2, 'restore_default_data_location', {
      adoptExisting: false,
    })
    expect(mockInvoke).toHaveBeenNthCalledWith(3, 'restore_default_data_location', {
      adoptExisting: true,
    })
  })

  it('提交成功后展示值来自刷新的命令返回（不做前端持久化）', async () => {
    const committed: DataLocationChangeOutcome = {
      requires_choice: false,
      committed: true,
      target_dir: '/Volumes/Sync/ledger-data',
    }
    mockOpen.mockResolvedValue('/Volumes/Sync/ledger-data')
    wireInvokeSeam({
      defaults: { get_data_location_info: baseInfo },
      overrides: {
        submit_data_location_change: () => {
          // 提交后下一次查询返回新意图：展示值必须来自命令返回而非本地推断。
          wireInvokeSeam({
            defaults: {
              get_data_location_info: {
                ...baseInfo,
                configured_dir: '/Volumes/Sync/ledger-data',
                pending_restart: true,
              },
            },
          })
          return Promise.resolve(committed)
        },
      },
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButton(wrapper, '更改')!.trigger('click')
    await flushPromises()
    expect(wrapper.html()).toContain('/Volumes/Sync/ledger-data')
    expect(messageApi.success).toHaveBeenCalled()
    expect(localStorage.getItem('data_location_dir')).toBeNull()
  })
})

describe('复制路径通道（issue #653）：界面文本不可选，复制走显式按钮 + 剪贴板 API', () => {
  it('当前生效位置：点击「复制路径」写入完整路径并成功提示', async () => {
    wireInvokeSeam({ defaults: { get_data_location_info: baseInfo } })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButtonByTestId(wrapper, 'copy-active-path').trigger('click')
    await flushPromises()
    expect(writeText).toHaveBeenCalledTimes(1)
    expect(writeText).toHaveBeenCalledWith('/Users/me/Library/Application Support/ledger')
    expect(messageApi.success).toHaveBeenCalledWith(expect.stringContaining('已复制完整路径'))
    expect(messageApi.error).not.toHaveBeenCalled()
  })

  it('待生效新位置：待重启生效时同样可复制意图目录完整路径（与当前生效位置两个入口并存）', async () => {
    wireInvokeSeam({
      overrides: {
        get_data_location_info: () => ({
          ...baseInfo,
          configured_dir: '/Volumes/Sync/ledger-data',
          pending_restart: true,
        }),
      },
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    expect(findButtonByTestId(wrapper, 'copy-active-path').exists()).toBe(true)
    await findButtonByTestId(wrapper, 'copy-pending-path').trigger('click')
    await flushPromises()
    expect(writeText).toHaveBeenCalledTimes(1)
    expect(writeText).toHaveBeenCalledWith('/Volumes/Sync/ledger-data')
    expect(messageApi.success).toHaveBeenCalled()
  })

  it('复制失败：错误提示，不静默', async () => {
    wireInvokeSeam({ defaults: { get_data_location_info: baseInfo } })
    writeText.mockRejectedValueOnce(new Error('剪贴板不可用'))
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButtonByTestId(wrapper, 'copy-active-path').trigger('click')
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalledWith(expect.stringContaining('复制路径失败'))
    expect(messageApi.success).not.toHaveBeenCalled()
  })
})
