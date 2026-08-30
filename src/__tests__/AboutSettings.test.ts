import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { invoke } from '@tauri-apps/api/core'
import AboutSettings from '@/components/settings/AboutSettings.vue'

const writeText = vi.fn().mockResolvedValue(undefined)
const mockInvoke = vi.mocked(invoke)

// 覆写 setup.ts 的 useMessage mock：改用稳定实例以便断言反馈分支
// （issue #283：成功→无提示、失败→原样透传后端中文错误）。
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

// 固定值注入全局常量：生产环境由 vite define 注入，测试用 stubGlobal 等价注入
const FULL_SHA = 'a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0'

function mountWithGit(opts: { sha?: string; dirty?: boolean } = {}) {
  vi.stubGlobal('__GIT_SHA__', opts.sha ?? FULL_SHA)
  vi.stubGlobal('__GIT_DIRTY__', opts.dirty ?? false)
  return mount(AboutSettings)
}

beforeEach(() => {
  writeText.mockClear()
  Object.assign(navigator, { clipboard: { writeText } })
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('AboutSettings.vue — Git 版本行', () => {
  it('显示短 sha（完整 sha 前 7 位）', () => {
    const wrapper = mountWithGit()
    const row = wrapper.find('[data-testid="git-version"]')
    expect(row.exists()).toBe(true)
    expect(row.text()).toContain('Git 版本')
    expect(row.text()).toContain(FULL_SHA.slice(0, 7))
  })

  it('脏树追加 -dirty 后缀', () => {
    const wrapper = mountWithGit({ dirty: true })
    const row = wrapper.find('[data-testid="git-version"]')
    expect(row.text()).toContain(`${FULL_SHA.slice(0, 7)}-dirty`)
  })

  it('干净树不出现 -dirty 后缀', () => {
    const wrapper = mountWithGit({ dirty: false })
    expect(wrapper.find('[data-testid="git-version"]').text()).not.toContain('-dirty')
  })

  it('sha 为空（无法读取 Git 信息）时不显示 Git 版本行', () => {
    const wrapper = mountWithGit({ sha: '' })
    expect(wrapper.find('[data-testid="git-version"]').exists()).toBe(false)
  })

  it('点击 Git 版本行复制完整 40 位 sha', async () => {
    const wrapper = mountWithGit({ dirty: true })
    await wrapper.find('[data-testid="git-version"]').trigger('click')
    expect(writeText).toHaveBeenCalledTimes(1)
    expect(writeText).toHaveBeenCalledWith(FULL_SHA)
    expect(writeText).toHaveBeenCalledWith(expect.not.stringContaining('-dirty'))
  })
})

describe('AboutSettings.vue — 打开日志目录（issue #283）', () => {
  beforeEach(() => {
    mockInvoke.mockReset()
    messageApi.error.mockClear()
  })

  function findOpenLogButton(wrapper: ReturnType<typeof mount>) {
    const btn = wrapper.findAll('button').find((b) => b.text() === '打开日志目录')
    expect(btn, '组件应渲染「打开日志目录」按钮').toBeTruthy()
    return btn!
  }

  it('成功路径：点击按钮以新命令名 open_log_dir 调用 IPC 一次，无错误提示', async () => {
    mockInvoke.mockResolvedValue(undefined)
    const wrapper = mountWithGit()
    await findOpenLogButton(wrapper).trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledTimes(1)
    expect(mockInvoke).toHaveBeenCalledWith('open_log_dir')
    expect(messageApi.error).not.toHaveBeenCalled()
  })

  it('失败路径：错误提示原样透传后端中文错误，前缀不双层', async () => {
    const backendError = '打开日志目录失败：权限不足'
    mockInvoke.mockRejectedValue(backendError)
    const wrapper = mountWithGit()
    await findOpenLogButton(wrapper).trigger('click')
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalledTimes(1)
    expect(messageApi.error).toHaveBeenCalledWith(backendError)
  })
})
