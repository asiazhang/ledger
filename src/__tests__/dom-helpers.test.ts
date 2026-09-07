import { describe, it, expect, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { defineComponent, h, Teleport } from 'vue'
import { findButton, findButtonByTestId, findBodyButton, findBodyButtonByTestId, findInput } from './helpers/dom'

// enableAutoUnmount 幂等：setup 层已全局注册，文件级重复注册降级为 no-op（不抛错）
enableAutoUnmount(afterEach)

/** 触达三种按钮变体与输入家族的最小组件：teleport 按钮落在 document.body。 */
const Comp = defineComponent({
  name: 'DomProbe',
  setup() {
    return () =>
      h('div', [
        h('button', { 'data-testid': 'save' }, '保 存'),
        h('button', '取消'),
        h('button', '保存草稿'),
        h('input', { placeholder: '金额' }),
        h('input', { type: 'password' }),
        h('input'),
        h(Teleport, { to: 'body' }, [h('button', { 'data-testid': 'modal-ok' }, '确定')]),
      ])
  },
})

async function mountProbe() {
  const wrapper = mount(Comp)
  await flushPromises()
  return wrapper
}

describe('helpers/dom：按钮查找三变体 + findInput 家族（issue #746）', () => {
  it('findButton 文本匹配：默认包含匹配，exact 精确匹配', async () => {
    const wrapper = await mountProbe()
    expect(findButton(wrapper, '取消')?.text()).toBe('取消')
    // 包含匹配按 DOM 顺序命中第一个：'保存草稿' 先含 '保存'（'保 存' 带空格不含）
    expect(findButton(wrapper, '保存')?.text()).toBe('保存草稿')
    expect(findButton(wrapper, '保 存', { exact: true })?.text()).toBe('保 存')
    expect(findButton(wrapper, '取消', { exact: true })?.text()).toBe('取消')
    expect(findButton(wrapper, '不存在', { exact: true })).toBeUndefined()
  })

  it('findButtonByTestId：wrapper 范围内按 data-testid 查找', async () => {
    const wrapper = await mountProbe()
    expect(findButtonByTestId(wrapper, 'save').exists()).toBe(true)
    expect(findButtonByTestId(wrapper, 'missing').exists()).toBe(false)
  })

  it('findBodyButton：body-teleport 弹窗按钮按文本查找', async () => {
    await mountProbe()
    const ok = findBodyButton('确定')
    expect(ok).toBeDefined()
    expect((ok!.element as HTMLButtonElement).dataset.testid).toBe('modal-ok')
    expect(findBodyButton('取消')).toBeUndefined() // 只查 body 范围，不查 wrapper
  })

  it('findBodyButtonByTestId：body-teleport 弹窗按钮按 testid 查找', async () => {
    await mountProbe()
    expect((findBodyButtonByTestId('modal-ok')!.element as HTMLButtonElement).tagName).toBe('BUTTON')
    expect(findBodyButtonByTestId('missing')).toBeUndefined()
  })

  it('findInput 家族：裸 input / placeholder / type 三种形态', async () => {
    const wrapper = await mountProbe()
    expect((findInput(wrapper).element as HTMLInputElement).type).toBe('text')
    expect(findInput(wrapper, { placeholder: '金额' }).attributes('placeholder')).toBe('金额')
    expect((findInput(wrapper, { type: 'password' }).element as HTMLInputElement).type).toBe('password')
    expect(findInput(wrapper, { placeholder: '缺' }).exists()).toBe(false)
  })
})
