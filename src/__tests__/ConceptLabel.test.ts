import { describe, it, expect } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { applyLocale, t } from '@ledger/i18n'
import { setFakeMedia } from '@ledger/test-support/media-mock'
import { clickTipText, hoverTipText, tipOpen } from '@ledger/test-support/tooltip'
import { CONCEPT_KEYS } from '@/investment/concept-tips'
import ConceptLabel from '@/investment/ConceptLabel.vue'

// 投资域口径说明标签（issue #1369 / ADR-0088 决策 6）：本文件钉住三件事——
// ① 文案唯一源（渲染值独立经 t() 读出即等，硬编码第二份措辞即变红）；
// ② 双轴形态（指针轴悬停 tooltip、触控轴点按入注册表的气泡，热区 ≥48px）；
// ③ 作用域变体的拼接与缺省（删掉变体调用即变红）；④ 概念闭集成员都有两份文案。

describe('ConceptLabel 口径说明标签（issue #1369）', () => {
  it('标签与口径说明都取自 i18n 概念命名空间，不持第二份措辞', async () => {
    const wrapper = mount(ConceptLabel, {
      props: { label: t('investments.holdings.columns.cost'), concept: 'cost', testId: 'cost' },
    })
    // 标签逐字：ⓘ 与标签之间无空白节点（消费方 .text() 口径不变）
    expect(wrapper.text()).toBe(t('investments.holdings.columns.cost'))
    const trigger = wrapper.find('[data-testid="cost-info"]')
    expect(trigger.exists()).toBe(true)
    expect(trigger.attributes('aria-label')).toBe(
      t('investments.concepts.tipAria', { concept: t('investments.holdings.columns.cost') }),
    )
    // 口径说明 = 概念键现取（换键即换文案；写死文案则此处变红）
    expect(await hoverTipText(trigger)).toBe(t('investments.concepts.costTip'))
  })

  it('换概念键即换文案：同一挂点形态承载不同口径', async () => {
    const wrapper = mount(ConceptLabel, { props: { label: '现价', concept: 'price' } })
    expect(await hoverTipText(wrapper.find('button'))).toBe(t('investments.concepts.priceTip'))
    expect(t('investments.concepts.priceTip')).not.toBe(t('investments.concepts.costTip'))
  })

  it('作用域变体拼在概念文案后；不给变体就只有概念文案', async () => {
    for (const [scope, key] of [
      ['filtered', 'scopeFiltered'],
      ['wholeLedger', 'scopeWholeLedger'],
      ['crossBook', 'scopeCrossBook'],
    ] as const) {
      const wrapper = mount(ConceptLabel, {
        props: { label: '总市值', concept: 'marketValue', scope },
      })
      // 拼接走 i18n 模板（zh 无分隔、en 空格分隔），不在这里重写拼接规则
      expect(await hoverTipText(wrapper.find('button'))).toBe(
        t('investments.concepts.tipTemplate', {
          body: t('investments.concepts.marketValueTip'),
          scope: t(`investments.concepts.${key}`),
        }),
      )
      // 卸载即撤下已开启的 tooltip，避免上一变体的气泡泄入下一变体的断言
      wrapper.unmount()
    }
    const bare = mount(ConceptLabel, { props: { label: '总市值', concept: 'marketValue' } })
    expect(await hoverTipText(bare.find('button'))).toBe(t('investments.concepts.marketValueTip'))
    bare.unmount()
  })

  it('英文两轴都不丢分隔：句号相接处必须有空格（issue #1369 复核项）', async () => {
    await applyLocale('en-US')
    try {
      const wrapper = mount(ConceptLabel, {
        props: { label: 'Total Market Value', concept: 'marketValue', scope: 'filtered' },
      })
      const tip = await hoverTipText(wrapper.find('button'))
      // 概念句与作用域句都以句号收尾/开头：拼接缺分隔即渲染成 `included.Amounts`
      expect(tip).toMatch(/[.!?] [A-Z]/)
      expect(tip).not.toMatch(/[.!?][A-Za-z]/)
      wrapper.unmount()
    } finally {
      // 语言是模块级单例：还原，避免污染同进程其他测试
      await applyLocale('zh-CN')
    }
  })

  it('概念闭集每个成员在两份 locale 都有口径说明文案（漏写即渲染 key 原文）', async () => {
    for (const locale of ['zh-CN', 'en-US'] as const) {
      await applyLocale(locale)
      try {
        for (const key of CONCEPT_KEYS) {
          const text = t(`investments.concepts.${key}Tip`)
          expect(text, `${locale}/${key}`).not.toContain('investments.concepts')
          expect(text.length, `${locale}/${key}`).toBeGreaterThan(0)
        }
      } finally {
        await applyLocale('zh-CN')
      }
    }
  })

  it('触控轴：点按弹出经 AppPopover 的气泡，热区外扩到 ≥48px', async () => {
    setFakeMedia({ hover: 'none', pointer: 'coarse' })
    const wrapper = mount(ConceptLabel, {
      props: { label: '成本', concept: 'cost', testId: 'cost' },
    })
    await flushPromises()
    const trigger = wrapper.find('[data-testid="cost-info"]')
    // 触控轴不挂悬停触发器：形态换轴即换（指针轴的裸 NTooltip 不存在于此轴）
    expect(trigger.classes()).toContain('touch-hit-area')
    expect((trigger.element as HTMLElement).style.getPropertyValue('--touch-hit-inset')).toBe(
      '-10px -10px',
    )
    // 读屏替代在点按轴成立（指针轴零变化）：aria 含他看到的标签与该口径解释
    expect(trigger.attributes('aria-label')).toBe('成本说明')
    expect(tipOpen()).toBe(false)
    // 两轴同源：触控轴气泡文案与指针轴 tooltip 同一份概念文案
    expect(await clickTipText(trigger)).toBe(t('investments.concepts.costTip'))
  })
})
