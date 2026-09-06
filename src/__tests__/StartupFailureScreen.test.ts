import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'

import StartupFailureScreen from '@/components/StartupFailureScreen.vue'
import { useEncryptionGate } from '@/composables/useEncryptionGate'
import { open } from '@tauri-apps/plugin-dialog'
import { stubReferenceInvoke } from './helpers/reference-stubs'

// 文件选择与重启单点 mock（先例 useBackup.test.ts；restartAppShortly 内含
// 800ms 延时，测试断言调用而非计时）。
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
  save: vi.fn(),
  confirm: vi.fn(),
}))
const restartAppShortly = vi.fn()
vi.mock('@/utils/restart', () => ({ restartAppShortly: () => restartAppShortly() }))
const mockOpen = vi.mocked(open)


/** mock-invoke 桩：失败恢复屏只消费启动命令面（fail-loud：其余命令一律拒绝）。 */
function stubInvoke(overrides: Record<string, (args?: any) => unknown> = {}) {
  stubReferenceInvoke({
    list_insurers: [],
    ...overrides,
  })
}

beforeEach(() => {
  mockInvoke.mockReset()
  mockOpen.mockReset()
  restartAppShortly.mockClear()
  setActivePinia(createPinia())
  // 每个用例从「未探测」起步（模块级单例状态复位）。
  const gate = useEncryptionGate()
  gate.locked.value = null
  gate.bootFailed.value = false
})

function findButton(wrapper: ReturnType<typeof mount>, testid: string) {
  return wrapper.find(`[data-testid="${testid}"]`)
}

/** 弹窗打开稳定等待：naive-ui NModal 经过渡挂载内容，仅 flushPromises 时序不稳。 */
async function waitModal() {
  await flushPromises()
  await new Promise((resolve) => setTimeout(resolve, 50))
  await flushPromises()
}

/** 以「启动已失败」现场挂载失败恢复屏（模拟 probe 返回 failed 后的状态）。 */
async function mountFailedScreen(
  overrides: Record<string, (args?: any) => unknown> = {},
) {
  stubInvoke(overrides)
  const gate = useEncryptionGate()
  gate.bootFailed.value = true
  const wrapper = mount(StartupFailureScreen, {
    // 确认弹窗（AppModal/NModal）默认 teleport 到 body：stub 内联渲染才能断言
    global: { stubs: { teleport: true } },
  })
  await flushPromises()
  return { wrapper, gate }
}

/** 备份恢复通道（issue #602）的命令面桩：文件选择返回指定备份，其余可覆写。 */
function stubRestoreChannel(backupPath: string, overrides: Record<string, (args?: any) => unknown> = {}) {
  mockOpen.mockResolvedValue(backupPath)
  return stubInvoke({
    get_backup_meta: () => Promise.resolve({ kind: 'manual', encrypted: false }),
    get_encryption_status: () => Promise.resolve({ locked: false, file_encrypted: false }),
    restore_backup: () =>
      Promise.resolve({ schema_version: 42, restored_at: '2026-09-06T00:00:00Z' }),
    ...overrides,
  })
}

describe('StartupFailureScreen.vue（启动失败恢复屏·issue #601）', () => {
  it('失败态挂载恢复屏：标题、说明与「重置为空库」通道入口就位', async () => {
    const { wrapper } = await mountFailedScreen()
    const html = wrapper.html()
    expect(html).toContain('无法打开账本数据库')
    expect(html).toContain('重置为空库')
    // 不渲染主界面业务面（失败期间业务 IPC 被后端门禁拦截）
    expect(html).not.toContain('仪表盘')
  })

  it('重置需二次确认：先展示不可逆后果说明，确认才调用重置命令', async () => {
    const { wrapper, gate } = await mountFailedScreen({
      reset_after_startup_failure: () => Promise.resolve(),
    })

    // 未确认前不发命令
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'reset_after_startup_failure'),
    ).toBe(false)

    // 打开确认弹窗：后果说明（不可逆 + .bak 副本）可见
    await findButton(wrapper, 'failure-reset-open')!.trigger('click')
    await waitModal()
    const html = wrapper.html()
    expect(html).toContain('重置为空库？')
    expect(html).toContain('不可逆')
    expect(html).toContain('无法在本应用中访问')
    expect(html).toContain('ledger.db.bak')

    await findButton(wrapper, 'failure-reset-confirm')!.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('reset_after_startup_failure')
    // 成功即翻转状态：失败屏退场，主界面随全新空库挂载
    expect(gate.bootFailed.value).toBe(false)
    expect(gate.locked.value).toBe(false)
  })

  it('二次确认取消：留在失败恢复屏，不发起重置', async () => {
    const { wrapper, gate } = await mountFailedScreen({
      reset_after_startup_failure: () => Promise.resolve(),
    })

    await findButton(wrapper, 'failure-reset-open')!.trigger('click')
    await waitModal()
    await findButton(wrapper, 'failure-reset-cancel')!.trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'reset_after_startup_failure'),
    ).toBe(false)
    expect(gate.bootFailed.value).toBe(true)
    expect(wrapper.html()).toContain('无法打开账本数据库')
  })

  it('重置失败：错误文案透传，保持失败态可重试', async () => {
    const { wrapper, gate } = await mountFailedScreen({
      reset_after_startup_failure: () =>
        Promise.reject({
          kind: 'Invalid',
          message: '数据库文件不存在或为空',
          code: 'encryption.db-missing',
        }),
    })

    await findButton(wrapper, 'failure-reset-open')!.trigger('click')
    await waitModal()
    await findButton(wrapper, 'failure-reset-confirm')!.trigger('click')
    await flushPromises()
    expect(wrapper.html()).toContain('数据库文件不存在或为空')
    expect(gate.bootFailed.value).toBe(true)
  })

  it('探测返回 failed：门状态翻转，失败恢复屏应由 App 挂载', async () => {
    stubInvoke({
      get_boot_status: () =>
        Promise.resolve({ phase: 'failed', error_code: 'boot.db-unreadable' }),
    })
    const gate = useEncryptionGate()
    await gate.probe()
    await flushPromises()
    expect(gate.bootFailed.value).toBe(true)
    // locked 保持 null：主界面与解锁屏都不挂载
    expect(gate.locked.value).toBe(null)
  })

  it('探测返回 ready：明文库正常进入主界面（明文日常启动零改动）', async () => {
    stubInvoke({
      get_boot_status: () => Promise.resolve({ phase: 'ready', error_code: null }),
    })
    const gate = useEncryptionGate()
    await gate.probe()
    await flushPromises()
    expect(gate.bootFailed.value).toBe(false)
    expect(gate.locked.value).toBe(false)
  })

  it('探测返回 locked：密文库进入解锁屏路径（#570 既有行为零改动）', async () => {
    stubInvoke({
      get_boot_status: () => Promise.resolve({ phase: 'locked', error_code: null }),
    })
    const gate = useEncryptionGate()
    await gate.probe()
    await flushPromises()
    expect(gate.bootFailed.value).toBe(false)
    expect(gate.locked.value).toBe(true)
  })
})

describe('StartupFailureScreen.vue（备份恢复通道·issue #602）', () => {
  /** 弹窗内元素（teleport 已 stub：内容内联在 wrapper 内）。 */
  function modalEl(wrapper: ReturnType<typeof mount>, testid: string) {
    return wrapper.find(`[data-testid="${testid}"]`)
  }

  /** 弹窗内 NInput 输入口令（受控值经组件 emit 驱动，先例 RestoreConfirmModal.test）。 */
  async function typePassphrase(wrapper: ReturnType<typeof mount>, value: string) {
    const { NInput } = await import('naive-ui')
    const input = wrapper.findComponent(NInput)
    input.vm.$emit('update:value', value)
    await flushPromises()
  }

  /** 以「启动已失败 + 备份恢复通道」现场挂载。 */
  async function mountWithRestore(overrides: Record<string, (args?: any) => unknown> = {}) {
    stubRestoreChannel('/tmp/plain.db.zip', overrides)
    const gate = useEncryptionGate()
    gate.bootFailed.value = true
    const wrapper = mount(StartupFailureScreen, {
      global: { stubs: { teleport: true } },
    })
    await flushPromises()
    return { wrapper, gate }
  }

  async function openRestoreModal(wrapper: ReturnType<typeof mount>) {
    await findButton(wrapper, 'failure-restore-open')!.trigger('click')
    await waitModal()
  }

  it('恢复通道入口就位：标题、说明与「从备份文件恢复…」按钮可见', async () => {
    const { wrapper } = await mountWithRestore()
    const html = wrapper.html()
    expect(html).toContain('从备份文件恢复')
    expect(html).toContain('备份时的状态')
    expect(findButton(wrapper, 'failure-restore-open')).toBeTruthy()
  })

  it('明文备份同模式：无跨模式警告、无需口令，确认后恢复并自动重启', async () => {
    const { wrapper } = await mountWithRestore()
    await openRestoreModal(wrapper)

    // 校验链：元数据 + 当前模式探测都在确认前发生
    expect(mockInvoke.mock.calls.some(([c]) => c === 'get_backup_meta')).toBe(true)
    expect(mockInvoke.mock.calls.some(([c]) => c === 'get_encryption_status')).toBe(true)
    // 同模式（明文备份 → 明文库）：不出现跨模式警告
    expect(wrapper.text()).not.toContain('加密模式不一致')
    // 未确认前不发恢复命令
    expect(mockInvoke.mock.calls.some(([c]) => c === 'restore_backup')).toBe(false)

    await modalEl(wrapper, 'restore-confirm')!.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('restore_backup', {
      backupPath: '/tmp/plain.db.zip',
      passphrase: null,
    })
    // 恢复成功即自动重启进入恢复后的数据（Restore 同型）
    expect(restartAppShortly).toHaveBeenCalledTimes(1)
  })

  it('密文备份跨模式（当前明文库）：显著警告 + 口令必填（无上下文口令直接弹口令框）', async () => {
    const { wrapper } = await mountWithRestore({
      get_backup_meta: () => Promise.resolve({ kind: 'manual', encrypted: true }),
    })
    await openRestoreModal(wrapper)

    // 跨模式警告照常出现（复用 #572 警告语义与文案）
    expect(wrapper.text()).toContain('加密模式不一致')
    expect(wrapper.text()).toContain('恢复后此库将变为加密库，应用重启后需凭该备份的主口令解锁')
    // 无上下文口令：密文备份直接显出口令框，口令未输时确认禁用
    expect(modalEl(wrapper, 'restore-passphrase').exists()).toBe(true)
    expect((modalEl(wrapper, 'restore-confirm').element as HTMLButtonElement).disabled).toBe(true)
    expect(mockInvoke.mock.calls.some(([c]) => c === 'restore_backup')).toBe(false)
  })

  it('密文备份口令重试：错误口令不关弹窗，重输后恢复成功并重启', async () => {
    let failFirst = true
    const { wrapper } = await mountWithRestore({
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
    await openRestoreModal(wrapper)

    // 第一次：错误口令 → 错误留在弹窗内，弹窗不关，可就地重输
    await typePassphrase(wrapper, 'wrong')
    await modalEl(wrapper, 'restore-confirm').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('口令错误或文件损坏，请重试')
    expect(modalEl(wrapper, 'restore-confirm').exists()).toBe(true)
    expect(restartAppShortly).not.toHaveBeenCalled()

    // 第二次：正确口令 → 恢复成功，自动重启
    failFirst = false
    await typePassphrase(wrapper, 'right-pw')
    await modalEl(wrapper, 'restore-confirm').trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenLastCalledWith('restore_backup', {
      backupPath: '/tmp/plain.db.zip',
      passphrase: 'right-pw',
    })
    expect(restartAppShortly).toHaveBeenCalledTimes(1)
  })

  it('备份元数据读取失败：报错中止，不打开确认弹窗、不发恢复命令', async () => {
    const { wrapper } = await mountWithRestore({
      get_backup_meta: () =>
        Promise.reject({ kind: 'Invalid', message: '不是有效的备份包', code: 'backup.corrupt' }),
    })
    await openRestoreModal(wrapper)
    // 校验失败即中止：确认弹窗不开、恢复不发（错误经 message.error 提示，
    // useMessage 为全局 mock，此处钉流程中止面）。
    expect(mockInvoke.mock.calls.some(([c]) => c === 'get_backup_meta')).toBe(true)
    expect(modalEl(wrapper, 'restore-confirm').exists()).toBe(false)
    expect(mockInvoke.mock.calls.some(([c]) => c === 'restore_backup')).toBe(false)
  })

  it('当前模式探测失败：报错中止，不打开确认弹窗（宁可不弹窗不跳过警告）', async () => {
    const { wrapper } = await mountWithRestore({
      get_encryption_status: () =>
        Promise.reject({ kind: 'Invalid', message: '探测失败', code: 'boot.db-unreadable' }),
    })
    await openRestoreModal(wrapper)
    expect(modalEl(wrapper, 'restore-confirm').exists()).toBe(false)
    expect(mockInvoke.mock.calls.some(([c]) => c === 'restore_backup')).toBe(false)
  })
})
