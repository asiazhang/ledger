import { describe, it, expect } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { mockInvoke } from '@ledger/test-support/invoke-mock'
import { applyLocale, t } from '@ledger/i18n'

import FeatureToggleSettings from '@/components/settings/FeatureToggleSettings.vue'
import {
  CLOSABLE_FEATURES,
  NON_CLOSABLE_FEATURES,
  useFeatureToggleStore,
} from '@/stores/feature-toggles'
import { getSavedClosedFeatures } from '@/utils/view-state'

// 设置页「功能」Tab 内容（issue #1243 / ADR-0116 决策 2/7/8）：九项可关功能各一行
//（功能名 + 一句简介 + NSwitch），不可关六项不出现（也不出现空行）。
// 关闭只隐藏入口（ADR-0116 决策 1）——「关闭后什么仍然在跑」由各条简介明示，
// 不设确认与拦截（决策 8）；状态走 feature-toggles store（localStorage，零后端调用）。

/** 定位某项功能行（行容器带 data-testid=feature-toggle-<id>）。 */
function featureRow(wrapper: ReturnType<typeof mount>, id: string) {
  return wrapper.find(`[data-testid="feature-toggle-${id}"]`)
}

/** 定位某项功能行的开关。 */
function featureSwitch(wrapper: ReturnType<typeof mount>, id: string) {
  return featureRow(wrapper, id).find('.n-switch')
}

describe('FeatureToggleSettings.vue 九项一行（issue #1243 范围 2）', () => {
  it('恰好九行，行序 = CLOSABLE_FEATURES 清单序，每行都有功能名、一句简介与一个开关', () => {
    const wrapper = mount(FeatureToggleSettings)
    const rows = wrapper.findAll('[data-testid^="feature-toggle-"]')
    expect(rows).toHaveLength(9)
    expect(rows.map((r) => r.attributes('data-testid'))).toEqual(
      CLOSABLE_FEATURES.map((id) => `feature-toggle-${id}`),
    )
    for (const id of CLOSABLE_FEATURES) {
      const row = featureRow(wrapper, id)
      expect(row.find('.n-switch').exists(), `${id} 行应有开关`).toBe(true)
      // 功能名与简介分两行文本（简介键 settings.features.descriptions.<id>）。
      expect(row.text()).toContain(t(`settings.features.descriptions.${id}`))
    }
    expect(wrapper.findAll('.n-switch')).toHaveLength(9)
  })

  it('功能名与侧栏视图名同源：预算、报表、定时、商户、投资、物品、保单、实物资产、保险公司', () => {
    const wrapper = mount(FeatureToggleSettings)
    const names = CLOSABLE_FEATURES.map((id) => featureRow(wrapper, id).find('.n-text').text())
    expect(names).toEqual([
      '预算',
      '报表',
      '定时',
      '商户',
      '投资',
      '物品',
      '保单',
      '实物资产',
      '保险公司',
    ])
  })

  it('不可关六项不出现：无对应功能行，且全页开关恰好九个', () => {
    const wrapper = mount(FeatureToggleSettings)
    for (const id of NON_CLOSABLE_FEATURES) {
      expect(featureRow(wrapper, id).exists(), `不可关项 ${id} 不应出现`).toBe(false)
    }
    expect(wrapper.findAll('.n-switch')).toHaveLength(9)
  })

  it('九条简介都写明「关闭后什么仍然在跑」；定时那条写明自动执行照旧、已有计划照常落账', () => {
    for (const id of CLOSABLE_FEATURES) {
      expect(t(`settings.features.descriptions.${id}`)).toMatch(/^关闭只隐藏入口/)
    }
    const scheduled = t('settings.features.descriptions.scheduled')
    expect(scheduled).toContain('自动执行')
    expect(scheduled).toContain('落账')
  })
})

describe('FeatureToggleSettings.vue 开关行为（issue #1243 范围 5：不设确认、不设拦截）', () => {
  it('初始默认全开：九个开关 aria-checked 均为 true', () => {
    const wrapper = mount(FeatureToggleSettings)
    for (const id of CLOSABLE_FEATURES) {
      expect(featureSwitch(wrapper, id).attributes('aria-checked')).toBe('true')
    }
  })

  it('点选即关：开关变关、store 记入关闭集合、localStorage 落地（无确认弹窗）', async () => {
    const store = useFeatureToggleStore()
    const wrapper = mount(FeatureToggleSettings)
    await featureSwitch(wrapper, 'budget').trigger('click')
    await nextTick()
    expect(featureSwitch(wrapper, 'budget').attributes('aria-checked')).toBe('false')
    expect(store.isFeatureClosed('budget')).toBe(true)
    expect(getSavedClosedFeatures()).toEqual(['budget'])
  })

  it('再点即开：关闭记录清除，回默认全开', async () => {
    const store = useFeatureToggleStore()
    const wrapper = mount(FeatureToggleSettings)
    await featureSwitch(wrapper, 'budget').trigger('click')
    await nextTick()
    await featureSwitch(wrapper, 'budget').trigger('click')
    await nextTick()
    expect(featureSwitch(wrapper, 'budget').attributes('aria-checked')).toBe('true')
    expect(store.isFeatureClosed('budget')).toBe(false)
    expect(getSavedClosedFeatures()).toBeNull()
  })

  it('全程零后端调用：九个开关全部拨动后 mockInvoke 仍无任何调用', async () => {
    const wrapper = mount(FeatureToggleSettings)
    for (const id of CLOSABLE_FEATURES) {
      await featureSwitch(wrapper, id).trigger('click')
      await nextTick()
    }
    expect(mockInvoke).not.toHaveBeenCalled()
  })

  it('开关状态跨启动保持：关闭两项后新一次启动仍只有这两项关着', async () => {
    const wrapper = mount(FeatureToggleSettings)
    await featureSwitch(wrapper, 'budget').trigger('click')
    await nextTick()
    await featureSwitch(wrapper, 'investments').trigger('click')
    await nextTick()
    expect(getSavedClosedFeatures()).toEqual(['budget', 'investments'])

    // 「重启」= 新 Pinia：store 首次实例化即重跑启动读路径（localStorage 未清）。
    setActivePinia(createPinia())
    const restarted = mount(FeatureToggleSettings)
    for (const id of CLOSABLE_FEATURES) {
      const expected = id === 'budget' || id === 'investments' ? 'false' : 'true'
      expect(featureSwitch(restarted, id).attributes('aria-checked'), `${id} 开关态`).toBe(expected)
    }
  })

  it('英文界面：卡片标题、功能名与九条简介均为英文文案', async () => {
    await applyLocale('en-US')
    let html = ''
    try {
      const wrapper = mount(FeatureToggleSettings)
      html = wrapper.html()
      expect(html).toContain('Feature Toggles')
      expect(html).toContain('Physical Assets')
      expect(html).toContain('Closing only hides the entry')
      expect(t('settings.features.descriptions.scheduled')).toContain('automatic execution')
    } finally {
      await applyLocale('zh-CN')
      await nextTick()
    }
  })
})
