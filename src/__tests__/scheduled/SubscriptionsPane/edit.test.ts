import { describe, it, expect, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { NModal, NInputNumber, NDatePicker, NSelect, NTreeSelect } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import {
  mockDetails,
  makeDetail,
  makePlan,
  mockMerchants,
  findInput,
  mockInvoke,
  mountView,
  setFailSubscriptionUpdate,
  setMockMerchants,
  setMockPlans,
  setup,
} from './common'

beforeEach(setup)

describe('SubscriptionsPane 订阅编辑——仅非金额字段（issue #162）', () => {
  /** 按标题定位弹窗：页面有两个 NModal，findComponent 只返回第一个。 */
  function findModal(wrapper: ReturnType<typeof mount>, title: string) {
    const modal = wrapper
      .findAllComponents(NModal)
      .find((m) => m.props('title') === title)
    expect(modal, `应存在标题为「${title}」的弹窗`).toBeDefined()
    return modal!
  }

  /** 打开 a1 行的编辑弹窗。 */
  async function openEditModal(wrapper: ReturnType<typeof mount>) {
    await wrapper.find('[data-testid="op-edit-a1"]').trigger('click')
    await flushPromises()
  }

  it('进行中/已暂停行提供编辑入口，已取消行不提供', async () => {
    const plan = makePlan({ id: 'a1' })
    setMockPlans([plan, makePlan({ id: 'c1', status: 'cancelled', note: '已取消订阅' })])
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    expect(wrapper.find('[data-testid="op-edit-a1"]').exists()).toBe(true)
    // 已取消行不提供编辑（列表切到已取消后无编辑按钮）
    await wrapper.find('[data-testid="filter-cancelled"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="op-edit-c1"]').exists()).toBe(false)
  })

  it('编辑弹窗预填非金额字段且无金额输入', async () => {
    const plan = makePlan({
      id: 'a1',
      note: '视频会员',
      category_id: 'cat-1',
      account_id: 'acc-1',
      amount_cents: 1500,
    })
    setMockPlans([plan])
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    await openEditModal(wrapper)
    const modal = findModal(wrapper, '编辑订阅')
    expect(modal.props('show')).toBe(true)
    expect(modal.props('title')).toBe('编辑订阅')
    // 预填备注
    expect(findInput(wrapper, 'sub-edit-note').element.value).toBe('视频会员')
    // 无金额输入：无金额输入框、无数字步进（周期间隔）、无日期选择
    expect(wrapper.findComponent('[data-testid="sub-amount"]').exists()).toBe(false)
    expect(modal.findComponent(NInputNumber).exists()).toBe(false)
    expect(modal.findComponent(NDatePicker).exists()).toBe(false)
    // 弹窗内不出现计划金额
    expect(modal.text()).not.toContain('¥15')
  })

  it('未选账户时不提交编辑', async () => {
    const plan = makePlan({ id: 'a1' })
    setMockPlans([plan])
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    await openEditModal(wrapper)
    // 清空账户（编辑弹窗内唯一的 NSelect 是扣款账户）
    wrapper.findComponent(NSelect).vm.$emit('update:value', null)
    await flushPromises()
    await wrapper.findComponent('[data-testid="sub-edit-save"]').trigger('click')
    await flushPromises()
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'update_scheduled_subscription'),
    ).toBe(false)
  })

  it('提交编辑走订阅编辑命令，参数不含金额字段，成功后关闭弹窗并刷新清单', async () => {
    const plan = makePlan({ id: 'a1', note: '视频会员' }, 'mer-1')
    setMockPlans([plan])
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    await openEditModal(wrapper)
    const noteInput = findInput(wrapper, 'sub-edit-note')
    await noteInput.setValue('音乐会员')
    await noteInput.trigger('input')
    // 账户/分类经组件 emit（编辑弹窗打开时新建弹窗未渲染，实例唯一）
    wrapper.findComponent(NSelect).vm.$emit('update:value', 'acc-1')
    wrapper.findComponent(NTreeSelect).vm.$emit('update:value', 'cat-1')
    await flushPromises()
    await wrapper.findComponent('[data-testid="sub-edit-save"]').trigger('click')
    await flushPromises()
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'update_scheduled_subscription')
    expect(call).toBeDefined()
    expect(call![1]).toEqual({
      input: {
        id: 'a1',
        account_id: 'acc-1',
        category_id: 'cat-1',
        merchant_id: 'mer-1',
        note: '音乐会员',
      },
    })
    // 弹窗关闭且清单刷新（新备注出现在列表）
    expect(findModal(wrapper, '编辑订阅').props('show')).toBe(false)
    expect(wrapper.text()).toContain('音乐会员')
  })

  it('编辑弹窗改商户：预填当前商户，保存携带新 merchant_id（issue #190）', async () => {
    // 第二个在用商户：编辑目标从 mer-1 改为 mer-2
    setMockMerchants([
      ...mockMerchants,
      {
        id: 'mer-2',
        name: '商户B',
        updated_at: '2026-01-01T00:00:00Z',
        version: 1,
        device_id: 'test',
        is_deleted: false,
      },
    ])
    // beforeEach 已加载 store，显式 refresh 强制重拉拿新字典
    await useReferenceStore().refresh()
    const plan = makePlan({ id: 'a1', note: '视频会员' }, 'mer-1')
    setMockPlans([plan])
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    await openEditModal(wrapper)
    // 商户下拉 = 编辑弹窗内 data-testid 为 sub-edit-merchant 的 PinyinSelect（内部 NSelect）
    const merchantSelect = wrapper
      .findComponent('[data-testid="sub-edit-merchant"]')
      .findComponent(NSelect)
    expect(merchantSelect.exists()).toBe(true)
    expect(merchantSelect.props('value')).toBe('mer-1')
    merchantSelect.vm.$emit('update:value', 'mer-2')
    await flushPromises()
    await wrapper.findComponent('[data-testid="sub-edit-save"]').trigger('click')
    await flushPromises()
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'update_scheduled_subscription')
    expect(call![1]).toMatchObject({ input: { merchant_id: 'mer-2' } })
    // 清单商户列刷新为新商户名
    expect(wrapper.text()).toContain('商户B')
  })

  it('挂保单的缴费协议编辑弹窗不显示商户字段（issue #713 / ADR-0082：付款对象语义由保司承担）', async () => {
    const plan = { ...makePlan({ id: 'a1', note: '重疾险年缴' }), policy_id: 'policy-1' }
    setMockPlans([plan])
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    await openEditModal(wrapper)
    // 商户字段整个表单项不渲染（而非置灰）：协议计划行不挂商户
    expect(
      wrapper.findComponent('[data-testid="sub-edit-merchant"]').exists(),
    ).toBe(false)
  })

  it('原商户软删且不在字典：下拉兜底选项承载原 id，未改动提交仍携带原 id（接缝软删兜底分支接线）', async () => {
    const plan = makePlan({ id: 'a1', note: '视频会员' }, 'mer-gone')
    setMockPlans([plan])
    mockDetails.set('a1', makeDetail(plan, []))
    // 商户字典为空：原商户已软删且超出会话缓存
    setMockMerchants([])
    await useReferenceStore().refresh()
    const wrapper = await mountView()
    await openEditModal(wrapper)
    const merchantSelect = wrapper
      .findComponent('[data-testid="sub-edit-merchant"]')
      .findComponent(NSelect)
    expect(merchantSelect.props('value')).toBe('mer-gone')
    // 兜底选项承载原 id（裸 uuid 不可读，以可读标签显示）
    const options = merchantSelect.props('options') as { value: string }[]
    expect(options.some((o) => o.value === 'mer-gone')).toBe(true)
    await wrapper.findComponent('[data-testid="sub-edit-save"]').trigger('click')
    await flushPromises()
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'update_scheduled_subscription')
    expect(call).toBeDefined()
    expect(call![1]).toMatchObject({ input: { merchant_id: 'mer-gone' } })
  })

  it('提交失败时弹窗保持打开', async () => {
    const plan = makePlan({ id: 'a1' })
    setMockPlans([plan])
    mockDetails.set('a1', makeDetail(plan, []))
    const wrapper = await mountView()
    setFailSubscriptionUpdate(true)
    await openEditModal(wrapper)
    await wrapper.findComponent('[data-testid="sub-edit-save"]').trigger('click')
    await flushPromises()
    expect(findModal(wrapper, '编辑订阅').props('show')).toBe(true)
  })
})
