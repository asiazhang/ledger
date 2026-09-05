import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { NInput } from 'naive-ui'
import AppModal from '@/components/AppModal.vue'
import RestoreConfirmModal from '@/components/settings/RestoreConfirmModal.vue'
import type { RestoreIntent } from '@/composables/useBackup'

// 恢复确认弹窗（issue #572 / ADR-0075 决策 7）：钉住跨模式显著警告文案与
// 密文备份主口令输入面——文案经 i18n，断言当前语言渲染出的完整句子。
// NModal 内容 teleport 到 document.body，须在每个测试后卸载 wrapper 并清空 body
// （先例：ManualPriceModal.test.ts）。
enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

/** 恢复确认回调桩（useBackup.confirmRestore 的形状）。 */
const onConfirm = vi.fn<(passphrase: string) => Promise<void>>()

beforeEach(() => {
  onConfirm.mockReset()
  onConfirm.mockResolvedValue(undefined)
})

const sameModePlaintext: RestoreIntent = {
  path: '/Users/me/backups/plain.db.zip',
  backupEncrypted: false,
  currentEncrypted: false,
}
const crossToPlaintext: RestoreIntent = {
  path: '/Users/me/backups/plain.db.zip',
  backupEncrypted: false,
  currentEncrypted: true,
}
const crossToEncrypted: RestoreIntent = {
  path: '/Users/me/backups/enc.db.zip',
  backupEncrypted: true,
  currentEncrypted: false,
}

function mountModal(intent: RestoreIntent | null, seq = 1) {
  return mount(RestoreConfirmModal, {
    props: { intent, seq, onConfirm },
    global: { components: { AppModal } },
  })
}

function bodyQuery(selector: string): HTMLElement | null {
  return document.body.querySelector(selector)
}

function confirmButton(): HTMLButtonElement {
  return bodyQuery('[data-testid="restore-confirm"]') as HTMLButtonElement
}

/** 输入主口令（弹窗内密码输入经 NInput 组件 emit 驱动，受控值在组件态）。 */
async function setPassphrase(wrapper: ReturnType<typeof mount>, value: string) {
  const input = wrapper.findComponent(NInput)
  input.vm.$emit('update:value', value)
  await flushPromises()
}

describe('RestoreConfirmModal（恢复确认弹窗，issue #572）', () => {
  it('同模式（明文→明文）：不显示跨模式警告，无需口令即可确认', async () => {
    const wrapper = mountModal(sameModePlaintext)
    await flushPromises()

    const body = document.body
    expect(body.textContent).toContain('恢复将替换当前全部数据')
    expect(body.textContent).not.toContain('加密模式不一致')
    expect(needsPassphraseInput()).toBe(false)
    expect(confirmButton().disabled).toBe(false)

    confirmButton().dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
    await flushPromises()
    expect(onConfirm).toHaveBeenCalledWith('')
  })

  it('跨模式（明文备份 → 加密库）：显著警告恢复后变为未加密', async () => {
    const wrapper = mountModal(crossToPlaintext)
    await flushPromises()

    expect(document.body.textContent).toContain('加密模式不一致')
    expect(document.body.textContent).toContain(
      '恢复这份明文备份后，数据将变为未加密，应用重启后以明文库启动',
    )
    expect(needsPassphraseInput()).toBe(false)
    void wrapper
  })

  it('跨模式（密文备份 → 明文库）：显著警告恢复后变为加密，且需输入主口令', async () => {
    const wrapper = mountModal(crossToEncrypted)
    await flushPromises()

    expect(document.body.textContent).toContain('加密模式不一致')
    expect(document.body.textContent).toContain(
      '恢复后此库将变为加密库，应用重启后需凭该备份的主口令解锁',
    )
    // 密文备份：口令未输时确认禁用（防呆）。
    expect(needsPassphraseInput()).toBe(true)
    expect(confirmButton().disabled).toBe(true)

    await setPassphrase(wrapper, 'pw')
    expect(confirmButton().disabled).toBe(false)

    confirmButton().dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
    await flushPromises()
    expect(onConfirm).toHaveBeenCalledWith('pw')
  })

  it('确认失败：错误留在弹窗内可就地重试（口令错误不关弹窗）', async () => {
    onConfirm.mockRejectedValueOnce({
      kind: 'Coded',
      code: 'encryption.passphrase-incorrect',
      message: '主口令不正确，请重试',
    })
    const wrapper = mountModal(crossToEncrypted)
    await flushPromises()
    await setPassphrase(wrapper, 'wrong')

    confirmButton().dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
    await flushPromises()

    expect(document.body.textContent).toContain('主口令不正确，请重试')
    // 弹窗仍开着：确认按钮仍在，可重试。
    expect(confirmButton()).not.toBeNull()
  })

  it('确认失败且错误为需口令（后端探测实库为密文）：显出口令输入可就地补救', async () => {
    // 异常产物：元数据谎报明文（弹窗未渲染口令框），后端探测实际密文拒绝。
    onConfirm.mockRejectedValueOnce({
      kind: 'Coded',
      code: 'backup.passphrase-required',
      message: '该备份为密文备份，需要备份所在库的主口令才能恢复',
    })
    const wrapper = mountModal(sameModePlaintext)
    await flushPromises()
    expect(needsPassphraseInput()).toBe(false)

    confirmButton().dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
    await flushPromises()

    // 以后端探测为准：口令输入显出，用户可输入后重试而非卡死。
    expect(needsPassphraseInput()).toBe(true)
    expect(document.body.textContent).toContain('该备份为密文备份')
    expect(confirmButton().disabled).toBe(true)
    void wrapper
  })

  it('重开（seq 递增）：口令与错误重置，迟到的旧错误不残留', async () => {
    onConfirm.mockRejectedValueOnce(new Error('旧错误'))
    const wrapper = mountModal(crossToEncrypted, 1)
    await flushPromises()
    await setPassphrase(wrapper, 'pw')

    confirmButton().dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
    await flushPromises()
    expect(document.body.textContent).toContain('旧错误')

    // 重开：同形单意图落位（seq 递增）。
    await wrapper.setProps({ intent: { ...crossToEncrypted }, seq: 2 })
    await flushPromises()

    expect(document.body.textContent).not.toContain('旧错误')
    expect(confirmButton().disabled).toBe(true)
  })

  it('取消或关闭：emit close（关闭归父层意图编排）', async () => {
    const wrapper = mountModal(crossToEncrypted, 1)
    await flushPromises()

    const cancel = bodyQuery('[data-testid="restore-cancel"]')!
    cancel.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
    await flushPromises()

    expect(wrapper.emitted('close')).toHaveLength(1)
  })
})

/** 密文备份口令输入是否渲染（needsPassphrase 的 DOM 面）。 */
function needsPassphraseInput(): boolean {
  return bodyQuery('[data-testid="restore-passphrase"]') !== null
}
