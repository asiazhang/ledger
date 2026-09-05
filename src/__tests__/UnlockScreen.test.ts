import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { invoke } from '@tauri-apps/api/core'
import { setActivePinia, createPinia } from 'pinia'

import UnlockScreen from '@/components/UnlockScreen.vue'
import { useEncryptionGate } from '@/composables/useEncryptionGate'
import { useAppStore } from '@/stores/app'

const mockInvoke = vi.mocked(invoke)

/** mock-invoke 桩：解锁屏只消费加密命令面（fail-loud：其余命令一律拒绝）。 */
function stubInvoke(overrides: Record<string, (args?: any) => unknown> = {}) {
  mockInvoke.mockImplementation((cmd: string, args?: any) => {
    if (cmd in overrides) return overrides[cmd](args)
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

beforeEach(() => {
  mockInvoke.mockReset()
  setActivePinia(createPinia())
  // 每个用例从「未探测」起步（模块级单例状态复位），并清空记住偏好的 localStorage。
  const gate = useEncryptionGate()
  gate.locked.value = null
  gate.bootFailed.value = false
  gate.rememberSupport.value = null
  localStorage.removeItem('remember_passphrase')
  document.body.innerHTML = ''
})

function findButton(wrapper: ReturnType<typeof mount>, text: string) {
  return wrapper.findAll('button').find((b) => b.text().includes(text))!
}

/** 重置确认弹窗（teleport 到 body）内按 data-testid 找按钮。 */
function bodyButton(testid: string): HTMLButtonElement {
  const btn = document.body.querySelector(`[data-testid="${testid}"]`) as HTMLButtonElement | null
  if (!btn) throw new Error(`未找到 testid=${testid} 的按钮`)
  return btn
}

async function mountWithProbe(
  locked: boolean,
  overrides: Record<string, (args?: any) => unknown> = {},
) {
  stubInvoke({
    get_boot_status: () =>
      Promise.resolve({ phase: locked ? ('locked' as const) : ('ready' as const), error_code: null }),
    ...overrides,
  })
  const gate = useEncryptionGate()
  const probePromise = gate.probe()
  const wrapper = mount(UnlockScreen)
  await probePromise
  await flushPromises()
  return { wrapper, locked: gate.locked }
}

describe('UnlockScreen.vue（加密锁定门·解锁屏流程）', () => {
  it('锁定时挂载解锁屏：标题、口令输入与解锁按钮就位', async () => {
    const { wrapper } = await mountWithProbe(true)
    const html = wrapper.html()
    expect(html).toContain('账本已加密')
    expect(html).toContain('主口令')
    expect(findButton(wrapper, '解锁')).toBeTruthy()
    // 不渲染主界面业务面（解锁先于一切业务读写）
    expect(html).not.toContain('仪表盘')
  })

  it('探测失败：按锁定处理（fail-closed），解锁屏仍渲染而非主界面', async () => {
    stubInvoke({
      get_boot_status: () => Promise.reject(new Error('invoke 失败')),
    })
    const { probe, locked } = useEncryptionGate()
    const probePromise = probe()
    const wrapper = mount(UnlockScreen)
    await probePromise
    await flushPromises()
    expect(locked.value).toBe(true)
    expect(wrapper.html()).toContain('账本已加密')
  })

  it('解锁成功：调用 unlock_encryption 携带口令，状态翻转为已解锁', async () => {
    stubInvoke({
      get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
      unlock_encryption: (args: any) => {
        expect(args.passphrase).toBe('口令①')
        return Promise.resolve({ relocated: false })
      },
    })
    const { probe, locked } = useEncryptionGate()
    const probePromise = probe()
    const wrapper = mount(UnlockScreen)
    await probePromise
    await flushPromises()

    const input = wrapper.find('input')
    await input.setValue('口令①')
    await findButton(wrapper, '解锁')!.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('unlock_encryption', { passphrase: '口令①' })
    expect(locked.value).toBe(false)
  })

  it('错误口令：提示重试（码化文案），状态保持锁定可无限重试', async () => {
    stubInvoke({
      get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
      unlock_encryption: () =>
        Promise.reject({
          kind: 'Invalid',
          message: '主口令不正确，请重试',
          code: 'encryption.passphrase-incorrect',
        }),
    })
    const { probe, locked } = useEncryptionGate()
    const probePromise = probe()
    const wrapper = mount(UnlockScreen)
    await probePromise
    await flushPromises()

    const input = wrapper.find('input')
    await input.setValue('错误口令')
    await findButton(wrapper, '解锁')!.trigger('click')
    await flushPromises()
    const html = wrapper.html()
    expect(html).toContain('主口令不正确，请重试')
    // 「口令错误」不是文件损坏：不出现损坏文案
    expect(html).not.toContain('损坏')
    expect(locked.value).toBe(true)

    // 无限重试：再次输入并提交，按钮仍可用
    await input.setValue('再试一次')
    const button = findButton(wrapper, '解锁')!
    expect((button.element as HTMLButtonElement).disabled).toBe(false)
  })

  it('文件损坏与口令错误文案可区分：损坏码透出损坏提示', async () => {
    stubInvoke({
      get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
      unlock_encryption: () =>
        Promise.reject({
          kind: 'Invalid',
          message: '数据库文件损坏，无法通过完整性检查',
          code: 'encryption.db-corrupt',
        }),
    })
    const { probe } = useEncryptionGate()
    const probePromise = probe()
    const wrapper = mount(UnlockScreen)
    await probePromise
    await flushPromises()

    await wrapper.find('input').setValue('口令')
    await findButton(wrapper, '解锁')!.trigger('click')
    await flushPromises()
    expect(wrapper.html()).toContain('数据库文件损坏')
    expect(wrapper.html()).not.toContain('主口令不正确')
  })

  it('解锁时补做了搬迁：成功提示后调用 restart_app（Restore 同型重启语义）', async () => {
    vi.useFakeTimers()
    try {
      stubInvoke({
        get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
        unlock_encryption: () => Promise.resolve({ relocated: true }),
        restart_app: () => Promise.resolve(),
      })
      const { probe } = useEncryptionGate()
      const probePromise = probe()
      const wrapper = mount(UnlockScreen)
      await probePromise
      await flushPromises()

      await wrapper.find('input').setValue('口令')
      await findButton(wrapper, '解锁')!.trigger('click')
      await flushPromises()
      vi.advanceTimersByTime(900)
      await flushPromises()
      expect(mockInvoke).toHaveBeenCalledWith('restart_app')
    } finally {
      vi.useRealTimers()
    }
  })

  it('忘记口令入口常驻：不解锁也能直达逃生门', async () => {
    const { wrapper } = await mountWithProbe(true)
    const forgot = findButton(wrapper, '忘记口令')
    expect(forgot).toBeTruthy()
  })

  it('点击入口先弹 error 级应用内确认弹窗（不再有系统原生对话框），后果说明在场，二次确认才重置', async () => {
    const { wrapper, locked } = await mountWithProbe(true, {
      reset_after_forgotten_passphrase: () => Promise.resolve(),
    })

    await findButton(wrapper, '忘记口令')!.trigger('click')
    await flushPromises()
    // 原生对话框退役（ADR-0078 决策 1，issue #652）：弹窗为应用内 error 级，
    // 后果说明明确不可恢复，且告知密文副本可日后救回（issue #573 语义不回退）
    expect(document.body.textContent).toContain('不可恢复')
    expect(document.body.textContent).toContain('密文副本')
    const alert = document.body.querySelector('.n-modal .n-alert')
    expect(alert, '红色警示块应存在').toBeTruthy()
    expect(alert!.querySelector('.n-text--strong'), '后果句应加粗').toBeTruthy()
    expect(bodyButton('danger-confirm').className).toContain('n-button--error-type')

    bodyButton('danger-confirm').click()
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('reset_after_forgotten_passphrase')
    expect(locked.value).toBe(false)
  })

  it('后果说明后取消：留在解锁屏，不发起重置', async () => {
    const { wrapper, locked } = await mountWithProbe(true, {
      reset_after_forgotten_passphrase: () => Promise.resolve(),
    })

    await findButton(wrapper, '忘记口令')!.trigger('click')
    await flushPromises()
    bodyButton('danger-cancel').click()
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'reset_after_forgotten_passphrase'),
    ).toBe(false)
    expect(locked.value).toBe(true)
    // 解锁屏仍在，可继续尝试口令或再次进入
    expect(wrapper.html()).toContain('账本已加密')
  })

  it('重置失败：错误文案透传，保持锁定可重试', async () => {
    const { wrapper, locked } = await mountWithProbe(true, {
      reset_after_forgotten_passphrase: () =>
        Promise.reject({
          kind: 'Db',
          message: '数据库文件不存在或为空',
          code: 'encryption.db-missing',
        }),
    })

    await findButton(wrapper, '忘记口令')!.trigger('click')
    await flushPromises()
    bodyButton('danger-confirm').click()
    await flushPromises()
    expect(wrapper.html()).toContain('数据库文件不存在或为空')
    expect(locked.value).toBe(true)
  })
})

describe('UnlockScreen.vue 本机记住主口令（issue #574）', () => {
  /** 记住复选项（语义定位按钮文本）。 */
  function findRememberCheckbox(wrapper: ReturnType<typeof mount>) {
    return wrapper.find('.n-checkbox')
  }

  it('记住开启 + 平台支持：挂载即凭缓存自动解锁，成功后翻转为已解锁', async () => {
    stubInvoke({
      get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
      get_remember_passphrase_support: () => Promise.resolve({ supported: true }),
      unlock_with_remembered_passphrase: () => Promise.resolve({ relocated: false }),
    })
    useAppStore().setRememberPassphrase(true)
    const { probe, locked } = useEncryptionGate()
    const probePromise = probe()
    const wrapper = mount(UnlockScreen)
    await probePromise
    await flushPromises()

    // 自动解锁：发起凭缓存解锁（口令不回流前端，只调命令），成功后翻解锁
    expect(mockInvoke).toHaveBeenCalledWith('unlock_with_remembered_passphrase')
    expect(locked.value).toBe(false)
  })

  it('记住开启但平台不支持：不触发自动解锁，回退手输并隐藏记住复选项', async () => {
    stubInvoke({
      get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
      get_remember_passphrase_support: () => Promise.resolve({ supported: false }),
    })
    useAppStore().setRememberPassphrase(true)
    const { probe, locked } = useEncryptionGate()
    const probePromise = probe()
    const wrapper = mount(UnlockScreen)
    await probePromise
    await flushPromises()

    expect(mockInvoke).not.toHaveBeenCalledWith('unlock_with_remembered_passphrase')
    expect(locked.value).toBe(true)
    expect(wrapper.html()).toContain('账本已加密')
    // 平台不支持：隐藏「记住」选项（回退手输）
    expect(wrapper.html()).not.toContain('在本机记住主口令')
  })

  it('自动解锁失败（无缓存）：回退手输，提示本地化且口令输入可交互', async () => {
    stubInvoke({
      get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
      get_remember_passphrase_support: () => Promise.resolve({ supported: true }),
      unlock_with_remembered_passphrase: () =>
        Promise.reject({
          kind: 'Invalid',
          message: '本机没有缓存的主口令，请手动输入',
          code: 'encryption.remember-no-cache',
        }),
    })
    useAppStore().setRememberPassphrase(true)
    const { probe, locked } = useEncryptionGate()
    const probePromise = probe()
    const wrapper = mount(UnlockScreen)
    await probePromise
    await flushPromises()

    expect(locked.value).toBe(true)
    expect(wrapper.html()).toContain('本机没有缓存的主口令')
    // 回退手输：口令输入仍可交互
    expect(wrapper.find('input').exists()).toBe(true)
  })

  it('自动解锁失败（生物认证取消）：回退手输并提示取消', async () => {
    stubInvoke({
      get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
      get_remember_passphrase_support: () => Promise.resolve({ supported: true }),
      unlock_with_remembered_passphrase: () =>
        Promise.reject({
          kind: 'Invalid',
          message: '生物认证已取消，请手动输入主口令',
          code: 'encryption.remember-biometric-cancelled',
        }),
    })
    useAppStore().setRememberPassphrase(true)
    const { probe, locked } = useEncryptionGate()
    const probePromise = probe()
    const wrapper = mount(UnlockScreen)
    await probePromise
    await flushPromises()

    expect(wrapper.html()).toContain('生物认证已取消')
    expect(locked.value).toBe(true)
  })

  it('手动解锁勾选记住：解锁后缓存主口令并置偏好开', async () => {
    stubInvoke({
      get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
      get_remember_passphrase_support: () => Promise.resolve({ supported: true }),
      unlock_encryption: (args: any) => {
        expect(args.passphrase).toBe('口令①')
        return Promise.resolve({ relocated: false })
      },
      set_remember_passphrase: (args: any) => {
        expect(args.passphrase).toBe('口令①')
        return Promise.resolve()
      },
    })
    const { probe, locked } = useEncryptionGate()
    const probePromise = probe()
    const wrapper = mount(UnlockScreen)
    await probePromise
    await flushPromises()

    // 勾选「记住」→ 输入口令 → 解锁
    await findRememberCheckbox(wrapper).trigger('click')
    await wrapper.find('input').setValue('口令①')
    await findButton(wrapper, '解锁')!.trigger('click')
    await flushPromises()

    expect(mockInvoke).toHaveBeenCalledWith('set_remember_passphrase', { passphrase: '口令①' })
    expect(useAppStore().rememberPassphrase).toBe(true)
    expect(locked.value).toBe(false)
  })

  it('手动解锁取消记住：解锁后清缓存并置偏好关', async () => {
    stubInvoke({
      get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
      get_remember_passphrase_support: () => Promise.resolve({ supported: true }),
      unlock_encryption: () => Promise.resolve({ relocated: false }),
      clear_remember_passphrase: () => Promise.resolve(),
    })
    const { probe, locked } = useEncryptionGate()
    const probePromise = probe()
    const wrapper = mount(UnlockScreen)
    await probePromise
    await flushPromises()

    // 不复选（默认关）→ 解锁后清缓存
    await wrapper.find('input').setValue('口令')
    await findButton(wrapper, '解锁')!.trigger('click')
    await flushPromises()

    expect(mockInvoke).toHaveBeenCalledWith('clear_remember_passphrase')
    expect(useAppStore().rememberPassphrase).toBe(false)
    expect(locked.value).toBe(false)
  })
})
