import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount } from '@vue/test-utils'

// 语言判定/切换逻辑（issue #342 / ADR-0049）：只测外显行为——判定链优先级、
// 覆盖持久化、切换信号传播（currentLocale 与 t() 输出）、懒加载触发、
// 未翻译回退与 dev 缺失告警。每个用例经 vi.resetModules 取全新模块状态，
// 避免单例 i18n 实例跨用例串扰。
function stubNavigatorLanguage(lang: string) {
  Object.defineProperty(window.navigator, 'language', {
    value: lang,
    configurable: true,
  })
}

async function freshI18n() {
  vi.resetModules()
  return await import('@/i18n')
}

beforeEach(() => {
  localStorage.clear()
  stubNavigatorLanguage('zh-CN')
})

afterEach(() => {
  vi.restoreAllMocks()
})

describe('语言判定链：手动覆盖 > 系统语言 > zh-CN', () => {
  it('系统语言为英文且无手动覆盖时，初始化进入英文界面', async () => {
    stubNavigatorLanguage('en-US')
    const { initAppLocale, currentLocale, t } = await freshI18n()
    await initAppLocale()
    expect(currentLocale.value).toBe('en-US')
    expect(t('common.language.followSystem')).toBe('Follow System')
  })

  it('手动覆盖为中文时优先于英文系统语言', async () => {
    stubNavigatorLanguage('en-US')
    localStorage.setItem('locale', JSON.stringify('zh-CN'))
    const { initAppLocale, currentLocale } = await freshI18n()
    await initAppLocale()
    expect(currentLocale.value).toBe('zh-CN')
  })

  it('系统语言非英文且无覆盖时，回退中文（源语言兜底）', async () => {
    stubNavigatorLanguage('ja-JP')
    const { initAppLocale, currentLocale } = await freshI18n()
    await initAppLocale()
    expect(currentLocale.value).toBe('zh-CN')
  })
})

describe('覆盖持久化与切换即时生效', () => {
  it('setLocaleSetting 写入 localStorage 并即时生效（t() 输出与 currentLocale 同步变化）', async () => {
    const { setLocaleSetting, currentLocale, t } = await freshI18n()
    expect(t('common.language.label')).toBe('界面语言')
    await setLocaleSetting('en-US')
    expect(localStorage.getItem('locale')).toBe(JSON.stringify('en-US'))
    expect(currentLocale.value).toBe('en-US')
    expect(t('common.language.label')).toBe('Language')
  })

  it('清除手动覆盖（回落 system）后跟随系统语言', async () => {
    stubNavigatorLanguage('en-US')
    const { setLocaleSetting, currentLocale } = await freshI18n()
    await setLocaleSetting('zh-CN')
    expect(currentLocale.value).toBe('zh-CN')
    await setLocaleSetting('system')
    expect(localStorage.getItem('locale')).toBe(JSON.stringify('system'))
    expect(currentLocale.value).toBe('en-US')
  })
})

describe('懒加载与回退', () => {
  it('非当前 locale 文案包初始不加载，切换时才拉取', async () => {
    const mod = await freshI18n()
    expect(mod.i18n.global.availableLocales).toEqual(['zh-CN'])
    await mod.applyLocale('en-US')
    expect(mod.i18n.global.availableLocales).toContain('en-US')
  })

  it('未翻译 key 回退显示中文原文而非 key 代号', async () => {
    const { i18n, applyLocale, t } = await freshI18n()
    i18n.global.mergeLocaleMessage('zh-CN', { common: { zhOnly: '仅中文的文案' } })
    await applyLocale('en-US')
    expect(t('common.zhOnly')).toBe('仅中文的文案')
  })

  it('开发模式下当前 locale 缺失 key 打控制台告警', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const { t } = await freshI18n()
    t('common.definitely.missing')
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('common.definitely.missing'))
  })
})

describe('组件渲染随语言切换即时变化（外显行为）', () => {
  it('切换语言后已挂载组件的文案随当前语言渲染', async () => {
    const { applyLocale, t } = await freshI18n()
    // 组件与业务代码同一消费方式：引用模块级 t() 做渲染输出
    const LangText = defineComponent({
      setup() {
        return () => h('span', t('common.language.label'))
      },
    })
    const wrapper = mount(LangText)
    expect(wrapper.text()).toBe('界面语言')
    await applyLocale('en-US')
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).toBe('Language')
    await applyLocale('zh-CN')
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).toBe('界面语言')
  })
})
