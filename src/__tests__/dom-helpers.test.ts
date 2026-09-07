import { describe, it, expect, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { defineComponent, h, ref, Teleport } from 'vue'
import { NDialogProvider, NModal, useDialog } from 'naive-ui'
import {
  findButton,
  findButtonByTestId,
  findBodyButton,
  findBodyButtonByTestId,
  findInput,
  findInputByTestId,
  clickDialogButton,
  dialogText,
  pressReleaseOnDialogMask,
  visibleModalText,
} from './helpers/dom'

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

// —— 弹窗可见性助手（issue #748 上收：原 TransactionsView/common 目录级实现） ——

/** testid 载体组件：根元素携带 data-testid、内含一个输入框（NInput 包裹形态）。 */
const FieldInput = defineComponent({
  name: 'FieldInput',
  props: { testid: { type: String, required: true } },
  setup(props) {
    return () => h('div', { 'data-testid': props.testid }, [h('input')])
  },
})

const InputProbe = defineComponent({
  name: 'InputProbe',
  setup() {
    return () => h('div', [h(FieldInput, { testid: 'field-note' })])
  },
})

/** useDialog 确认框 + NModal 卡片双探针：遮罩不可关（同视图删除确认配置）。 */
const DialogProbe = defineComponent({
  name: 'DialogProbe',
  setup() {
    const dialog = useDialog()
    const showCard = ref(false)
    return () => [
      h(
        'button',
        {
          'data-testid': 'open-dialog',
          onClick: () =>
            dialog.warning({
              title: '删除确认',
              content: '删除后不可恢复',
              positiveText: '删除',
              negativeText: '取消',
              maskClosable: false,
            }),
        },
        '打开确认框',
      ),
      h(
        'button',
        { 'data-testid': 'open-card', onClick: () => (showCard.value = true) },
        '打开卡片',
      ),
      h(
        NModal,
        {
          show: showCard.value,
          'onUpdate:show': (v: boolean) => (showCard.value = v),
          preset: 'card',
          title: '卡片弹窗',
        },
        { default: () => h('p', '卡片内容文本') },
      ),
    ]
  },
})

async function mountDialogProbe() {
  const wrapper = mount(NDialogProvider, { slots: { default: () => h(DialogProbe) } })
  await flushPromises()
  await wrapper.find('[data-testid="open-dialog"]').trigger('click')
  await flushPromises()
  return wrapper
}

describe('helpers/dom：弹窗可见性助手（issue #748）', () => {
  it('dialogText：useDialog 确认框（teleport 到 body）的可见文本', async () => {
    await mountDialogProbe()
    expect(dialogText()).toContain('删除后不可恢复')
  })

  it('clickDialogButton：按文案点确认框按钮，取消后确认框关闭', async () => {
    await mountDialogProbe()
    await clickDialogButton('取消')
    await flushPromises()
    expect(dialogText()).toBe('')
  })

  it('pressReleaseOnDialogMask：遮罩按下-抬起完整事件序列派发（maskClosable=false 不关闭）', async () => {
    await mountDialogProbe()
    await pressReleaseOnDialogMask()
    await flushPromises()
    expect(dialogText()).toContain('删除后不可恢复')
  })

  it('visibleModalText：NModal 卡片（teleport 到 body）的可见文本', async () => {
    const wrapper = await mountDialogProbe()
    expect(visibleModalText()).toBe('')
    await wrapper.find('[data-testid="open-card"]').trigger('click')
    await flushPromises()
    expect(visibleModalText()).toContain('卡片内容文本')
  })
})

/** 输入查找探针挂载（独立 describe 使用）。 */
async function mountInputProbe() {
  const wrapper = mount(InputProbe)
  await flushPromises()
  return wrapper
}

describe('helpers/dom：findInputByTestId（issue #748）', () => {
  it('经 testid 载体组件锚定输入框', async () => {
    const wrapper = await mountInputProbe()
    expect((findInputByTestId(wrapper, 'field-note').element as HTMLInputElement).type).toBe('text')
    expect(findInputByTestId(wrapper, 'field-missing').exists()).toBe(false)
  })
})
