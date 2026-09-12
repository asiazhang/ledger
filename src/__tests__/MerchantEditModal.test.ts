import { describe, it, expect } from 'vitest'
import { mockInvoke, wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { messageApi } from '@ledger/test-support/message-mock'
import { findBodyButton } from '@ledger/test-support/dom'
import { DOMWrapper, mount, flushPromises } from '@vue/test-utils'
import MerchantEditModal from '@/components/merchants/MerchantEditModal.vue'
import type { Merchant } from '@ledger/types'

const mockMerchant: Merchant = {
  id: 'mch-1', name: '京东',
  updated_at: '2026-01-01T00:00:00Z',
  version: 1, device_id: 'test', is_deleted: false,
}

/** NModal 内容传送至 document.body：直接在 body 上查询。 */
function findInputByPlaceholder(placeholder: string): DOMWrapper<HTMLInputElement> {
  const el = document.body.querySelector(`input[placeholder="${placeholder}"]`)
  expect(el, `input[placeholder=${placeholder}] 应存在`).not.toBeNull()
  return new DOMWrapper(el as HTMLInputElement)
}

function findButtonByText(text: string): DOMWrapper<HTMLButtonElement> {
  const btn = findBodyButton(text, { exact: true })
  expect(btn, `button「${text}」应存在`).not.toBeNull()
  return btn!
}

describe('MerchantEditModal.vue（issue #189）', () => {

  it('打开时回填商户名称（表单只含名称输入，icon/color 已退役）', async () => {
    mount(MerchantEditModal, {
      props: { show: true, merchant: mockMerchant },
    })
    await flushPromises()
    const nameInput = findInputByPlaceholder('商户名称')
    expect(nameInput.element.value).toBe('京东')
    expect(document.body.querySelector('input[placeholder="图标名"]')).toBeNull()
    expect(document.body.querySelector('input[placeholder="颜色"]')).toBeNull()
  })

  it('改名保存：调用 update_merchant 并关窗', async () => {
    wireInvokeSeam({
      overrides: { update_merchant: () => Promise.resolve(undefined) },
    })
    const wrapper = mount(MerchantEditModal, {
      props: { show: true, merchant: mockMerchant },
    })
    await flushPromises()
    await findInputByPlaceholder('商户名称').setValue('京东商城')
    const saveBtn = findButtonByText('保存')
    await saveBtn.trigger('click')
    await flushPromises()

    expect(mockInvoke.mock.calls.filter(([c]) => c === 'update_merchant')).toHaveLength(1)
    expect(mockInvoke.mock.calls.find(([c]) => c === 'update_merchant')![1]).toEqual({
      id: 'mch-1',
      input: { name: '京东商城' },
    })
    expect(wrapper.emitted('update:show')?.[0]).toEqual([false])
  })

  it('空名称保存被拦截（不调用 update_merchant）', async () => {
    mount(MerchantEditModal, {
      props: { show: true, merchant: mockMerchant },
    })
    await flushPromises()
    await findInputByPlaceholder('商户名称').setValue('')
    const saveBtn = findButtonByText('保存')
    await saveBtn.trigger('click')
    await flushPromises()

    expect(mockInvoke.mock.calls.filter(([c]) => c === 'update_merchant')).toHaveLength(0)
    expect(messageApi.warning).toHaveBeenCalled()
  })

  it('改名撞名：显示可理解的错误提示，弹窗不关', async () => {
    wireInvokeSeam({
      overrides: {
        update_merchant: () =>
          Promise.reject(new Error('参数错误: 商户已存在: 京东商城')),
      },
    })
    const wrapper = mount(MerchantEditModal, {
      props: { show: true, merchant: mockMerchant },
    })
    await flushPromises()
    await findInputByPlaceholder('商户名称').setValue('京东商城')
    const saveBtn = findButtonByText('保存')
    await saveBtn.trigger('click')
    await flushPromises()

    expect(messageApi.error).toHaveBeenCalledWith('更新失败: Error: 参数错误: 商户已存在: 京东商城')
    expect(wrapper.emitted('update:show')).toBeUndefined()
  })
})

