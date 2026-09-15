import { describe, it, expect } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { t } from '@ledger/i18n'
import { setFakeMedia } from '@ledger/test-support/media-mock'
import ConceptLabel from '@/investment/ConceptLabel.vue'

// 投资域口径说明标签（issue #1369 / ADR-0088 决策 6）：本文件钉住三件事——
// ① 文案唯一源（渲染值独立经 t() 读出即等，硬编码第二份措辞即变红）；
// ② 双轴形态（指针轴悬停 tooltip、触控轴点按入注册表的气泡，热区 ≥48px）；
// ③ 作用域变体必填（缺省即无作用域句——删掉变体调用即变红）。

/** 悬停出 tooltip：NTooltip 默认 100ms 防误触延迟，jsdom 等真实时钟 */
async function hoverTip(trigger: { trigger: (e: string) => Promise<void> }): Promise<string> {
  await trigger.trigger('mouseenter')
  await new Promise((r) => setTimeout(r, 200))
  await flushPromises()
  // 气泡体在 {{ tip }} 两侧带模板空白，取文案本体比较
  return (document.body.querySelector('.n-popover')?.textContent ?? '').trim()
}

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
    expect(await hoverTip(trigger)).toBe(t('investments.concepts.costTip'))
  })

  it('换概念键即换文案：同一挂点形态承载不同口径', async () => {
    const wrapper = mount(ConceptLabel, { props: { label: '现价', concept: 'price' } })
    expect(await hoverTip(wrapper.find('button'))).toBe(t('investments.concepts.priceTip'))
    expect(t('investments.concepts.priceTip')).not.toBe(t('investments.concepts.costTip'))
  })

  it('作用域变体拼在概念文案后；不给变体就只有概念文案', async () => {
    for (const [scope, key] of [
      ['filtered', 'scopeFiltered'],
      ['wholeLedger', 'scopeWholeLedger'],
      ['crossBook', 'scopeCrossBook'],
      ['mwr', 'scopeMwr'],
    ] as const) {
      const wrapper = mount(ConceptLabel, {
        props: { label: '总市值', concept: 'marketValue', scope },
      })
      expect(await hoverTip(wrapper.find('button'))).toBe(
        `${t('investments.concepts.marketValueTip')}${t(`investments.concepts.${key}`)}`,
      )
      // 卸载即撤下已开启的 tooltip，避免上一变体的气泡泄入下一变体的断言
      wrapper.unmount()
    }
    const bare = mount(ConceptLabel, { props: { label: '总市值', concept: 'marketValue' } })
    expect(await hoverTip(bare.find('button'))).toBe(t('investments.concepts.marketValueTip'))
    bare.unmount()
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
    expect(document.body.querySelector('.n-popover')).toBeNull()
    await trigger.trigger('click')
    await flushPromises()
    // 两轴同源：触控轴气泡文案与指针轴 tooltip 同一份概念文案
    expect(document.body.querySelector('.n-popover')?.textContent?.trim()).toBe(
      t('investments.concepts.costTip'),
    )
  })
})
