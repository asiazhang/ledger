import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { invoke } from '@tauri-apps/api/core'
import type { EncryptionStatus } from '@/types'

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
  save: vi.fn(),
  confirm: vi.fn(),
}))

// 覆写 setup.ts 的 useMessage mock：改用稳定实例以便断言反馈分支。
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

import { confirm } from '@tauri-apps/plugin-dialog'
import EncryptionSettings from '@/components/settings/EncryptionSettings.vue'

const mockInvoke = vi.mocked(invoke)
const mockConfirm = vi.mocked(confirm)

const plaintextStatus: EncryptionStatus = { locked: false, file_encrypted: false }
const encryptedStatus: EncryptionStatus = { locked: false, file_encrypted: true }

function stubInvoke(overrides: Record<string, (args?: any) => unknown> = {}) {
  mockInvoke.mockImplementation((cmd: string, args?: any) => {
    if (cmd in overrides) return overrides[cmd](args)
    if (cmd === 'get_encryption_status') return Promise.resolve(plaintextStatus)
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

beforeEach(() => {
  mockInvoke.mockReset()
  mockConfirm.mockReset()
  messageApi.success.mockClear()
  messageApi.warning.mockClear()
  messageApi.error.mockClear()
  messageApi.info.mockClear()
})

function findButton(wrapper: ReturnType<typeof mount>, text: string) {
  return wrapper.findAll('button').find((b) => b.text().includes(text))!
}

async function setPasswords(wrapper: ReturnType<typeof mount>, pass: string, confirm: string) {
  const inputs = wrapper.findAll('input')
  await inputs[0].setValue(pass)
  await inputs[1].setValue(confirm)
}

describe('EncryptionSettings.vue（设置页加密卡片）', () => {
  it('明文库：展示开启表单与无后门警示，确认一致后可提交', async () => {
    stubInvoke()
    const wrapper = mount(EncryptionSettings)
    await flushPromises()
    const html = wrapper.html()
    expect(html).toContain('开启加密')
    expect(html).toContain('主口令')
    // 无后门后果说明（ADR-0075 决策 2：用户显式知情）
    expect(html).toContain('无法读取数据')

    await setPasswords(wrapper, '口令①', '口令①')
    await flushPromises()
    const button = findButton(wrapper, '开启加密')!
    expect(button.element.disabled).toBe(false)
  })

  it('两次输入不一致：禁用提交并给出即时反馈', async () => {
    stubInvoke()
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await setPasswords(wrapper, '口令①', '口令②')
    await flushPromises()
    const html = wrapper.html()
    expect(html).toContain('两次输入不一致')
    const button = findButton(wrapper, '开启加密')!
    expect(button.element.disabled).toBe(true)
  })

  it('确认后调用 enable_encryption 携带口令，成功提示重启并调用 restart_app', async () => {
    vi.useFakeTimers()
    try {
      stubInvoke({
        enable_encryption: (args: any) => {
          expect(args.passphrase).toBe('口令①')
          return Promise.resolve()
        },
        restart_app: () => Promise.resolve(),
      })
      mockConfirm.mockResolvedValue(true)
      const wrapper = mount(EncryptionSettings)
      await flushPromises()

      await setPasswords(wrapper, '口令①', '口令①')
      await findButton(wrapper, '开启加密')!.trigger('click')
      await flushPromises()
      expect(mockInvoke).toHaveBeenCalledWith('enable_encryption', { passphrase: '口令①' })
      expect(messageApi.success).toHaveBeenCalled()
      vi.advanceTimersByTime(900)
      await flushPromises()
      expect(mockInvoke).toHaveBeenCalledWith('restart_app')
    } finally {
      vi.useRealTimers()
    }
  })

  it('确认弹窗取消：不发起转换', async () => {
    stubInvoke()
    mockConfirm.mockResolvedValue(false)
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await setPasswords(wrapper, '口令①', '口令①')
    await findButton(wrapper, '开启加密')!.trigger('click')
    await flushPromises()
    expect(mockInvoke).not.toHaveBeenCalledWith('enable_encryption', expect.anything())
  })

  it('转换失败：错误反馈（口令错误之外的后端错误透传），应用留在明文状态', async () => {
    stubInvoke({
      enable_encryption: () =>
        Promise.reject({
          kind: 'Invalid',
          message: '主口令不能为空',
          code: 'encryption.passphrase-empty',
        }),
    })
    mockConfirm.mockResolvedValue(true)
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await setPasswords(wrapper, '口令①', '口令①')
    await findButton(wrapper, '开启加密')!.trigger('click')
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalled()
    // 未调用重启（转换失败不重启，应用回到明文可用状态）
    expect(mockInvoke).not.toHaveBeenCalledWith('restart_app')
  })

  it('已加密库：展示已开启状态与修改主口令、关闭加密表单，不再展示开启表单', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()
    const html = wrapper.html()
    expect(html).toContain('已开启加密')
    expect(html).toContain('修改主口令')
    expect(html).toContain('关闭加密')
    expect(findButton(wrapper, '开启加密')).toBeUndefined()
  })

  it('已加密库·修改主口令：新旧不一致禁用提交并给出即时反馈', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    const inputs = wrapper.findAll('input')
    await inputs[0].setValue('旧口令')
    await inputs[1].setValue('新口令①')
    await inputs[2].setValue('新口令②')
    await flushPromises()
    expect(wrapper.html()).toContain('两次输入不一致')
    expect(findButton(wrapper, '修改主口令')!.element.disabled).toBe(true)
  })

  it('已加密库·修改主口令：新口令与当前口令相同禁用提交并给出警示反馈', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    const inputs = wrapper.findAll('input')
    await inputs[0].setValue('同一口令')
    await inputs[1].setValue('同一口令')
    await inputs[2].setValue('同一口令')
    await flushPromises()
    expect(wrapper.html()).toContain('新主口令与当前主口令相同')
    expect(findButton(wrapper, '修改主口令')!.element.disabled).toBe(true)
  })

  it('已加密库·修改主口令：确认后调用 change_encryption_passphrase 携带新旧口令，成功提示重启', async () => {
    vi.useFakeTimers()
    try {
      stubInvoke({
        get_encryption_status: () => Promise.resolve(encryptedStatus),
        change_encryption_passphrase: (args: any) => {
          expect(args.passphrase).toBe('旧口令')
          expect(args.newPassphrase).toBe('新口令①')
          return Promise.resolve()
        },
        restart_app: () => Promise.resolve(),
      })
      mockConfirm.mockResolvedValue(true)
      const wrapper = mount(EncryptionSettings)
      await flushPromises()

      const inputs = wrapper.findAll('input')
      await inputs[0].setValue('旧口令')
      await inputs[1].setValue('新口令①')
      await inputs[2].setValue('新口令①')
      await findButton(wrapper, '修改主口令')!.trigger('click')
      await flushPromises()
      expect(mockInvoke).toHaveBeenCalledWith('change_encryption_passphrase', {
        passphrase: '旧口令',
        newPassphrase: '新口令①',
      })
      expect(messageApi.success).toHaveBeenCalled()
      vi.advanceTimersByTime(900)
      await flushPromises()
      expect(mockInvoke).toHaveBeenCalledWith('restart_app')
    } finally {
      vi.useRealTimers()
    }
  })

  it('已加密库·修改主口令：旧口令错误时错误反馈，不重启（原库原样保留）', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
      change_encryption_passphrase: () =>
        Promise.reject({
          kind: 'Coded',
          message: '主口令不正确，请重试',
          code: 'encryption.passphrase-incorrect',
        }),
    })
    mockConfirm.mockResolvedValue(true)
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    const inputs = wrapper.findAll('input')
    await inputs[0].setValue('错口令')
    await inputs[1].setValue('新口令①')
    await inputs[2].setValue('新口令①')
    await findButton(wrapper, '修改主口令')!.trigger('click')
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalled()
    expect(mockInvoke).not.toHaveBeenCalledWith('restart_app')
  })

  it('已加密库·修改主口令：确认弹窗取消不发起转换', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
    })
    mockConfirm.mockResolvedValue(false)
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    const inputs = wrapper.findAll('input')
    await inputs[0].setValue('旧口令')
    await inputs[1].setValue('新口令①')
    await inputs[2].setValue('新口令①')
    await findButton(wrapper, '修改主口令')!.trigger('click')
    await flushPromises()
    expect(mockInvoke).not.toHaveBeenCalledWith(
      'change_encryption_passphrase',
      expect.anything(),
    )
  })

  it('已加密库·关闭加密：确认后调用 disable_encryption 携带当前口令，成功提示重启', async () => {
    vi.useFakeTimers()
    try {
      stubInvoke({
        get_encryption_status: () => Promise.resolve(encryptedStatus),
        disable_encryption: (args: any) => {
          expect(args.passphrase).toBe('当前口令')
          return Promise.resolve()
        },
        restart_app: () => Promise.resolve(),
      })
      mockConfirm.mockResolvedValue(true)
      const wrapper = mount(EncryptionSettings)
      await flushPromises()

      const inputs = wrapper.findAll('input')
      await inputs[3].setValue('当前口令')
      await findButton(wrapper, '关闭加密')!.trigger('click')
      await flushPromises()
      expect(mockInvoke).toHaveBeenCalledWith('disable_encryption', {
        passphrase: '当前口令',
      })
      expect(messageApi.success).toHaveBeenCalled()
      vi.advanceTimersByTime(900)
      await flushPromises()
      expect(mockInvoke).toHaveBeenCalledWith('restart_app')
    } finally {
      vi.useRealTimers()
    }
  })

  it('已加密库·关闭加密：后端报错（口令错误）时错误反馈，不重启（原库原样保留）', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
      disable_encryption: () =>
        Promise.reject({
          kind: 'Coded',
          message: '主口令不正确，请重试',
          code: 'encryption.passphrase-incorrect',
        }),
    })
    mockConfirm.mockResolvedValue(true)
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    const inputs = wrapper.findAll('input')
    await inputs[3].setValue('错口令')
    await findButton(wrapper, '关闭加密')!.trigger('click')
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalled()
    expect(mockInvoke).not.toHaveBeenCalledWith('restart_app')
  })

  it('已加密库·关闭加密：确认弹窗取消不发起转换', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
    })
    mockConfirm.mockResolvedValue(false)
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    const inputs = wrapper.findAll('input')
    await inputs[3].setValue('当前口令')
    await findButton(wrapper, '关闭加密')!.trigger('click')
    await flushPromises()
    expect(mockInvoke).not.toHaveBeenCalledWith('disable_encryption', expect.anything())
  })
})
