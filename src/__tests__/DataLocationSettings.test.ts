import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { invoke } from '@tauri-apps/api/core'
import type { DataLocationChangeOutcome, DataLocationInfo } from '@/types'

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
  save: vi.fn(),
  confirm: vi.fn(),
}))

// 覆写 setup.ts 的 useMessage mock：改用稳定实例以便断言反馈分支
// （spec：成功→生效提示、校验失败→错误反馈）。
const messageApi = vi.hoisted(() => ({
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
  info: vi.fn(),
  loading: vi.fn(),
  destroyAll: vi.fn(),
}))
vi.mock('naive-ui', async (importOriginal) => {
  const actual = await importOriginal<typeof import('naive-ui')>()
  return { ...actual, useMessage: () => messageApi }
})

import { open, confirm } from '@tauri-apps/plugin-dialog'
import DataLocationSettings from '@/components/settings/DataLocationSettings.vue'

const mockInvoke = vi.mocked(invoke)
const mockOpen = vi.mocked(open)
const mockConfirm = vi.mocked(confirm)

const baseInfo: DataLocationInfo = {
  active_dir: '/Users/me/Library/Application Support/ledger',
  configured_dir: null,
  pending_restart: false,
  fallback_reason: null,
}

/** mock-invoke 桩：默认返回 baseInfo，测试用 overrides 覆写差异项。 */
function stubInvoke(overrides: Record<string, (args?: any) => unknown> = {}) {
  mockInvoke.mockImplementation((cmd: string, args?: any) => {
    if (cmd in overrides) return overrides[cmd](args)
    if (cmd === 'get_data_location_info') return Promise.resolve(baseInfo)
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

/** 让下一次 get_data_location_info 返回指定信息（模拟提交意图后刷新）。 */
function nextInfo(info: DataLocationInfo) {
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'get_data_location_info') return Promise.resolve(info)
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

beforeEach(() => {
  mockInvoke.mockReset()
  mockOpen.mockReset()
  mockConfirm.mockReset()
  messageApi.success.mockClear()
  messageApi.warning.mockClear()
  messageApi.error.mockClear()
  messageApi.info.mockClear()
})

function findButton(wrapper: ReturnType<typeof mount>, text: string) {
  return wrapper.findAll('button').find((b) => b.text().includes(text))!
}

describe('DataLocationSettings.vue', () => {
  it('正常状态：展示当前生效的完整路径，无待重启提示、无回退警示', async () => {
    stubInvoke()
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('get_data_location_info')
    const html = wrapper.html()
    expect(html).toContain('/Users/me/Library/Application Support/ledger')
    expect(html).not.toContain('待重启生效')
    expect(html).not.toContain('已回退')
  })

  it('已更改待重启生效：给出明确的下次启动生效提示并展示意图目录', async () => {
    stubInvoke({
      get_data_location_info: () => ({
        ...baseInfo,
        active_dir: '/Users/me/Library/Application Support/ledger',
        configured_dir: '/Volumes/Sync/ledger-data',
        pending_restart: true,
      }),
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    const html = wrapper.html()
    expect(html).toContain('/Volumes/Sync/ledger-data')
    expect(html).toContain('待重启生效')
  })

  it('存在回退警示：显著提示已回退到默认位置且原库未动', async () => {
    stubInvoke({
      get_data_location_info: () => ({
        ...baseInfo,
        fallback_reason: '配置位置无法打开：权限不足',
      }),
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    const html = wrapper.html()
    expect(html).toContain('已回退')
    expect(html).toContain('原库仍在原地未动')
    expect(html).toContain('权限不足')
  })

  it('更改按钮触发目录选择并以 adoptExisting=false 提交，成功后刷新展示', async () => {
    stubInvoke()
    const committed: DataLocationChangeOutcome = {
      requires_choice: false,
      committed: true,
      target_dir: '/Volumes/Sync/ledger-data',
    }
    mockOpen.mockResolvedValue('/Volumes/Sync/ledger-data')
    let called = 0
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'get_data_location_info') {
        called += 1
        return Promise.resolve(called > 1 ? { ...baseInfo, pending_restart: true, configured_dir: '/Volumes/Sync/ledger-data' } : baseInfo)
      }
      if (cmd === 'submit_data_location_change') return Promise.resolve(committed)
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButton(wrapper, '更改').trigger('click')
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

  it('目标已有同名库：先弹二选一确认，确认接管后以 adoptExisting=true 二次提交', async () => {
    stubInvoke()
    const choice: DataLocationChangeOutcome = { requires_choice: true, committed: false, target_dir: null }
    const committed: DataLocationChangeOutcome = {
      requires_choice: false,
      committed: true,
      target_dir: '/Volumes/Sync/ledger-data',
    }
    mockOpen.mockResolvedValue('/Volumes/Sync/ledger-data')
    mockConfirm.mockResolvedValue(true)
    let submits = 0
    mockInvoke.mockImplementation((cmd: string, args?: any) => {
      if (cmd === 'get_data_location_info') return Promise.resolve(baseInfo)
      if (cmd === 'submit_data_location_change') {
        submits += 1
        return Promise.resolve(submits === 1 ? choice : committed)
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButton(wrapper, '更改').trigger('click')
    await flushPromises()
    expect(mockConfirm).toHaveBeenCalledWith(
      expect.any(String),
      expect.objectContaining({ okLabel: '接管该库', cancelLabel: '取消换位' }),
    )
    expect(mockInvoke).toHaveBeenNthCalledWith(2, 'submit_data_location_change', {
      targetDir: '/Volumes/Sync/ledger-data',
      adoptExisting: false,
    })
    expect(mockInvoke).toHaveBeenNthCalledWith(3, 'submit_data_location_change', {
      targetDir: '/Volumes/Sync/ledger-data',
      adoptExisting: true,
    })
  })

  it('二选一取消：不再提交，状态保持不变', async () => {
    stubInvoke()
    const choice: DataLocationChangeOutcome = { requires_choice: true, committed: false, target_dir: null }
    mockOpen.mockResolvedValue('/Volumes/Sync/ledger-data')
    mockConfirm.mockResolvedValue(false)
    let submits = 0
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'get_data_location_info') return Promise.resolve(baseInfo)
      if (cmd === 'submit_data_location_change') {
        submits += 1
        return Promise.resolve(choice)
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButton(wrapper, '更改').trigger('click')
    await flushPromises()
    expect(submits).toBe(1)
    expect(mockConfirm).toHaveBeenCalled()
    expect(messageApi.info).toHaveBeenCalled()
    expect(wrapper.html()).not.toContain('待重启生效')
  })

  it('目录选择被取消时不提交任何更改', async () => {
    stubInvoke()
    mockOpen.mockResolvedValue(null)
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButton(wrapper, '更改').trigger('click')
    await flushPromises()
    expect(mockInvoke).not.toHaveBeenCalledWith(
      'submit_data_location_change',
      expect.anything(),
    )
  })

  it('校验失败（命令层拒绝）：错误反馈，不崩溃、保持当前状态', async () => {
    stubInvoke({
      get_data_location_info: () => Promise.resolve(baseInfo),
      submit_data_location_change: () =>
        Promise.reject(new Error('目标目录不可写（/Volumes/RO）：只读文件系统')),
    })
    mockOpen.mockResolvedValue('/Volumes/RO')
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButton(wrapper, '更改').trigger('click')
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalled()
    // 状态未被假装成已提交：仍然没有待重启提示
    expect(wrapper.html()).not.toContain('待重启生效')
    expect(wrapper.html()).toContain('/Users/me/Library/Application Support/ledger')
  })

  it('未配置自定义位置时「恢复默认」不生效；已配置时走同一路径可用', async () => {
    stubInvoke()
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    // 未配置意图目录 ⇔ 处于默认位置：点击不产生任何命令调用。
    await findButton(wrapper, '恢复默认').trigger('click')
    await flushPromises()
    expect(mockInvoke).not.toHaveBeenCalledWith(
      'restore_default_data_location',
      expect.anything(),
    )

    // 已配置自定义位置后按钮可用，走与更改相同的提交流。
    mockOpen.mockResolvedValue('/Volumes/Sync/ledger-data')
    nextInfo({ ...baseInfo, configured_dir: '/Volumes/Sync/ledger-data' })
    await findButton(wrapper, '更改').trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('submit_data_location_change',
      expect.objectContaining({ targetDir: '/Volumes/Sync/ledger-data' }),
    )
  })

  it('信息读取失败：错误文案诚实呈现，不用「读取中…」假装正常', async () => {
    stubInvoke({
      get_data_location_info: () => Promise.reject(new Error('IPC 失败')),
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
    mockConfirm.mockResolvedValue(true)
    let submits = 0
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'get_data_location_info') return Promise.resolve(current)
      if (cmd === 'restore_default_data_location') {
        submits += 1
        return Promise.resolve(submits === 1 ? choice : committed)
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButton(wrapper, '恢复默认').trigger('click')
    await flushPromises()
    expect(mockConfirm).toHaveBeenCalled()
    expect(mockInvoke).toHaveBeenNthCalledWith(2, 'restore_default_data_location', {
      adoptExisting: false,
    })
    expect(mockInvoke).toHaveBeenNthCalledWith(3, 'restore_default_data_location', {
      adoptExisting: true,
    })
  })

  it('提交成功后展示值来自刷新的命令返回（不做前端持久化）', async () => {
    stubInvoke()
    const committed: DataLocationChangeOutcome = {
      requires_choice: false,
      committed: true,
      target_dir: '/Volumes/Sync/ledger-data',
    }
    mockOpen.mockResolvedValue('/Volumes/Sync/ledger-data')
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'get_data_location_info') return Promise.resolve(baseInfo)
      if (cmd === 'submit_data_location_change') {
        // 提交后下一次查询返回新意图：展示值必须来自命令返回而非本地推断。
        mockInvoke.mockImplementation((cmd2: string) => {
          if (cmd2 === 'get_data_location_info') {
            return Promise.resolve({
              ...baseInfo,
              configured_dir: '/Volumes/Sync/ledger-data',
              pending_restart: true,
            })
          }
          return Promise.reject(new Error(`unexpected invoke: ${cmd2}`))
        })
        return Promise.resolve(committed)
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = mount(DataLocationSettings)
    await flushPromises()
    await findButton(wrapper, '更改').trigger('click')
    await flushPromises()
    expect(wrapper.html()).toContain('/Volumes/Sync/ledger-data')
    expect(messageApi.success).toHaveBeenCalled()
    expect(localStorage.getItem('data_location_dir')).toBeNull()
  })
})
