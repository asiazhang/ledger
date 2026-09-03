import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import MerchantManager from '@/components/MerchantManager.vue'
import type { Merchant } from '@/types'

const { messageMock, pushMock } = vi.hoisted(() => ({
  messageMock: {
    success: vi.fn(),
    warning: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
    loading: vi.fn(),
    destroyAll: vi.fn(),
  },
  pushMock: vi.fn(),
}))

// 条数下钻（issue #446）经 useRouter 跳转（pushMock 断言导航目标，同 AccountLink 先例）
vi.mock('vue-router', () => ({
  useRouter: () => ({ push: pushMock }),
}))

// 覆盖 setup.ts 的全局 naive-ui mock：message 实例可断言（重名错误提示等）
vi.mock('naive-ui', async (importOriginal) => {
  const actual = await importOriginal<typeof import('naive-ui')>()
  return {
    ...actual,
    useMessage: () => messageMock,
  }
})

const mockInvoke = vi.mocked(invoke)

const mockMerchants: Merchant[] = [
  {
    id: 'mch-1', name: '京东',
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
  {
    id: 'mch-2', name: '红旗连锁',
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
]

let merchantDb: Merchant[] = mockMerchants

/** 关联交易计数后端响应（issue #445，毛笔数口径）：可缺行（无引用商户前端补 0）。 */
let countDb: { merchant_id: string; transaction_count: number }[] = []

function mockBaseCommands() {
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve([])
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve(merchantDb)
    if (cmd === 'list_merchant_transaction_counts') return Promise.resolve(countDb)
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

function merchantCalls(cmd: string) {
  return mockInvoke.mock.calls.filter(([c]) => c === cmd)
}

describe('MerchantManager.vue（issue #189）', () => {
  beforeEach(async () => {
    setActivePinia(createPinia())
    mockInvoke.mockReset()
    merchantDb = mockMerchants
    countDb = []
    mockBaseCommands()
    messageMock.success.mockClear()
    messageMock.error.mockClear()
    messageMock.warning.mockClear()
    const store = useReferenceStore()
    await store.refresh()
  })

  it('挂载并渲染商户列表', () => {
    const wrapper = mount(MerchantManager)
    expect(wrapper.text()).toContain('京东')
    expect(wrapper.text()).toContain('红旗连锁')
    expect(wrapper.text()).toContain('新增商户')
  })

  it('空名称不调用 create_merchant', async () => {
    const wrapper = mount(MerchantManager)
    const addBtn = wrapper.findAll('button').find((b) => b.text() === '添加')!
    await addBtn.trigger('click')
    expect(merchantCalls('create_merchant')).toHaveLength(0)
    expect(messageMock.warning).toHaveBeenCalled()
  })

  it('添加商户：调用 create_merchant，重拉后列表出现新商户', async () => {
    mockInvoke.mockImplementation((cmd: string, args?: { input?: { name: string } }) => {
      if (cmd === 'create_merchant') {
        merchantDb = [
          ...merchantDb,
          {
            id: 'mch-new', name: args!.input!.name,
            created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
            version: 1, device_id: 'test', is_deleted: false,
          },
        ]
        return Promise.resolve('mch-new')
      }
      if (cmd === 'list_merchants') return Promise.resolve(merchantDb)
      if (cmd === 'list_currencies') return Promise.resolve([])
      if (cmd === 'list_accounts') return Promise.resolve([])
      if (cmd === 'list_categories') return Promise.resolve([])
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = mount(MerchantManager)
    const nameInput = wrapper.findAll('input').find((i) => i.attributes('placeholder') === '商户名称')!
    await nameInput.setValue('盒马')
    const addBtn = wrapper.findAll('button').find((b) => b.text() === '添加')!
    await addBtn.trigger('click')
    await flushPromises()

    expect(merchantCalls('create_merchant')).toHaveLength(1)
    expect(merchantCalls('create_merchant')[0][1]).toEqual({ input: { name: '盒马' } })
    expect(messageMock.success).toHaveBeenCalled()
    // 表单清空
    expect(nameInput.element.value).toBe('')
    // 重拉后列表出现新商户（store 由失效信号驱动，测试中手动 refresh 模拟）
    await useReferenceStore().refresh()
    expect(wrapper.text()).toContain('盒马')
  })

  it('重名创建失败：显示可理解的错误提示，表单不清空', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'create_merchant') {
        return Promise.reject(new Error('参数错误: 商户已存在: 盒马'))
      }
      if (cmd === 'list_merchants') return Promise.resolve(merchantDb)
      if (cmd === 'list_currencies') return Promise.resolve([])
      if (cmd === 'list_accounts') return Promise.resolve([])
      if (cmd === 'list_categories') return Promise.resolve([])
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = mount(MerchantManager)
    const nameInput = wrapper.findAll('input').find((i) => i.attributes('placeholder') === '商户名称')!
    await nameInput.setValue('盒马')
    const addBtn = wrapper.findAll('button').find((b) => b.text() === '添加')!
    await addBtn.trigger('click')
    await flushPromises()

    expect(messageMock.error).toHaveBeenCalledWith('添加失败: Error: 参数错误: 商户已存在: 盒马')
    // 表单不清空，用户可直接修正
    expect(nameInput.element.value).toBe('盒马')
  })

  it('每行有编辑与删除入口', () => {
    const wrapper = mount(MerchantManager)
    const editBtns = wrapper.findAll('button').filter((b) => b.text() === '编辑')
    const deleteBtns = wrapper.findAll('button').filter((b) => b.text() === '删除')
    expect(editBtns.length).toBe(2)
    expect(deleteBtns.length).toBe(2)
  })
})

describe('MerchantManager.vue 关联交易条数列（issue #445，毛笔数口径）', () => {
  beforeEach(async () => {
    setActivePinia(createPinia())
    mockInvoke.mockReset()
    merchantDb = mockMerchants
    countDb = []
    mockBaseCommands()
    const store = useReferenceStore()
    await store.refresh()
  })

  function rowTexts(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('tbody tr').map((r) => r.text())
  }

  it('每行显示关联交易条数；计数缺失的无引用商户显示 0', async () => {
    countDb = [{ merchant_id: 'mch-1', transaction_count: 2 }]
    const wrapper = mount(MerchantManager)
    await flushPromises()
    const rows = rowTexts(wrapper)
    expect(rows[0]).toContain('京东')
    expect(rows[0]).toContain('2')
    expect(rows[1]).toContain('红旗连锁')
    expect(rows[1]).toContain('0')
  })

  it('条数列展示走数字分组口径（数量列与金额列同一核心助手）', async () => {
    countDb = [{ merchant_id: 'mch-1', transaction_count: 12345 }]
    const wrapper = mount(MerchantManager)
    await flushPromises()
    expect(rowTexts(wrapper)[0]).toContain('1,2345')
  })

  it('点击条数列表头可按条数排序（升/降序往返，不影响名称列自然序初始态）', async () => {
    countDb = [
      { merchant_id: 'mch-1', transaction_count: 1 },
      { merchant_id: 'mch-2', transaction_count: 5 },
    ]
    const wrapper = mount(MerchantManager)
    await flushPromises()
    // 初始自然序（与后端名称序一致）：京东在前
    expect(rowTexts(wrapper)[0]).toContain('京东')

    const sorter = wrapper.find('th.n-data-table-th--sortable')
    expect(sorter.exists()).toBe(true)
    await sorter.trigger('click')
    await flushPromises()
    // 第一次点击降序：条数多的红旗连锁（5）在前
    expect(rowTexts(wrapper)[0]).toContain('红旗连锁')

    await sorter.trigger('click')
    await flushPromises()
    // 第二次点击升序：条数少的京东（1）回到最前
    expect(rowTexts(wrapper)[0]).toContain('京东')
  })

  it('参考数据重拉后条数随之更新（既有失效机制，经 store version 伴随重拉计数）', async () => {
    countDb = [{ merchant_id: 'mch-1', transaction_count: 2 }]
    const wrapper = mount(MerchantManager)
    await flushPromises()
    expect(rowTexts(wrapper)[0]).toContain('2')

    const callsBefore = merchantCalls('list_merchant_transaction_counts').length
    countDb = [{ merchant_id: 'mch-1', transaction_count: 3 }]
    await useReferenceStore().refresh()
    await flushPromises()
    expect(rowTexts(wrapper)[0]).toContain('3')
    expect(merchantCalls('list_merchant_transaction_counts').length).toBeGreaterThan(callsBefore)
  })
})

describe('MerchantManager.vue 条数下钻（issue #446）', () => {
  beforeEach(async () => {
    setActivePinia(createPinia())
    mockInvoke.mockReset()
    merchantDb = mockMerchants
    countDb = []
    mockBaseCommands()
    pushMock.mockReset()
    const store = useReferenceStore()
    await store.refresh()
  })

  /** 条数下钻按钮：每行的条数单元格按钮（title 与 MerchantLink 同源，用作语义定位）。 */
  function countButtons(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAll('tbody tr')
      .map((r) => r.findAll('button').find((b) => b.attributes('title') === '查看该商户的交易'))
  }

  it('条数渲染为可点击按钮并带 title 提示（与 MerchantLink 同一口径）', async () => {
    countDb = [{ merchant_id: 'mch-1', transaction_count: 2 }]
    const wrapper = mount(MerchantManager)
    await flushPromises()
    const btn = countButtons(wrapper)[0]!
    expect(btn.exists()).toBe(true)
    expect(btn.text()).toBe('2')
  })

  it('点击条数跳转交易列表，URL 携带该商户过滤参数（既有 URL 下钻机制）', async () => {
    countDb = [
      { merchant_id: 'mch-1', transaction_count: 2 },
      { merchant_id: 'mch-2', transaction_count: 5 },
    ]
    const wrapper = mount(MerchantManager)
    await flushPromises()
    const btns = countButtons(wrapper)

    await btns[0]!.trigger('click')
    expect(pushMock).toHaveBeenCalledTimes(1)
    expect(pushMock).toHaveBeenCalledWith({ name: 'transactions', query: { merchant: 'mch-1' } })

    await btns[1]!.trigger('click')
    expect(pushMock).toHaveBeenCalledWith({ name: 'transactions', query: { merchant: 'mch-2' } })
  })

  it('条数为 0 同样可下钻（跳转不按条数/商户状态设门；空列表行为归 TransactionFilter 既有测试）', async () => {
    countDb = []
    const wrapper = mount(MerchantManager)
    await flushPromises()
    const btns = countButtons(wrapper)
    expect(btns).toHaveLength(2)
    expect(btns.every((b) => b!.exists())).toBe(true)

    await btns[1]!.trigger('click')
    expect(pushMock).toHaveBeenCalledWith({ name: 'transactions', query: { merchant: 'mch-2' } })
  })
})
