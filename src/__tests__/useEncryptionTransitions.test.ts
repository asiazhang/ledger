import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { defineComponent } from 'vue'
import { lastInvokeArgs, mockInvoke, wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import type { InvokeSeamOptions, InvokeSeamOverride } from '@ledger/test-support/invoke-mock'
import { messageApi } from '@ledger/test-support/message-mock'
import { registerToastSink } from '@ledger/loadable'
import { resetToastSink } from './factories'
import type { EncryptionStatus } from '@ledger/types'

// 重启钩子单点 mock：断言各转换流的编排触发与「toast 先落地再重启」时序；
// restart_app 的 IPC 时序（800ms 延迟、命令成功后重载）归既有 restart.test.ts。
vi.mock('@/backup/restart', () => ({ restartAppShortly: vi.fn() }))

import { useEncryptionTransitions } from '@/backup/useEncryptionTransitions'
import { restartAppShortly } from '@/backup/restart'
import { useEncryptionGate } from '@/backup/useEncryptionGate'
import { useAppStore } from '@/stores/app'

const plaintextStatus: EncryptionStatus = { locked: false, file_encrypted: false }
const encryptedStatus: EncryptionStatus = { locked: false, file_encrypted: true }

/** 合法主口令（恰 8 位边界值，issue #650 最小长度 ≥8）。 */
const PASS_OK = '主口令至少八个字'
/** 过短主口令（3 位，触发字段错误红显）。 */
const PASS_SHORT = '短口令'

/** 承载 composable 生命周期的宿主组件（useBackup.test.ts 先例）：工厂体内
 *  useMessage() 须有 setup 上下文，setup 中捕获返回值供断言。 */
function mountHost() {
  let tr!: ReturnType<typeof useEncryptionTransitions>
  const Host = defineComponent({
    setup() {
      tr = useEncryptionTransitions()
      return () => null
    },
  })
  mount(Host)
  return { tr }
}

/** 已挂载且状态就绪的宿主（默认明文库）。 */
async function mountReadyHost(
  seam: InvokeSeamOptions = {
    defaults: {
      get_encryption_status: plaintextStatus,
      get_remember_passphrase_support: { supported: false },
    },
  },
) {
  wireInvokeSeam(seam)
  const { tr } = mountHost()
  await flushPromises()
  return tr
}

/** 开启流推进到「确认弹窗已打开」：合法口令 + requestEnable。 */
async function openEnableConfirm(tr: ReturnType<typeof useEncryptionTransitions>) {
  tr.passphrase.value = PASS_OK
  tr.confirmPassphrase.value = PASS_OK
  tr.requestEnable()
  expect(tr.enableConfirmShow.value).toBe(true)
}

/** 修改流推进到「确认弹窗已打开」：changeReady 三字段 + requestChange。 */
async function openChangeConfirm(tr: ReturnType<typeof useEncryptionTransitions>) {
  tr.changeOld.value = '旧口令八个字'
  tr.changeNew.value = PASS_OK
  tr.changeConfirm.value = PASS_OK
  tr.requestChange()
  expect(tr.changeConfirmShow.value).toBe(true)
}

/** 永不落定的应答（重入守卫用：让在途窗口可观察）。 */
function pending(): Promise<never> {
  return new Promise(() => {})
}

/** toast → 重启时序的观察器：记录两钩子的实际触发次序（Vitest mock 无
 *  invocationCallOrder，以实现注入记录次序，断言「toast 先落地再重启」）。 */
function trackToastRestartOrder() {
  const order: string[] = []
  messageApi.success.mockImplementation(() => order.push('toast'))
  vi.mocked(restartAppShortly).mockImplementation(() => order.push('restart'))
  return order
}

beforeEach(() => {
  // Loadable 默认错误 toast 经模块级单点 sink；测试把它接到消息替身，供断言读取
  //（book-sidebar-entry.test.ts 先例）。
  registerToastSink(messageApi)
  // 本机记住的平台能力是模块级单例态，不在全局清理四件套内，就地复位。
  const { rememberSupport } = useEncryptionGate()
  rememberSupport.value = null
})

afterEach(() => {
  resetToastSink()
})

describe('useEncryptionTransitions 状态加载', () => {
  it('挂载首刷拉取加密状态（onMounted 内化），成功后 status 落位、无错误位', async () => {
    const tr = await mountReadyHost({
      defaults: { get_encryption_status: encryptedStatus },
    })
    expect(mockInvoke).toHaveBeenCalledWith('get_encryption_status')
    expect(tr.status.value).toEqual(encryptedStatus)
    expect(tr.statusLoading.value).toBe(false)
    expect(tr.statusError.value).toBeNull()
  })

  it('首刷失败：错误位 = 裸 errorMessage + toast（默认策略），status 保持空；refresh 重试成功后清错误位', async () => {
    let fail = true
    const tr = await mountReadyHost({
      overrides: {
        get_encryption_status: () =>
          fail
            ? Promise.reject({ kind: 'Db', message: '库打不开', code: 'db.open-failed' })
            : Promise.resolve(encryptedStatus),
      },
    })
    expect(tr.statusError.value).toBe('库打不开')
    expect(tr.status.value).toBeNull()
    expect(messageApi.error).toHaveBeenCalledWith('库打不开')

    fail = false
    await tr.refresh()
    await flushPromises()
    expect(tr.status.value).toEqual(encryptedStatus)
    expect(tr.statusError.value).toBeNull()
  })
})

describe('useEncryptionTransitions 开启流', () => {
  it('requestEnable 门槛：空口令 / 两次不一致 / 过短不开确认弹窗，合法口令才开', async () => {
    const tr = await mountReadyHost()

    tr.requestEnable()
    expect(tr.enableConfirmShow.value).toBe(false)

    tr.passphrase.value = PASS_SHORT
    tr.confirmPassphrase.value = PASS_SHORT
    tr.requestEnable()
    expect(tr.enableConfirmShow.value).toBe(false)

    tr.passphrase.value = PASS_OK
    tr.confirmPassphrase.value = PASS_OK
    tr.requestEnable()
    expect(tr.enableConfirmShow.value).toBe(true)

    // 两次不一致（确认非空且不同）不开弹窗
    tr.enableConfirmShow.value = false
    tr.confirmPassphrase.value = PASS_SHORT
    tr.requestEnable()
    expect(tr.enableConfirmShow.value).toBe(false)
  })

  it('confirmEnable 先关确认弹窗再执行（确认弹窗不是重试现场），invoke 携带口令', async () => {
    const tr = await mountReadyHost({
      overrides: { enable_encryption: () => pending() },
    })
    await openEnableConfirm(tr)

    void tr.confirmEnable()
    expect(tr.enableConfirmShow.value).toBe(false)
    expect(tr.submitting.value).toBe(true)
    await flushPromises()
    expect(lastInvokeArgs('enable_encryption')).toEqual({ passphrase: PASS_OK })
  })

  it('成功链路：转换 → 成功 toast → 触发重启，toast 先落地、表单字段清空', async () => {
    const order = trackToastRestartOrder()
    const tr = await mountReadyHost({
      defaults: { get_encryption_status: plaintextStatus, restart_app: null },
      overrides: { enable_encryption: () => Promise.resolve() },
    })
    await openEnableConfirm(tr)

    await tr.confirmEnable()
    await flushPromises()
    expect(messageApi.success).toHaveBeenCalledWith('加密已开启，应用即将重启')
    expect(tr.passphrase.value).toBe('')
    expect(tr.confirmPassphrase.value).toBe('')
    // toast 先落地再触发重启（Restart 同型时序，ADR-0080）
    expect(order).toEqual(['toast', 'restart'])
    expect(restartAppShortly).toHaveBeenCalledTimes(1)
  })

  it('转换失败：错误 toast（裸 errorMessage）、表单字段保留可改后重提、不触发重启', async () => {
    const tr = await mountReadyHost({
      overrides: {
        enable_encryption: () =>
          Promise.reject({ kind: 'Coded', message: '口令错误或文件损坏，请重试', code: 'x' }),
      },
    })
    await openEnableConfirm(tr)

    await tr.confirmEnable()
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalledWith('口令错误或文件损坏，请重试')
    expect(tr.passphrase.value).toBe(PASS_OK)
    expect(tr.confirmPassphrase.value).toBe(PASS_OK)
    expect(restartAppShortly).not.toHaveBeenCalled()
    expect(tr.submitting.value).toBe(false)

    // 字段保留可改后重提：修正应答后重新走确认流
    wireInvokeSeam({ overrides: { enable_encryption: () => Promise.resolve() } })
    tr.requestEnable()
    await tr.confirmEnable()
    await flushPromises()
    expect(restartAppShortly).toHaveBeenCalledTimes(1)
  })

  it('重入守卫：在途时 requestEnable / confirmEnable 不再开门也不再发起命令', async () => {
    const tr = await mountReadyHost({
      overrides: { enable_encryption: () => pending() },
    })
    await openEnableConfirm(tr)
    void tr.confirmEnable()
    await flushPromises()
    const calls = () => callsOf('enable_encryption')

    expect(tr.submitting.value).toBe(true)
    tr.requestEnable()
    expect(tr.enableConfirmShow.value).toBe(false)
    void tr.confirmEnable()
    expect(calls()).toBe(1)
    expect(tr.enableConfirmShow.value).toBe(false)
  })

  it('勾选自动解锁后开启：缓存主口令（set_remember_passphrase 携带口令）并置偏好', async () => {
    const tr = await mountReadyHost({
      defaults: {
        get_encryption_status: plaintextStatus,
        get_remember_passphrase_support: { supported: true },
        restart_app: null,
      },
      overrides: {
        enable_encryption: () => Promise.resolve(),
        set_remember_passphrase: () => Promise.resolve(),
      },
    })
    tr.enableRemember.value = true
    await openEnableConfirm(tr)

    await tr.confirmEnable()
    await flushPromises()
    expect(lastInvokeArgs('set_remember_passphrase')).toEqual({ passphrase: PASS_OK })
    expect(useAppStore().rememberPassphrase).toBe(true)
  })

  it('缓存同步失败不阻断重启：warning 提示未记住、偏好回退、仍 success + 重启', async () => {
    const tr = await mountReadyHost({
      defaults: { get_encryption_status: plaintextStatus, restart_app: null },
      overrides: {
        enable_encryption: () => Promise.resolve(),
        set_remember_passphrase: () =>
          Promise.reject({ kind: 'Keyring', message: '钥匙串不可用', code: 'x' }),
      },
    })
    tr.enableRemember.value = true
    await openEnableConfirm(tr)

    await tr.confirmEnable()
    await flushPromises()
    expect(messageApi.warning).toHaveBeenCalledWith('未能启用自动解锁，已保持手动输入')
    expect(useAppStore().rememberPassphrase).toBe(false)
    expect(messageApi.success).toHaveBeenCalledWith('加密已开启，应用即将重启')
    expect(restartAppShortly).toHaveBeenCalledTimes(1)
  })

  it('未勾选自动解锁：清缓存恢复手输（clear_remember_passphrase），不缓存口令', async () => {
    const tr = await mountReadyHost({
      defaults: {
        get_encryption_status: plaintextStatus,
        restart_app: null,
        clear_remember_passphrase: null,
      },
      overrides: { enable_encryption: () => Promise.resolve() },
    })
    await openEnableConfirm(tr)

    await tr.confirmEnable()
    await flushPromises()
    expect(callsOf('set_remember_passphrase')).toBe(0)
    expect(mockInvoke).toHaveBeenCalledWith('clear_remember_passphrase')
    expect(useAppStore().rememberPassphrase).toBe(false)
  })
})

describe('useEncryptionTransitions 修改主口令流', () => {
  it('requestChange 门槛：changeReady 不满足（旧口令空 / 不一致 / 同旧口令 / 过短）不开弹窗', async () => {
    const tr = await mountReadyHost({ defaults: { get_encryption_status: encryptedStatus } })

    tr.requestChange()
    expect(tr.changeConfirmShow.value).toBe(false)

    tr.changeOld.value = '旧口令八个字'
    tr.changeNew.value = PASS_OK
    tr.changeConfirm.value = PASS_OK
    tr.requestChange()
    expect(tr.changeConfirmShow.value).toBe(true)
  })

  it('confirmChange 先关确认弹窗再执行，invoke 携带新旧口令', async () => {
    const tr = await mountReadyHost({
      defaults: { get_encryption_status: encryptedStatus },
      overrides: { change_encryption_passphrase: () => pending() },
    })
    await openChangeConfirm(tr)

    void tr.confirmChange()
    expect(tr.changeConfirmShow.value).toBe(false)
    expect(tr.submittingChange.value).toBe(true)
    await flushPromises()
    expect(lastInvokeArgs('change_encryption_passphrase')).toEqual({
      passphrase: '旧口令八个字',
      newPassphrase: PASS_OK,
    })
  })

  it('成功链路：转换 → 成功 toast → 触发重启，三字段清空', async () => {
    const order = trackToastRestartOrder()
    const tr = await mountReadyHost({
      defaults: { get_encryption_status: encryptedStatus, restart_app: null },
      overrides: { change_encryption_passphrase: () => Promise.resolve() },
    })
    await openChangeConfirm(tr)

    await tr.confirmChange()
    await flushPromises()
    expect(messageApi.success).toHaveBeenCalledWith('主口令已修改，应用即将重启')
    expect(tr.changeOld.value).toBe('')
    expect(tr.changeNew.value).toBe('')
    expect(tr.changeConfirm.value).toBe('')
    expect(order).toEqual(['toast', 'restart'])
    expect(restartAppShortly).toHaveBeenCalledTimes(1)
  })

  it('旧口令错误：错误 toast、字段保留、不触发重启（原库原样保留）', async () => {
    const tr = await mountReadyHost({
      defaults: { get_encryption_status: encryptedStatus },
      overrides: {
        change_encryption_passphrase: () =>
          Promise.reject({
            kind: 'Coded',
            message: '口令错误或文件损坏，请重试',
            code: 'encryption.passphrase-incorrect',
          }),
      },
    })
    await openChangeConfirm(tr)

    await tr.confirmChange()
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalledWith('口令错误或文件损坏，请重试')
    expect(tr.changeOld.value).toBe('旧口令八个字')
    expect(restartAppShortly).not.toHaveBeenCalled()
  })

  it('重入守卫：在途时 requestChange / confirmChange 不再开门也不再发起命令', async () => {
    const tr = await mountReadyHost({
      defaults: { get_encryption_status: encryptedStatus },
      overrides: { change_encryption_passphrase: () => pending() },
    })
    await openChangeConfirm(tr)
    void tr.confirmChange()
    await flushPromises()

    expect(tr.submittingChange.value).toBe(true)
    tr.requestChange()
    expect(tr.changeConfirmShow.value).toBe(false)
    void tr.confirmChange()
    expect(callsOf('change_encryption_passphrase')).toBe(1)
  })
})

describe('useEncryptionTransitions 关闭加密流', () => {
  it('requestDisable 门槛：当前口令为空不开弹窗，非空才开', async () => {
    const tr = await mountReadyHost({ defaults: { get_encryption_status: encryptedStatus } })

    tr.requestDisable()
    expect(tr.disableConfirmShow.value).toBe(false)

    tr.disablePassphrase.value = '当前口令八个字'
    tr.requestDisable()
    expect(tr.disableConfirmShow.value).toBe(true)
  })

  it('confirmDisable 先关确认弹窗再执行，invoke 携带当前口令；先清缓存再 toast 再重启', async () => {
    const order = trackToastRestartOrder()
    const tr = await mountReadyHost({
      defaults: {
        get_encryption_status: encryptedStatus,
        restart_app: null,
        clear_remember_passphrase: null,
      },
      overrides: { disable_encryption: () => Promise.resolve() },
    })
    tr.disablePassphrase.value = '当前口令八个字'
    tr.requestDisable()

    await tr.confirmDisable()
    await flushPromises()
    expect(lastInvokeArgs('disable_encryption')).toEqual({ passphrase: '当前口令八个字' })
    expect(mockInvoke).toHaveBeenCalledWith('clear_remember_passphrase')
    expect(useAppStore().rememberPassphrase).toBe(false)
    expect(tr.disablePassphrase.value).toBe('')
    expect(messageApi.success).toHaveBeenCalledWith('加密已关闭，应用即将重启')
    expect(order).toEqual(['toast', 'restart'])
    expect(restartAppShortly).toHaveBeenCalledTimes(1)
  })

  it('关闭失败：错误 toast、口令保留、不触发重启', async () => {
    const tr = await mountReadyHost({
      defaults: { get_encryption_status: encryptedStatus },
      overrides: {
        disable_encryption: () =>
          Promise.reject({
            kind: 'Coded',
            message: '口令错误或文件损坏，请重试',
            code: 'encryption.passphrase-incorrect',
          }),
      },
    })
    tr.disablePassphrase.value = '错口令八个字'
    tr.requestDisable()

    await tr.confirmDisable()
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalledWith('口令错误或文件损坏，请重试')
    expect(tr.disablePassphrase.value).toBe('错口令八个字')
    expect(restartAppShortly).not.toHaveBeenCalled()
  })

  it('重入守卫：在途时 requestDisable / confirmDisable 不再开门也不再发起命令', async () => {
    const tr = await mountReadyHost({
      defaults: { get_encryption_status: encryptedStatus },
      overrides: { disable_encryption: () => pending() },
    })
    tr.disablePassphrase.value = '当前口令八个字'
    tr.requestDisable()
    void tr.confirmDisable()
    await flushPromises()

    expect(tr.submittingDisable.value).toBe(true)
    tr.requestDisable()
    expect(tr.disableConfirmShow.value).toBe(false)
    void tr.confirmDisable()
    expect(callsOf('disable_encryption')).toBe(1)
  })
})

describe('useEncryptionTransitions 自动解锁', () => {
  /** 已加密 + 支持自动解锁的宿主。 */
  async function mountEncryptedHost(
    overrides: Record<string, InvokeSeamOverride> = {},
  ) {
    return mountReadyHost({
      defaults: {
        get_encryption_status: encryptedStatus,
        get_remember_passphrase_support: { supported: true },
      },
      overrides,
    })
  }

  it('rememberSupport 透传 useEncryptionGate 单例（平台能力懒加载）', async () => {
    const tr = await mountEncryptedHost()
    expect(tr.rememberSupport.value).toEqual({ supported: true })
    expect(tr.rememberSupport).toBe(useEncryptionGate().rememberSupport)
  })

  it('openAutoUnlockModal 打开小弹窗并清上次输入与错误', async () => {
    const tr = await mountEncryptedHost({
      set_remember_passphrase: () =>
        Promise.reject({ kind: 'Coded', message: '口令错误或文件损坏，请重试', code: 'x' }),
    })
    tr.autoUnlockPass.value = '残留'
    tr.autoUnlockError.value = '残留错误'

    tr.openAutoUnlockModal()
    expect(tr.autoUnlockModalShow.value).toBe(true)
    expect(tr.autoUnlockPass.value).toBe('')
    expect(tr.autoUnlockError.value).toBeNull()
  })

  it('confirmAutoUnlock：invoke 携带当前口令、置偏好、success toast、关弹窗清输入', async () => {
    const tr = await mountEncryptedHost({
      set_remember_passphrase: () => Promise.resolve(),
    })
    tr.openAutoUnlockModal()
    tr.autoUnlockPass.value = '当前口令八个字'

    await tr.confirmAutoUnlock()
    await flushPromises()
    expect(lastInvokeArgs('set_remember_passphrase')).toEqual({ passphrase: '当前口令八个字' })
    expect(useAppStore().rememberPassphrase).toBe(true)
    expect(messageApi.success).toHaveBeenCalledWith('已启用自动解锁')
    expect(tr.autoUnlockModalShow.value).toBe(false)
    expect(tr.autoUnlockPass.value).toBe('')
    expect(tr.autoUnlockError.value).toBeNull()
  })

  it('确认失败不弹 toast（silent）：error 就地置位（裸 errorMessage）、偏好不置位、弹窗保持打开；重试成功后翻转到已启用', async () => {
    const tr = await mountEncryptedHost({
      set_remember_passphrase: vi
        .fn()
        .mockRejectedValueOnce({
          kind: 'Coded',
          message: '口令错误或文件损坏，请重试',
          code: 'encryption.passphrase-incorrect',
        })
        .mockResolvedValueOnce(undefined),
    })
    tr.openAutoUnlockModal()
    tr.autoUnlockPass.value = '错口令八个字'

    await tr.confirmAutoUnlock()
    await flushPromises()
    expect(tr.autoUnlockError.value).toBe('口令错误或文件损坏，请重试')
    expect(useAppStore().rememberPassphrase).toBe(false)
    expect(tr.autoUnlockModalShow.value).toBe(true)
    expect(messageApi.error).not.toHaveBeenCalled()
    expect(tr.autoUnlockSubmitting.value).toBe(false)

    // 就地重试：换正确口令后启用成功
    tr.autoUnlockPass.value = '正确口令八个字'
    await tr.confirmAutoUnlock()
    await flushPromises()
    expect(useAppStore().rememberPassphrase).toBe(true)
    expect(tr.autoUnlockModalShow.value).toBe(false)
    expect(tr.autoUnlockOn.value).toBe(true)
  })

  it('重入守卫：在途时 confirmAutoUnlock 不再发起命令', async () => {
    const tr = await mountEncryptedHost({ set_remember_passphrase: () => pending() })
    tr.openAutoUnlockModal()
    tr.autoUnlockPass.value = '当前口令八个字'

    void tr.confirmAutoUnlock()
    await flushPromises()
    expect(tr.autoUnlockSubmitting.value).toBe(true)

    void tr.confirmAutoUnlock()
    expect(callsOf('set_remember_passphrase')).toBe(1)
  })

  it('disableAutoUnlock：立即清缓存恢复手输（clear_remember_passphrase）并提示，偏好回 false', async () => {
    useAppStore().setRememberPassphrase(true)
    const tr = await mountEncryptedHost({ clear_remember_passphrase: null })

    await tr.disableAutoUnlock()
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('clear_remember_passphrase')
    expect(useAppStore().rememberPassphrase).toBe(false)
    expect(tr.autoUnlockOn.value).toBe(false)
    expect(messageApi.success).toHaveBeenCalledWith('已关闭自动解锁，下次启动恢复手动输入主口令')
  })
})

// —— invoke 调用读取：次数摘要（args 读取走 test-support 的 lastInvokeArgs）——

function callsOf(cmd: string): number {
  return mockInvoke.mock.calls.filter(([c]) => c === cmd).length
}
