import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { invoke } from '@tauri-apps/api/core'
import { setActivePinia, createPinia } from 'pinia'

import StartupFailureScreen from '@/components/StartupFailureScreen.vue'
import { useEncryptionGate } from '@/composables/useEncryptionGate'

const mockInvoke = vi.mocked(invoke)

/** mock-invoke 桩：失败恢复屏只消费启动命令面（fail-loud：其余命令一律拒绝）。 */
function stubInvoke(overrides: Record<string, (args?: any) => unknown> = {}) {
  mockInvoke.mockImplementation((cmd: string, args?: any) => {
    if (cmd in overrides) return overrides[cmd](args)
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

beforeEach(() => {
  mockInvoke.mockReset()
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
