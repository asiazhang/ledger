import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { invoke } from '@tauri-apps/api/core'
import { setActivePinia, createPinia } from 'pinia'

// 文件选择与重启单点 mock（先例 StartupFailureScreen.test.ts；restartAppShortly
// 内含延时，测试断言调用而非计时）。恢复流只需 open；confirm 已随 issue #652
// 退役，不再 mock。
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
}))
const restartAppShortly = vi.fn()
vi.mock('@/utils/restart', () => ({ restartAppShortly: () => restartAppShortly() }))

import { open } from '@tauri-apps/plugin-dialog'
import UnlockScreen from '@/components/UnlockScreen.vue'
import { AUTO_UNLOCK_TIMEOUT_MS, useEncryptionGate } from '@/composables/useEncryptionGate'
import { useAppStore } from '@/stores/app'

const mockInvoke = vi.mocked(invoke)
const mockOpen = vi.mocked(open)

/** mock-invoke 桩：解锁屏只消费加密命令面（fail-loud：其余命令一律拒绝）。 */
function stubInvoke(overrides: Record<string, (args?: any) => unknown> = {}) {
  mockInvoke.mockImplementation((cmd: string, args?: any) => {
    if (cmd in overrides) return overrides[cmd](args)
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

beforeEach(() => {
  mockInvoke.mockReset()
  mockOpen.mockReset()
  restartAppShortly.mockClear()
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

  it('错误口令：提示「口令错误或文件损坏」合并口径（issue #603），状态保持锁定可无限重试', async () => {
    stubInvoke({
      get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
      unlock_encryption: () =>
        Promise.reject({
          kind: 'Invalid',
          message: '口令错误或文件损坏，请重试',
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
    // 合并口径（issue #603 / ADR-0075 决策 5 修订）：错误口令与损坏同报
    // NOTADB、运行期不可区分，提示不误报损坏、可无限重试。
    expect(html).toContain('口令错误或文件损坏，请重试')
    expect(locked.value).toBe(true)

    // 无限重试：再次输入并提交，按钮仍可用
    await input.setValue('再试一次')
    const button = findButton(wrapper, '解锁')!
    expect((button.element as HTMLButtonElement).disabled).toBe(false)
  })

  it('文件损坏码透出专属损坏提示，与合并口径并存（凭口令打开成功但完整性检查失败）', async () => {
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
    // db-corrupt 专属码不走口令失败的合并口径
    expect(wrapper.html()).not.toContain('口令错误或文件损坏')
  })

  it('解锁时补做了搬迁：成功提示后触发应用重启（Restore 同型重启语义）', async () => {
    vi.useFakeTimers()
    try {
      stubInvoke({
        get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
        unlock_encryption: () => Promise.resolve({ relocated: true }),
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
      expect(restartAppShortly).toHaveBeenCalledTimes(1)
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

  it('自动解锁有界等待（issue #644）：超时回退手输并提示，不无限停在加载态', async () => {
    vi.useFakeTimers()
    try {
      stubInvoke({
        get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
        get_remember_passphrase_support: () => Promise.resolve({ supported: true }),
        // 钥匙串读取阻塞：长时间不返回（开发构建受限形态的白屏根因一）。
        unlock_with_remembered_passphrase: () => new Promise(() => {}),
      })
      useAppStore().setRememberPassphrase(true)
      const { probe, locked } = useEncryptionGate()
      const probePromise = probe()
      const wrapper = mount(UnlockScreen)
      await probePromise
      await flushPromises()

      // 到期前：仍处于自动解锁加载态
      expect(wrapper.html()).toContain('正在尝试自动解锁')
      vi.advanceTimersByTime(AUTO_UNLOCK_TIMEOUT_MS)
      await flushPromises()

      // 到期后：回退手输，超时提示可见，口令输入可交互（恢复通道不再被加载态遮蔽）
      expect(locked.value).toBe(true)
      expect(wrapper.html()).toContain('账本已加密')
      expect(wrapper.html()).toContain('自动解锁超时')
      expect(wrapper.find('input').exists()).toBe(true)
      // 逃生门双入口随回退重新可达（恢复通道不受影响）
      expect(findButton(wrapper, '从备份文件恢复')).toBeTruthy()
      expect(findButton(wrapper, '忘记口令')).toBeTruthy()
    } finally {
      vi.useRealTimers()
    }
  })

  it('超时后自动解锁迟到成功：仍翻转锁定门进入应用（等待有界、结果不丢，issue #644）', async () => {
    vi.useFakeTimers()
    try {
      let resolveUnlock!: (v: { relocated: boolean }) => void
      stubInvoke({
        get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
        get_remember_passphrase_support: () => Promise.resolve({ supported: true }),
        unlock_with_remembered_passphrase: () =>
          new Promise((resolve) => {
            resolveUnlock = resolve
          }),
      })
      useAppStore().setRememberPassphrase(true)
      const { probe, locked } = useEncryptionGate()
      const probePromise = probe()
      const wrapper = mount(UnlockScreen)
      await probePromise
      await flushPromises()

      vi.advanceTimersByTime(AUTO_UNLOCK_TIMEOUT_MS)
      await flushPromises()
      expect(locked.value).toBe(true)

      // 迟到成功：生物认证最终通过——结果照常生效，主界面挂载
      resolveUnlock({ relocated: false })
      await flushPromises()
      expect(locked.value).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })

  it('自动解锁补做了搬迁：成功提示后触发应用重启（Restore 同型重启语义）', async () => {
    vi.useFakeTimers()
    try {
      stubInvoke({
        get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
        get_remember_passphrase_support: () => Promise.resolve({ supported: true }),
        unlock_with_remembered_passphrase: () => Promise.resolve({ relocated: true }),
      })
      useAppStore().setRememberPassphrase(true)
      const { probe } = useEncryptionGate()
      const probePromise = probe()
      const wrapper = mount(UnlockScreen)
      await probePromise
      await flushPromises()

      vi.advanceTimersByTime(900)
      await flushPromises()
      expect(restartAppShortly).toHaveBeenCalledTimes(1)
    } finally {
      vi.useRealTimers()
    }
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

describe('UnlockScreen.vue 从备份文件恢复入口（issue #603）', () => {
  /** 入口按钮（data-testid 定位，先例 StartupFailureScreen.test）。 */
  function entryButton(wrapper: ReturnType<typeof mount>) {
    return wrapper.find('[data-testid="unlock-restore-open"]')
  }

  /** 弹窗内元素（teleport 已 stub：内容内联在 wrapper 内，先例 StartupFailureScreen.test）。 */
  function modalEl(wrapper: ReturnType<typeof mount>, testid: string) {
    return wrapper.find(`[data-testid="${testid}"]`)
  }

  /** 弹窗打开稳定等待：naive-ui NModal 经过渡挂载内容，仅 flushPromises 时序不稳。 */
  async function waitModal() {
    await flushPromises()
    await new Promise((resolve) => setTimeout(resolve, 50))
    await flushPromises()
  }

  /** 弹窗内口令输入：经 data-testid 容器定位内层 input（解锁屏自身也有 NInput，
   *  findComponent 会误中先渲染的解锁框，不能用）。 */
  async function typePassphrase(wrapper: ReturnType<typeof mount>, value: string) {
    const input = modalEl(wrapper, 'restore-passphrase').find('input')
    await input.setValue(value)
    await flushPromises()
  }

  /** 挂载解锁屏并桩好恢复通道命令面：文件选择器返回指定备份。 */
  async function mountWithRestore(
    backupPath: string,
    overrides: Record<string, (args?: any) => unknown> = {},
  ) {
    mockOpen.mockResolvedValue(backupPath)
    stubInvoke({
      get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
      get_backup_meta: () => Promise.resolve({ kind: 'manual', encrypted: false }),
      get_encryption_status: () => Promise.resolve({ locked: true, file_encrypted: true }),
      restore_backup: () =>
        Promise.resolve({ schema_version: 42, restored_at: '2026-09-06T00:00:00Z' }),
      ...overrides,
    })
    const gate = useEncryptionGate()
    const probePromise = gate.probe()
    const wrapper = mount(UnlockScreen, {
      // 恢复确认弹窗（AppModal/NModal）默认 teleport 到 body：stub 内联渲染才能断言
      global: { stubs: { teleport: true } },
    })
    await probePromise
    await flushPromises()
    return { wrapper, gate }
  }

  it('恢复入口常驻：与「忘记口令」并列可见', async () => {
    const { wrapper } = await mountWithRestore('/tmp/plain.db.zip')
    expect(findButton(wrapper, '从备份文件恢复')).toBeTruthy()
    expect(findButton(wrapper, '忘记口令')).toBeTruthy()
  })

  it('明文备份跨模式（密文库 → 明文）：警告照常出现，确认后恢复并自动重启进入恢复后的数据', async () => {
    const { wrapper } = await mountWithRestore('/tmp/plain.db.zip')
    await entryButton(wrapper)!.trigger('click')
    await waitModal()

    // 校验链在确认前发生（元数据 + 当前模式探测）
    expect(mockInvoke.mock.calls.some(([c]) => c === 'get_backup_meta')).toBe(true)
    expect(mockInvoke.mock.calls.some(([c]) => c === 'get_encryption_status')).toBe(true)
    // 密文库恢复明文备份：跨模式警告照常出现（复用 #572 警告语义）
    expect(wrapper.text()).toContain('加密模式不一致')
    // 明文备份无需口令；未确认前不发恢复命令
    expect(modalEl(wrapper, 'restore-passphrase').exists()).toBe(false)
    expect(mockInvoke.mock.calls.some(([c]) => c === 'restore_backup')).toBe(false)

    await modalEl(wrapper, 'restore-confirm')!.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('restore_backup', {
      backupPath: '/tmp/plain.db.zip',
      passphrase: null,
    })
    // 恢复成功即自动重启，由启动探测接管实际模式（重启后直达主界面）
    expect(restartAppShortly).toHaveBeenCalledTimes(1)
  })

  it('密文备份上下文口令自动试开：手输过的口令随确认上送，无需重复输入', async () => {
    const { wrapper } = await mountWithRestore('/tmp/enc.db.zip', {
      get_backup_meta: () => Promise.resolve({ kind: 'manual', encrypted: true }),
    })
    // 先在解锁框手输口令（不提交解锁），再进入恢复入口
    await wrapper.find('input').setValue('typed-pw')
    await entryButton(wrapper)!.trigger('click')
    await waitModal()

    // 有上下文口令：不显出口令框，确认即可
    expect(modalEl(wrapper, 'restore-passphrase').exists()).toBe(false)
    await modalEl(wrapper, 'restore-confirm')!.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('restore_backup', {
      backupPath: '/tmp/enc.db.zip',
      passphrase: 'typed-pw',
    })
    expect(restartAppShortly).toHaveBeenCalledTimes(1)
  })

  it('上下文口令试开失败：口令框显出可就地重输（合并口径文案），重输成功后自动重启', async () => {
    let failFirst = true
    const { wrapper } = await mountWithRestore('/tmp/enc.db.zip', {
      get_backup_meta: () => Promise.resolve({ kind: 'manual', encrypted: true }),
      restore_backup: () =>
        failFirst
          ? Promise.reject({
              kind: 'Coded',
              code: 'encryption.passphrase-incorrect',
              message: '口令错误或文件损坏，请重试',
            })
          : Promise.resolve({ schema_version: 42, restored_at: '2026-09-06T00:00:00Z' }),
    })
    await wrapper.find('input').setValue('wrong-pw')
    await entryButton(wrapper)!.trigger('click')
    await waitModal()

    // 第一次：上下文口令自动试开失败 → 错误留在弹窗内（不关弹窗），口令框显出
    await modalEl(wrapper, 'restore-confirm')!.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenLastCalledWith('restore_backup', {
      backupPath: '/tmp/enc.db.zip',
      passphrase: 'wrong-pw',
    })
    expect(wrapper.text()).toContain('口令错误或文件损坏，请重试')
    expect(modalEl(wrapper, 'restore-confirm').exists()).toBe(true)
    expect(modalEl(wrapper, 'restore-passphrase').exists()).toBe(true)
    expect(restartAppShortly).not.toHaveBeenCalled()

    // 第二次：重输正确口令 → 恢复成功，自动重启
    failFirst = false
    await typePassphrase(wrapper, 'right-pw')
    await modalEl(wrapper, 'restore-confirm').trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenLastCalledWith('restore_backup', {
      backupPath: '/tmp/enc.db.zip',
      passphrase: 'right-pw',
    })
    expect(restartAppShortly).toHaveBeenCalledTimes(1)
  })

  it('未手输口令时进入恢复入口：密文备份直接弹口令框（无上下文口令可试开）', async () => {
    const { wrapper } = await mountWithRestore('/tmp/enc.db.zip', {
      get_backup_meta: () => Promise.resolve({ kind: 'manual', encrypted: true }),
    })
    await entryButton(wrapper)!.trigger('click')
    await waitModal()

    // 无上下文口令：密文备份直接显出口令框，口令未输时确认禁用（防呆）
    expect(modalEl(wrapper, 'restore-passphrase').exists()).toBe(true)
    expect((modalEl(wrapper, 'restore-confirm')!.element as HTMLButtonElement).disabled).toBe(true)
    expect(mockInvoke.mock.calls.some(([c]) => c === 'restore_backup')).toBe(false)
  })
})
