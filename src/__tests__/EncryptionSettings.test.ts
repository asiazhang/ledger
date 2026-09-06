import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises, DOMWrapper } from '@vue/test-utils'
import { invoke } from '@tauri-apps/api/core'
import { setActivePinia, createPinia } from 'pinia'
import type { EncryptionStatus } from '@/types'
import zhAll from '@/i18n/locales/zh-CN'
import enAll from '@/i18n/locales/en-US'

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
  save: vi.fn(),
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

import EncryptionSettings from '@/components/settings/EncryptionSettings.vue'
import { useEncryptionGate } from '@/composables/useEncryptionGate'
import { hasOpenOverlay, resetOverlays } from '@/composables/overlayRegistry'
import { useAppStore } from '@/stores/app'

const mockInvoke = vi.mocked(invoke)

const plaintextStatus: EncryptionStatus = { locked: false, file_encrypted: false }
const encryptedStatus: EncryptionStatus = { locked: false, file_encrypted: true }

/** 合法主口令（恰 8 位边界值，issue #650 最小长度 ≥8）。 */
const PASS_OK = '主口令至少八个字'
/** 另一合法主口令（8 位，构造不一致 / 相同分支）。 */
const PASS_ALT = '不一样的八个字符'
/** 过短主口令（3 位，触发即时红显与提交禁用）。 */
const PASS_SHORT = '短口令'

function stubInvoke(overrides: Record<string, (args?: any) => unknown> = {}) {
  mockInvoke.mockImplementation((cmd: string, args?: any) => {
    if (cmd in overrides) return overrides[cmd](args)
    if (cmd === 'get_encryption_status') return Promise.resolve(plaintextStatus)
    if (cmd === 'list_insurers') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

beforeEach(() => {
  mockInvoke.mockReset()
  document.body.innerHTML = ''
  resetOverlays()
  setActivePinia(createPinia())
  // 本机记住（issue #574）：清空偏好 localStorage 与模块级能力态，避免跨用例泄漏。
  localStorage.removeItem('remember_passphrase')
  const { rememberSupport } = useEncryptionGate()
  rememberSupport.value = null
  messageApi.success.mockClear()
  messageApi.warning.mockClear()
  messageApi.error.mockClear()
  messageApi.info.mockClear()
})

function findButton(wrapper: ReturnType<typeof mount>, text: string) {
  return wrapper.findAll('button').find((b) => b.text().includes(text))!
}

/** 折叠区指定流程的折叠项（issue #654：低频流程默认收起）。 */
function collapseItem(wrapper: ReturnType<typeof mount>, title: string) {
  const item = wrapper.findAll('.n-collapse-item').find(
    (el) => el.find('.n-collapse-item__header').text().includes(title),
  )
  if (!item) throw new Error(`未找到折叠项：${title}`)
  return item
}

/** 展开折叠区指定流程（默认收起 → 展开后表单才挂载，displayDirective="if"）。
 *  点击目标为 header-main（naive-ui 把 onClick 绑在该子元素上）。 */
async function expandCollapseItem(wrapper: ReturnType<typeof mount>, title: string) {
  await collapseItem(wrapper, title).find('.n-collapse-item__header-main').trigger('click')
  await flushPromises()
}

/** 折叠项内容区（展开后才挂载；表单输入经此作用域定位，不依赖全局索引）。 */
function collapseContent(wrapper: ReturnType<typeof mount>, title: string) {
  const content = collapseItem(wrapper, title).find('.n-collapse-item__content-wrapper')
  if (!content.exists()) throw new Error(`折叠项未展开或内容未挂载：${title}`)
  return content
}

/** 弹窗（teleport 到 body）内按 data-testid 找按钮。 */
function bodyButton(testid: string): HTMLButtonElement {
  const btn = document.body.querySelector(`[data-testid="${testid}"]`) as HTMLButtonElement | null
  if (!btn) throw new Error(`未找到 testid=${testid} 的按钮`)
  return btn
}

/** 弹窗内的警示块（表单区警示块不 teleport，弹窗内容在 .n-modal 下）。 */
function modalAlert(): Element | null {
  return document.body.querySelector('.n-modal .n-alert')
}

/** 强度条行（issue #685：纯展示、逐键刷新；scope 内全部强度条）。 */
function strengthMeters(scope: ReturnType<typeof mount> | ReturnType<typeof collapseContent>) {
  return scope.findAll('[data-testid="passphrase-strength"]')
}

/** 典型弱口令（issue #685 验收样例：password123 → 弱）。 */
const PASS_WEAK = 'password123'
/** 典型极强口令（长随机串）。 */
const PASS_VERY_STRONG = 'qW7#mKx2$vLp9&zR4'

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

    await setPasswords(wrapper, PASS_OK, PASS_OK)
    await flushPromises()
    const button = findButton(wrapper, '开启加密')!
    expect(button.element.disabled).toBe(false)
  })

  it('两次输入不一致：禁用提交并给出即时反馈', async () => {
    stubInvoke()
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await setPasswords(wrapper, PASS_OK, PASS_ALT)
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
          expect(args.passphrase).toBe(PASS_OK)
          return Promise.resolve()
        },
        restart_app: () => Promise.resolve(),
      })
      const wrapper = mount(EncryptionSettings)
      await flushPromises()

      await setPasswords(wrapper, PASS_OK, PASS_OK)
      await findButton(wrapper, '开启加密')!.trigger('click')
      await flushPromises()
      // 开启加密确认弹窗（issue #650 / ADR-0078）：点应用内确认按钮。
      bodyButton('danger-confirm').click()
      await flushPromises()
      expect(mockInvoke).toHaveBeenCalledWith('enable_encryption', { passphrase: PASS_OK })
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
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await setPasswords(wrapper, PASS_OK, PASS_OK)
    await findButton(wrapper, '开启加密')!.trigger('click')
    await flushPromises()
    // 取消弹窗（teleport 到 body）：点「取消」按钮，不发起转换。
    const cancelBtn = [...document.body.querySelectorAll('button')].find((b) =>
      b.textContent?.includes('取消'),
    )!
    cancelBtn.click()
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
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await setPasswords(wrapper, PASS_OK, PASS_OK)
    await findButton(wrapper, '开启加密')!.trigger('click')
    await flushPromises()
    bodyButton('danger-confirm').click()
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalled()
    // 未调用重启（转换失败不重启，应用回到明文可用状态）
    expect(mockInvoke).not.toHaveBeenCalledWith('restart_app')
  })

  it('已加密库：日常视图 = 已开启标识 + 自动解锁；修改主口令与关闭加密默认收起（issue #654）', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
      get_remember_passphrase_support: () => Promise.resolve({ supported: true }),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()
    const html = wrapper.html()
    // 日常视图：「已开启」标识 + 自动解锁区块（未启用 → 单按钮入口）
    expect(html).toContain('已开启加密')
    expect(html).toContain('自动解锁')
    expect(findButton(wrapper, '启用自动解锁')).toBeTruthy()
    expect(findButton(wrapper, '开启加密')).toBeUndefined()
    // 折叠区标题常驻，但两个低频流程的表单默认收起（内容不挂载，无任何输入框）
    expect(html).toContain('修改主口令')
    expect(html).toContain('关闭加密')
    expect(html).not.toContain('关闭前请确认')
    expect(wrapper.findAll('input').length).toBe(0)
  })

  it('已加密库·折叠区：展开修改主口令后表单挂载，展开关闭加密后其表单挂载', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await expandCollapseItem(wrapper, '修改主口令')
    expect(wrapper.html()).toContain('新主口令')
    expect(collapseContent(wrapper, '修改主口令').findAll('input').length).toBe(3)
    // 另一流程仍收起
    expect(collapseItem(wrapper, '关闭加密').find('.n-collapse-item__content-wrapper').exists()).toBe(false)

    await expandCollapseItem(wrapper, '关闭加密')
    expect(wrapper.html()).toContain('关闭前请确认')
    expect(collapseContent(wrapper, '关闭加密').findAll('input').length).toBe(1)
  })

  it('已加密库·修改主口令：新旧不一致禁用提交并给出即时反馈', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await expandCollapseItem(wrapper, '修改主口令')
    const inputs = collapseContent(wrapper, '修改主口令').findAll('input')
    await inputs[0].setValue('旧口令')
    await inputs[1].setValue(PASS_OK)
    await inputs[2].setValue(PASS_ALT)
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

    await expandCollapseItem(wrapper, '修改主口令')
    const inputs = collapseContent(wrapper, '修改主口令').findAll('input')
    await inputs[0].setValue('同一口令八个字符')
    await inputs[1].setValue('同一口令八个字符')
    await inputs[2].setValue('同一口令八个字符')
    await flushPromises()
    expect(wrapper.html()).toContain('新主口令与当前主口令相同')
    expect(findButton(wrapper, '修改主口令')!.element.disabled).toBe(true)
  })

  it('已加密库·修改主口令：确认弹窗（error 级）确认后调用 change_encryption_passphrase 携带新旧口令，成功提示重启', async () => {
    vi.useFakeTimers()
    try {
      stubInvoke({
        get_encryption_status: () => Promise.resolve(encryptedStatus),
        change_encryption_passphrase: (args: any) => {
          expect(args.passphrase).toBe('旧口令')
          expect(args.newPassphrase).toBe(PASS_OK)
          return Promise.resolve()
        },
        restart_app: () => Promise.resolve(),
      })
      const wrapper = mount(EncryptionSettings)
      await flushPromises()

      await expandCollapseItem(wrapper, '修改主口令')
      const inputs = collapseContent(wrapper, '修改主口令').findAll('input')
      await inputs[0].setValue('旧口令')
      await inputs[1].setValue(PASS_OK)
      await inputs[2].setValue(PASS_OK)
      await findButton(wrapper, '修改主口令')!.trigger('click')
      await flushPromises()
      // 危险确认分级（issue #650）：点应用内 error 级确认弹窗按钮，而非系统 confirm。
      bodyButton('danger-confirm').click()
      await flushPromises()
      expect(mockInvoke).toHaveBeenCalledWith('change_encryption_passphrase', {
        passphrase: '旧口令',
        newPassphrase: PASS_OK,
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
          message: '口令错误或文件损坏，请重试',
          code: 'encryption.passphrase-incorrect',
        }),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await expandCollapseItem(wrapper, '修改主口令')
    const inputs = collapseContent(wrapper, '修改主口令').findAll('input')
    await inputs[0].setValue('错口令')
    await inputs[1].setValue(PASS_OK)
    await inputs[2].setValue(PASS_OK)
    await findButton(wrapper, '修改主口令')!.trigger('click')
    await flushPromises()
    bodyButton('danger-confirm').click()
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalled()
    expect(mockInvoke).not.toHaveBeenCalledWith('restart_app')
  })

  it('已加密库·修改主口令：确认弹窗取消不发起转换，也不再调用系统原生对话框', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await expandCollapseItem(wrapper, '修改主口令')
    const inputs = collapseContent(wrapper, '修改主口令').findAll('input')
    await inputs[0].setValue('旧口令')
    await inputs[1].setValue(PASS_OK)
    await inputs[2].setValue(PASS_OK)
    await findButton(wrapper, '修改主口令')!.trigger('click')
    await flushPromises()
    bodyButton('danger-cancel').click()
    await flushPromises()
    expect(mockInvoke).not.toHaveBeenCalledWith(
      'change_encryption_passphrase',
      expect.anything(),
    )
  })

  it('已加密库·关闭加密：确认弹窗（warning 级）确认后调用 disable_encryption 携带当前口令，成功提示重启', async () => {
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
      const wrapper = mount(EncryptionSettings)
      await flushPromises()

      await expandCollapseItem(wrapper, '关闭加密')
      await collapseContent(wrapper, '关闭加密').find('input').setValue('当前口令')
      await findButton(wrapper, '关闭加密')!.trigger('click')
      await flushPromises()
      // 危险确认分级（issue #652 / ADR-0078）：点应用内 warning 级确认弹窗按钮。
      bodyButton('danger-confirm').click()
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
          message: '口令错误或文件损坏，请重试',
          code: 'encryption.passphrase-incorrect',
        }),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await expandCollapseItem(wrapper, '关闭加密')
    await collapseContent(wrapper, '关闭加密').find('input').setValue('错口令')
    await findButton(wrapper, '关闭加密')!.trigger('click')
    await flushPromises()
    bodyButton('danger-confirm').click()
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalled()
    expect(mockInvoke).not.toHaveBeenCalledWith('restart_app')
  })

  it('已加密库·关闭加密：确认弹窗取消不发起转换', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await expandCollapseItem(wrapper, '关闭加密')
    await collapseContent(wrapper, '关闭加密').find('input').setValue('当前口令')
    await findButton(wrapper, '关闭加密')!.trigger('click')
    await flushPromises()
    bodyButton('danger-cancel').click()
    await flushPromises()
    expect(mockInvoke).not.toHaveBeenCalledWith('disable_encryption', expect.anything())
  })
})

describe('危险确认分级试点（issue #650 / ADR-0078）', () => {
  it('开启表单：短于 8 位的主口令即时红显、提交禁用、不发起后端调用', async () => {
    stubInvoke()
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await setPasswords(wrapper, PASS_SHORT, PASS_SHORT)
    await flushPromises()
    expect(wrapper.html()).toContain('主口令至少需要 8 位')
    const button = findButton(wrapper, '开启加密')!
    expect(button.element.disabled).toBe(true)
    await button.trigger('click')
    await flushPromises()
    expect(mockInvoke).not.toHaveBeenCalledWith('enable_encryption', expect.anything())
    // 确认弹窗未弹出
    expect(document.body.querySelector('[data-testid="danger-confirm"]')).toBeNull()
    void wrapper
  })

  it('开启表单：恰 8 位（边界值）不红、可提交', async () => {
    stubInvoke()
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await setPasswords(wrapper, PASS_OK, PASS_OK)
    await flushPromises()
    expect(wrapper.html()).not.toContain('主口令至少需要 8 位')
    expect(findButton(wrapper, '开启加密')!.element.disabled).toBe(false)
  })

  it('开启确认弹窗：error 级视觉——红色警示块＋加粗后果句＋红色确认按钮，打开期间快捷键抑制', async () => {
    stubInvoke()
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await setPasswords(wrapper, PASS_OK, PASS_OK)
    await findButton(wrapper, '开启加密')!.trigger('click')
    await flushPromises()

    // error 级形态（ADR-0078 决策 2）：警示块内加粗后果句 + 红色确认按钮；
    // 既有语义不回退：无后门后果说明在场。
    const alert = modalAlert()
    expect(alert, '警示块应存在').toBeTruthy()
    expect(alert!.querySelector('.n-text--strong'), '后果句应加粗').toBeTruthy()
    expect(document.body.textContent).toContain('一旦忘记，这份账本的数据将无法再打开')
    const confirmBtn = bodyButton('danger-confirm')
    expect(confirmBtn.className).toContain('n-button--error-type')
    // 弹层注册表上报（ADR-0035）：打开期间快捷键照常抑制
    expect(hasOpenOverlay()).toBe(true)

    bodyButton('danger-cancel').click()
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)
    void wrapper
  })

  it('修改主口令：新口令短于 8 位即时红显、提交禁用、不发起后端调用', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await expandCollapseItem(wrapper, '修改主口令')
    const inputs = collapseContent(wrapper, '修改主口令').findAll('input')
    await inputs[0].setValue('旧口令')
    await inputs[1].setValue(PASS_SHORT)
    await inputs[2].setValue(PASS_SHORT)
    await flushPromises()
    expect(wrapper.html()).toContain('主口令至少需要 8 位')
    const button = findButton(wrapper, '修改主口令')!
    expect(button.element.disabled).toBe(true)
    await button.trigger('click')
    await flushPromises()
    expect(mockInvoke).not.toHaveBeenCalledWith('change_encryption_passphrase', expect.anything())
    expect(document.body.querySelector('[data-testid="danger-confirm"]')).toBeNull()
    void wrapper
  })

  it('关闭加密：确认走 warning 级应用内弹窗（不再出现系统原生对话框），承载兜底说明', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await expandCollapseItem(wrapper, '关闭加密')
    await collapseContent(wrapper, '关闭加密').find('input').setValue('当前口令')
    await findButton(wrapper, '关闭加密')!.trigger('click')
    await flushPromises()

    // warning 级形态（原生 confirm 已由 no-native-confirm.test.ts 全树守门）：琥珀警示块承载后果与兜底说明（密文副本保留、可再开启）+ warning 色确认按钮
    const alert = modalAlert()
    expect(alert, '警示块应存在').toBeTruthy()
    expect(document.body.textContent).toContain('日后可随时重新开启加密')
    expect(bodyButton('danger-confirm').className).toContain('n-button--warning-type')
    expect(hasOpenOverlay()).toBe(true)

    bodyButton('danger-cancel').click()
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)
    void wrapper
  })

  it('修改主口令：确认走 error 级应用内弹窗（不再出现系统原生对话框），承载无后门后果说明', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await expandCollapseItem(wrapper, '修改主口令')
    const inputs = collapseContent(wrapper, '修改主口令').findAll('input')
    await inputs[0].setValue('旧口令')
    await inputs[1].setValue(PASS_OK)
    await inputs[2].setValue(PASS_OK)
    await findButton(wrapper, '修改主口令')!.trigger('click')
    await flushPromises()

    // error 级形态（原生 confirm 已由 no-native-confirm.test.ts 全树守门）：警示块 + 加粗无后门后果说明（新主口令遗忘即数据不可读）+ 红色确认按钮
    const alert = modalAlert()
    expect(alert, '警示块应存在').toBeTruthy()
    expect(alert!.querySelector('.n-text--strong'), '后果句应加粗').toBeTruthy()
    expect(document.body.textContent).toContain('一旦忘记，这份账本的数据将无法再打开')
    expect(bodyButton('danger-confirm').className).toContain('n-button--error-type')
    expect(hasOpenOverlay()).toBe(true)
    void wrapper
  })
})

describe('EncryptionSettings.vue 自动解锁（issue #654 重做；原 #574）', () => {
  /** 记住复选项（语义定位按钮文本）：明文库开启表单内的勾选项。 */
  function findRememberCheckbox(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('.n-checkbox').find((c) => c.text().includes('自动解锁'))!
  }

  /** 小弹窗内的口令输入框（弹窗 teleport 到 body）。 */
  function modalPassInput(): DOMWrapper<HTMLInputElement> {
    const input = document.body.querySelector('.n-modal input[type="password"]')
    if (!input) throw new Error('弹窗内未找到口令输入框')
    return new DOMWrapper(input as HTMLInputElement)
  }

  it('术语统一「自动解锁」：全部 zh/en 资源无别名变体（issue #654）', () => {
    const zh = JSON.stringify(zhAll)
    const en = JSON.stringify(enAll)
    // zh 变体：旧开关文案与本机记住别名（含 errors.json 的码化错误文案）
    expect(zh).not.toContain('启动时自动解锁')
    expect(zh).not.toContain('本机记住')
    // en 变体：旧标签、旧错误措辞、连字符写法收口为 auto unlock
    expect(en).not.toContain('Unlock automatically at launch')
    expect(en).not.toContain('enable remember')
    expect(en).not.toContain('remembering the master passphrase')
    expect(en.toLowerCase()).not.toContain('auto-unlock')
  })

  it('平台支持：明文库开启表单出现「自动解锁」复选项；勾选后开启会缓存主口令', async () => {
    vi.useFakeTimers()
    try {
      stubInvoke({
        get_remember_passphrase_support: () => Promise.resolve({ supported: true }),
        enable_encryption: (args: any) => {
          expect(args.passphrase).toBe(PASS_OK)
          return Promise.resolve()
        },
        set_remember_passphrase: (args: any) => {
          expect(args.passphrase).toBe(PASS_OK)
          return Promise.resolve()
        },
        restart_app: () => Promise.resolve(),
      })
      const wrapper = mount(EncryptionSettings)
      await flushPromises()

      const checkbox = findRememberCheckbox(wrapper)
      expect(checkbox).toBeTruthy()
      await setPasswords(wrapper, PASS_OK, PASS_OK)
      await checkbox.trigger('click')
      await findButton(wrapper, '开启加密')!.trigger('click')
      await flushPromises()

      // 开启加密确认弹窗（issue #650 / ADR-0078）：点应用内确认按钮，而非系统 confirm。
      bodyButton('danger-confirm').click()
      await flushPromises()

      expect(mockInvoke).toHaveBeenCalledWith('set_remember_passphrase', { passphrase: PASS_OK })
      expect(useAppStore().rememberPassphrase).toBe(true)
      vi.advanceTimersByTime(900)
    } finally {
      vi.useRealTimers()
    }
  })

  it('平台不支持：隐藏「自动解锁」复选项与整个自动解锁区块', async () => {
    stubInvoke({
      get_remember_passphrase_support: () => Promise.resolve({ supported: false }),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()
    // 区块整体不渲染：无启用/关闭按钮，无复选项（仅剩开启表单本体）
    expect(findButton(wrapper, '启用自动解锁')).toBeUndefined()
    expect(findButton(wrapper, '关闭自动解锁')).toBeUndefined()
    expect(wrapper.findAll('.n-checkbox').length).toBe(0)
  })

  it('已加密·未启用：「启用自动解锁…」单按钮入口，弹窗输入当前主口令确认后启用并有提示', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
      get_remember_passphrase_support: () => Promise.resolve({ supported: true }),
      set_remember_passphrase: (args: any) => {
        expect(args.passphrase).toBe('当前口令')
        return Promise.resolve()
      },
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    // 无开关形态（「开关开着但未生效」中间态从形态上消灭）
    expect(wrapper.find('.n-switch').exists()).toBe(false)

    await findButton(wrapper, '启用自动解锁')!.trigger('click')
    await flushPromises()
    // 小弹窗：输入当前主口令 → 确认
    await modalPassInput().setValue('当前口令')
    bodyButton('auto-unlock-confirm').click()
    await flushPromises()

    expect(mockInvoke).toHaveBeenCalledWith('set_remember_passphrase', { passphrase: '当前口令' })
    expect(useAppStore().rememberPassphrase).toBe(true)
    expect(messageApi.success).toHaveBeenCalled()
    // 成功后入口翻转为「关闭自动解锁」（弹窗关闭；NModal 离场动画在 jsdom 不完成，
    // 不做 DOM 移除断言——与 BackupSettings 弹窗断言同口径）
    expect(findButton(wrapper, '关闭自动解锁')).toBeTruthy()
  })

  it('已加密·口令错误：就地报错且不启用，弹窗保持打开可就地重试', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
      get_remember_passphrase_support: () => Promise.resolve({ supported: true }),
      set_remember_passphrase: vi
        .fn()
        .mockRejectedValueOnce({
          kind: 'Coded',
          message: '口令错误或文件损坏，请重试',
          code: 'encryption.passphrase-incorrect',
        })
        .mockResolvedValueOnce(undefined),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await findButton(wrapper, '启用自动解锁')!.trigger('click')
    await flushPromises()
    await modalPassInput().setValue('错口令')
    bodyButton('auto-unlock-confirm').click()
    await flushPromises()

    // 就地报错：错误文本在弹窗内，偏好不置位，无成功提示
    expect(document.body.textContent).toContain('口令错误或文件损坏，请重试')
    expect(useAppStore().rememberPassphrase).toBe(false)
    expect(messageApi.success).not.toHaveBeenCalled()

    // 就地重试：输入正确口令后启用成功
    await modalPassInput().setValue('正确口令')
    bodyButton('auto-unlock-confirm').click()
    await flushPromises()
    expect(useAppStore().rememberPassphrase).toBe(true)
    expect(messageApi.success).toHaveBeenCalled()
    expect(findButton(wrapper, '关闭自动解锁')).toBeTruthy()
  })

  it('已加密·已启用：关闭自动解锁立即生效（清缓存恢复手输）并有提示', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
      get_remember_passphrase_support: () => Promise.resolve({ supported: true }),
      clear_remember_passphrase: () => Promise.resolve(),
    })
    // 预置自动解锁已启用
    useAppStore().setRememberPassphrase(true)
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    const disableBtn = findButton(wrapper, '关闭自动解锁')!
    expect(disableBtn).toBeTruthy()
    await disableBtn.trigger('click')
    await flushPromises()

    expect(mockInvoke).toHaveBeenCalledWith('clear_remember_passphrase')
    expect(useAppStore().rememberPassphrase).toBe(false)
    expect(messageApi.success).toHaveBeenCalled()
    // 即刻回未启用形态
    expect(findButton(wrapper, '启用自动解锁')).toBeTruthy()
  })

  it('已加密：自动解锁不再存在开关形态（无 .n-switch）', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
      get_remember_passphrase_support: () => Promise.resolve({ supported: true }),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()
    expect(wrapper.find('.n-switch').exists()).toBe(false)
  })

  it('开发回退形态（issue #662）：显示开发构建提示，区别于发布生物门形态', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
      get_remember_passphrase_support: () =>
        Promise.resolve({ supported: true, mode: 'dev-fallback' }),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()
    expect(wrapper.html()).toContain('当前为开发构建')
  })

  it('发布生物门形态（issue #662）：不显示开发构建提示', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
      get_remember_passphrase_support: () => Promise.resolve({ supported: true, mode: 'biometry' }),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()
    expect(wrapper.html()).not.toContain('当前为开发构建')
  })
})

describe('口令强度实时显示（issue #685）', () => {
  it('术语纪律：全部 zh 资源无「密码强度」措辞（正式术语为「口令强度」）', () => {
    expect(JSON.stringify(zhAll)).not.toContain('密码强度')
  })

  it('开启表单：初始为空不显示强度条；输入弱口令显示「弱」，换强口令逐键刷新为「极强」', async () => {
    stubInvoke()
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    // 初始为空不显示（不惩罚尚未输入）
    expect(strengthMeters(wrapper).length).toBe(0)

    await wrapper.findAll('input')[0].setValue(PASS_WEAK)
    await flushPromises()
    expect(strengthMeters(wrapper).length).toBe(1)
    expect(strengthMeters(wrapper)[0].text()).toContain('弱')

    // 逐键刷新：同一输入框换成强口令，档位随输入更新
    await wrapper.findAll('input')[0].setValue(PASS_VERY_STRONG)
    await flushPromises()
    expect(strengthMeters(wrapper).length).toBe(1)
    expect(strengthMeters(wrapper)[0].text()).toContain('极强')
  })

  it('开启表单：确认字段不显示强度条（仅新设主口令框显示）', async () => {
    stubInvoke()
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await setPasswords(wrapper, PASS_WEAK, PASS_WEAK)
    await flushPromises()
    expect(strengthMeters(wrapper).length).toBe(1)
  })

  it('开启表单：短于 8 位时字段错误红显与强度条并存（互不替代）', async () => {
    stubInvoke()
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await setPasswords(wrapper, PASS_SHORT, PASS_SHORT)
    await flushPromises()
    expect(wrapper.html()).toContain('主口令至少需要 8 位')
    expect(strengthMeters(wrapper).length).toBe(1)
    expect(strengthMeters(wrapper)[0].text()).toContain('弱')
  })

  it('纯展示：弱口令强度显示不改变提交可用性（≥8 位即由既有规则放行）', async () => {
    stubInvoke()
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    // PASS_OK 为 8 位中文串，zxcvbn 大概率判弱；即便判弱，提交可用性只由既有规则决定
    await setPasswords(wrapper, PASS_OK, PASS_OK)
    await flushPromises()
    expect(strengthMeters(wrapper).length).toBe(1)
    expect(findButton(wrapper, '开启加密')!.element.disabled).toBe(false)
  })

  it('修改主口令折叠区：仅「新主口令」显示强度条；当前主口令与确认新主口令不显示', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    await expandCollapseItem(wrapper, '修改主口令')
    const inputs = collapseContent(wrapper, '修改主口令').findAll('input')
    await inputs[0].setValue('旧口令')
    await flushPromises()
    expect(strengthMeters(wrapper).length).toBe(0)

    await inputs[1].setValue(PASS_WEAK)
    await flushPromises()
    expect(strengthMeters(wrapper).length).toBe(1)
    expect(strengthMeters(wrapper)[0].text()).toContain('弱')

    await inputs[2].setValue(PASS_WEAK)
    await flushPromises()
    expect(strengthMeters(wrapper).length).toBe(1)
  })

  it('其余口令输入框不出现强度条：关闭加密表单与启用自动解锁弹窗', async () => {
    stubInvoke({
      get_encryption_status: () => Promise.resolve(encryptedStatus),
      get_remember_passphrase_support: () => Promise.resolve({ supported: true }),
    })
    const wrapper = mount(EncryptionSettings)
    await flushPromises()

    // 关闭加密表单：输入已存在口令，不显示强度
    await expandCollapseItem(wrapper, '关闭加密')
    await collapseContent(wrapper, '关闭加密').find('input').setValue(PASS_VERY_STRONG)
    await flushPromises()
    expect(strengthMeters(wrapper).length).toBe(0)

    // 启用自动解锁小弹窗：输入当前主口令，不显示强度
    await findButton(wrapper, '启用自动解锁')!.trigger('click')
    await flushPromises()
    const modalInput = document.body.querySelector('.n-modal input[type="password"]')!
    await new DOMWrapper(modalInput).setValue(PASS_VERY_STRONG)
    await flushPromises()
    expect(strengthMeters(wrapper).length).toBe(0)
  })
})
