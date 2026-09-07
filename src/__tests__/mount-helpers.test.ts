import { describe, it, expect } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { useDialog } from 'naive-ui'
import { mountFlushed, mountWithDialog } from './helpers/mount'

/** 最小内容探针：文本断言用。 */
const Probe = defineComponent({
  name: 'MountProbe',
  setup() {
    return () => h('p', '探针内容')
  },
})

describe('helpers/mount：mount+flush 一体与 NDialogProvider 包裹（issue #748）', () => {
  it('mountFlushed：挂载并冲刷异步链后返回 wrapper', async () => {
    const wrapper = await mountFlushed(Probe)
    expect(wrapper.text()).toContain('探针内容')
  })

  it('mountFlushed 透传挂载 options（attrs 落根元素）', async () => {
    const wrapper = await mountFlushed(Probe, { attrs: { 'data-probe': 'on' } })
    expect(wrapper.attributes('data-probe')).toBe('on')
  })

  it('mountWithDialog：子组件内 useDialog 可用（NDialogProvider 上下文就位）', async () => {
    const DialogProbe = defineComponent({
      name: 'DialogContextProbe',
      setup() {
        const dialog = useDialog()
        return () =>
          h('button', { onClick: () => dialog.info({ content: '上下文可用' }) }, '开')
      },
    })
    const wrapper = mountWithDialog(DialogProbe)
    await wrapper.find('button').trigger('click')
    await flushPromises()
    expect(document.body.querySelector('.n-dialog')).not.toBeNull()
  })
})
