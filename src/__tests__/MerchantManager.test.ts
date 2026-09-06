import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { NDataTable, NPopconfirm } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import { stubReferenceInvoke } from './helpers/reference-stubs'
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

/** 参考数据桩（issue #725）：管理页只消费商户表与条数聚合（可变库函数型覆写），其余走共享助手规范夹具。 */
function mockBaseCommands() {
  stubReferenceInvoke({
    list_merchants: () => merchantDb,
    list_merchant_transaction_counts: () => countDb,
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
    stubReferenceInvoke({
      create_merchant: (args?: { input?: { name: string } }) => {
        merchantDb = [
          ...merchantDb,
          {
            id: 'mch-new', name: args!.input!.name,
            created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
            version: 1, device_id: 'test', is_deleted: false,
          },
        ]
        return Promise.resolve('mch-new')
      },
      list_merchants: () => merchantDb,
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
    stubReferenceInvoke({
      create_merchant: () => Promise.reject(new Error('参数错误: 商户已存在: 盒马')),
      list_merchants: () => merchantDb,
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

describe('MerchantManager.vue 拼音模糊搜索（issue #447，统一模糊搜索语义 ADR-0027）', () => {
  /** 五个商户：检索词构造覆盖原文子串（盒马 ⊂ 盒马/盒马鲜生、物业 ⊂ 万科物业）
   * 与拼音首字母子序列（wy ⊂ wkwy、jd ⊂ jd）双入口。 */
  const searchMerchants: Merchant[] = [
    '京东', '盒马', '万科物业', '红旗连锁', '盒马鲜生',
  ].map((name, i) => ({
    id: `mch-s-${i}`, name,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  }))

  beforeEach(async () => {
    setActivePinia(createPinia())
    mockInvoke.mockReset()
    merchantDb = searchMerchants
    countDb = []
    mockBaseCommands()
    const store = useReferenceStore()
    await store.refresh()
  })

  function searchInput(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAll('input')
      .find((i) => i.attributes('placeholder') === '搜索商户（名称/拼音）')!
  }

  function rowNames(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('tbody tr').map((r) => r.text())
  }

  it('汉字关键字过滤：未命中项隐藏、命中项保留', async () => {
    const wrapper = mount(MerchantManager)
    await searchInput(wrapper).setValue('物业')
    const rows = rowNames(wrapper)
    expect(rows).toHaveLength(1)
    expect(rows[0]).toContain('万科物业')
  })

  it('拼音首字母入口：子序列命中（wy → 万科物业），不误命中其他商户', async () => {
    const wrapper = mount(MerchantManager)
    await searchInput(wrapper).setValue('wy')
    const rows = rowNames(wrapper)
    expect(rows).toHaveLength(1)
    expect(rows[0]).toContain('万科物业')

    await searchInput(wrapper).setValue('jd')
    expect(rowNames(wrapper)).toHaveLength(1)
    expect(rowNames(wrapper)[0]).toContain('京东')
  })

  it('只过滤不重排：命中项保持列表原有相对顺序（汉字与拼音双路径）', async () => {
    const wrapper = mount(MerchantManager)
    await searchInput(wrapper).setValue('盒马')
    let rows = rowNames(wrapper)
    expect(rows).toHaveLength(2)
    expect(rows[0]).toContain('盒马')
    expect(rows[1]).toContain('盒马鲜生')

    // 拼音首字母子序列路径（hm ⊂ hm / hmxs）同序
    await searchInput(wrapper).setValue('hm')
    rows = rowNames(wrapper)
    expect(rows).toHaveLength(2)
    expect(rows[0]).toContain('盒马')
    expect(rows[1]).toContain('盒马鲜生')
  })

  it('清空搜索词恢复完整列表', async () => {
    const wrapper = mount(MerchantManager)
    await searchInput(wrapper).setValue('物业')
    expect(rowNames(wrapper)).toHaveLength(1)
    await searchInput(wrapper).setValue('')
    expect(rowNames(wrapper)).toHaveLength(5)
    expect(wrapper.text()).toContain('京东')
    expect(wrapper.text()).toContain('盒马鲜生')
  })
})

describe('MerchantManager.vue 显示已删切换（issue #447）', () => {
  const deletedMerchant: Merchant = {
    id: 'mch-del', name: '永辉超市',
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: true,
  }

  beforeEach(async () => {
    setActivePinia(createPinia())
    mockInvoke.mockReset()
    merchantDb = [...mockMerchants, deletedMerchant]
    countDb = []
    mockBaseCommands()
    pushMock.mockReset()
    const store = useReferenceStore()
    await store.refresh()
  })

  /** 「显示已删」开关（checkbox 语义定位）。 */
  function showDeletedToggle(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('.n-checkbox').find((c) => c.text() === '显示已删')!
  }

  function rowTexts(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('tbody tr').map((r) => r.text())
  }

  function editButtons(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('button').filter((b) => b.text() === '编辑')
  }

  function deleteButtons(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('button').filter((b) => b.text() === '删除')
  }

  it('默认列表不含已软删商户', () => {
    const wrapper = mount(MerchantManager)
    expect(wrapper.text()).not.toContain('永辉超市')
    expect(rowTexts(wrapper)).toHaveLength(2)
  })

  it('切换「显示已删」后已删商户以行展示', async () => {
    const wrapper = mount(MerchantManager)
    await showDeletedToggle(wrapper).trigger('click')
    expect(wrapper.text()).toContain('永辉超市')
    expect(rowTexts(wrapper)).toHaveLength(3)
  })

  it('已删行只读：无编辑/删除操作（在用行操作不受影响）', async () => {
    const wrapper = mount(MerchantManager)
    await showDeletedToggle(wrapper).trigger('click')
    expect(editButtons(wrapper)).toHaveLength(2)
    expect(deleteButtons(wrapper)).toHaveLength(2)
    // 行级精确断言（按按钮而非裸文本，避免与「已删除」标记的子串相撞）
    const deletedRowEl = wrapper
      .findAll('tbody tr')
      .find((r) => r.text().includes('永辉超市'))!
    expect(
      deletedRowEl.findAll('button').filter((b) => b.text() === '编辑'),
    ).toHaveLength(0)
    expect(
      deletedRowEl.findAll('button').filter((b) => b.text() === '删除'),
    ).toHaveLength(0)
  })

  it('已删行带删除标记（与在用行可区分）', async () => {
    const wrapper = mount(MerchantManager)
    await showDeletedToggle(wrapper).trigger('click')
    const deletedRow = rowTexts(wrapper).find((r) => r.includes('永辉超市'))!
    expect(deletedRow).toContain('已删除')
  })

  it('已删行条数照常显示且可下钻（软删商户条数后端照常计数）', async () => {
    countDb = [{ merchant_id: 'mch-del', transaction_count: 7 }]
    const wrapper = mount(MerchantManager)
    await showDeletedToggle(wrapper).trigger('click')
    const deletedRow = wrapper
      .findAll('tbody tr')
      .find((r) => r.text().includes('永辉超市'))!
    const countBtn = deletedRow
      .findAll('button')
      .find((b) => b.attributes('title') === '查看该商户的交易')!
    expect(countBtn.text()).toBe('7')

    await countBtn.trigger('click')
    expect(pushMock).toHaveBeenCalledWith({
      name: 'transactions',
      query: { merchant: 'mch-del' },
    })
  })

  it('搜索与显示已删叠加：搜索词对已删行同样过滤', async () => {
    const wrapper = mount(MerchantManager)
    await showDeletedToggle(wrapper).trigger('click')
    const search = wrapper
      .findAll('input')
      .find((i) => i.attributes('placeholder') === '搜索商户（名称/拼音）')!
    await search.setValue('永辉')
    const rows = rowTexts(wrapper)
    expect(rows).toHaveLength(1)
    expect(rows[0]).toContain('永辉超市')
  })
})

describe('MerchantManager.vue 前端分页（issue #457）', () => {
  /** 分页数据工厂：名称序 = id 序（补零保证字典序稳定）；56 条 → 首页 50 + 第 2 页 6。 */
  function makeMerchants(n: number): Merchant[] {
    return Array.from({ length: n }, (_, i) => ({
      id: `mch-p-${i}`,
      name: `商户${String(i).padStart(3, '0')}`,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
      version: 1,
      device_id: 'test',
      is_deleted: false,
    }))
  }

  const deletedExtra: Merchant = {
    id: 'mch-p-del',
    name: '永辉超市',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: true,
  }

  beforeEach(async () => {
    setActivePinia(createPinia())
    mockInvoke.mockReset()
    merchantDb = makeMerchants(56)
    countDb = []
    mockBaseCommands()
    pushMock.mockReset()
    const store = useReferenceStore()
    await store.refresh()
  })

  function paginationProps(wrapper: ReturnType<typeof mount>) {
    return wrapper.findComponent(NDataTable).props('pagination') as {
      page: number
      pageSize: number
      pageSizes: number[]
      onChange: (page: number) => void
      onUpdatePageSize: (size: number) => void
    }
  }

  function rowTexts(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('tbody tr').map((r) => r.text())
  }

  async function gotoPage(wrapper: ReturnType<typeof mount>, n: number) {
    const item = wrapper
      .findAll('.n-pagination-item')
      .find((el) => el.text() === String(n))
    expect(item).toBeTruthy()
    await item!.trigger('click')
    await flushPromises()
  }

  function searchInput(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAll('input')
      .find((i) => i.attributes('placeholder') === '搜索商户（名称/拼音）')!
  }

  function showDeletedToggle(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('.n-checkbox').find((c) => c.text() === '显示已删')!
  }

  /** 走完整删除流程：点击行内「删除」→ popconfirm 正向确认（NPopconfirm 内容
   * teleport 到 body，直接对其组件 emit 正向点击，PoliciesView 先例）→ mock 命令
   * 更新数据 → 手动 refresh 模拟 ledger:changed 失效重拉。 */
  async function deleteRow(wrapper: ReturnType<typeof mount>, rowIndex: number) {
    const row = wrapper.findAll('tbody tr')[rowIndex]!
    const deleteBtn = row.findAll('button').find((b) => b.text() === '删除')!
    await deleteBtn.trigger('click')
    await flushPromises()
    wrapper.findComponent(NPopconfirm).vm.$emit('positiveClick')
    await flushPromises()
    await useReferenceStore().refresh()
    await flushPromises()
  }

  /** 让 delete_merchant 命令对指定商户软删生效（is_deleted 置位；含软删全量
   * 列表由 store 按 is_deleted 拆分，软删行转已删区而非消失）。 */
  function mockDeleteMerchant(id: string) {
    stubReferenceInvoke({
      delete_merchant: () => {
        merchantDb = merchantDb.map((m) =>
          m.id === id ? { ...m, is_deleted: true } : m,
        )
        return Promise.resolve(null)
      },
      list_merchants: () => merchantDb,
      list_merchant_transaction_counts: () => countDb,
    })
  }

  it('默认每页 50 条，分页条显示过滤后总数', () => {
    const wrapper = mount(MerchantManager)
    expect(rowTexts(wrapper)).toHaveLength(50)
    expect(wrapper.text()).toContain('共 56 条')
    expect(paginationProps(wrapper).pageSize).toBe(50)
    expect(paginationProps(wrapper).page).toBe(1)
  })

  it('页大小档位为 10/20/50/100', () => {
    const wrapper = mount(MerchantManager)
    expect(paginationProps(wrapper).pageSizes).toEqual([10, 20, 50, 100])
  })

  it('翻页：第 2 页展示剩余 6 条', async () => {
    const wrapper = mount(MerchantManager)
    await gotoPage(wrapper, 2)
    expect(paginationProps(wrapper).page).toBe(2)
    const rows = rowTexts(wrapper)
    expect(rows).toHaveLength(6)
    expect(rows[0]).toContain('商户050')
  })

  it('切换页大小生效，且回到第一页', async () => {
    const wrapper = mount(MerchantManager)
    await gotoPage(wrapper, 2)
    paginationProps(wrapper).onUpdatePageSize(20)
    await flushPromises()
    expect(paginationProps(wrapper).page).toBe(1)
    expect(rowTexts(wrapper)).toHaveLength(20)

    // 档位上限 100：56 条全量单页展示
    paginationProps(wrapper).onUpdatePageSize(100)
    await flushPromises()
    expect(rowTexts(wrapper)).toHaveLength(56)
  })

  it('分页条总数随搜索过滤（过滤后总数而非全库条数）', async () => {
    const wrapper = mount(MerchantManager)
    await searchInput(wrapper).setValue('055')
    await flushPromises()
    expect(wrapper.text()).toContain('共 1 条')
    expect(rowTexts(wrapper)).toHaveLength(1)
  })

  it('搜索输入归零页码：第 2 页输入搜索词回第一页', async () => {
    const wrapper = mount(MerchantManager)
    await gotoPage(wrapper, 2)
    expect(rowTexts(wrapper)).toHaveLength(6)

    await searchInput(wrapper).setValue('055')
    await flushPromises()
    expect(paginationProps(wrapper).page).toBe(1)
    expect(rowTexts(wrapper)).toHaveLength(1)
  })

  it('清空搜索归零页码：第 2 页清空回第一页完整列表', async () => {
    const wrapper = mount(MerchantManager)
    await searchInput(wrapper).setValue('商户')
    await flushPromises()
    await gotoPage(wrapper, 2)
    expect(rowTexts(wrapper)).toHaveLength(6)

    await searchInput(wrapper).setValue('')
    await flushPromises()
    expect(paginationProps(wrapper).page).toBe(1)
    expect(rowTexts(wrapper)).toHaveLength(50)
    expect(wrapper.text()).toContain('共 56 条')
  })

  it('切换「显示已删」归零页码', async () => {
    merchantDb = [...makeMerchants(56), deletedExtra]
    await useReferenceStore().refresh()
    const wrapper = mount(MerchantManager)
    await gotoPage(wrapper, 2)
    expect(rowTexts(wrapper)).toHaveLength(6)

    await showDeletedToggle(wrapper).trigger('click')
    await flushPromises()
    expect(paginationProps(wrapper).page).toBe(1)
    expect(rowTexts(wrapper)).toHaveLength(50)
  })

  it('切换排序归零页码', async () => {
    const wrapper = mount(MerchantManager)
    await gotoPage(wrapper, 2)
    expect(rowTexts(wrapper)).toHaveLength(6)

    await wrapper.find('th.n-data-table-th--sortable').trigger('click')
    await flushPromises()
    expect(paginationProps(wrapper).page).toBe(1)
    expect(rowTexts(wrapper)).toHaveLength(50)
  })

  it('删除当前页最后一条 → 页码回退一页，不回第一页（ADR-0045 先例）', async () => {
    merchantDb = makeMerchants(51)
    await useReferenceStore().refresh()
    const wrapper = mount(MerchantManager)
    await gotoPage(wrapper, 2)
    expect(rowTexts(wrapper)).toHaveLength(1)

    mockDeleteMerchant('mch-p-50')
    await deleteRow(wrapper, 0)

    expect(paginationProps(wrapper).page).toBe(1)
    expect(rowTexts(wrapper)).toHaveLength(50)
    expect(wrapper.text()).not.toContain('商户050')
  })

  it('删除的行不是当前页最后一条 → 保持当前页', async () => {
    const wrapper = mount(MerchantManager)
    await gotoPage(wrapper, 2)
    expect(rowTexts(wrapper)).toHaveLength(6)

    mockDeleteMerchant('mch-p-50')
    await deleteRow(wrapper, 0)

    expect(paginationProps(wrapper).page).toBe(2)
    expect(rowTexts(wrapper)).toHaveLength(5)
  })

  it('「显示已删」开启时删除行不离开展示集合 → 页码不回退', async () => {
    merchantDb = [...makeMerchants(51), deletedExtra]
    await useReferenceStore().refresh()
    const wrapper = mount(MerchantManager)
    await showDeletedToggle(wrapper).trigger('click')
    await flushPromises()
    await gotoPage(wrapper, 2)
    expect(rowTexts(wrapper)).toHaveLength(2)

    mockDeleteMerchant('mch-p-50')
    // 第 2 页首行是商户050（在用区尾部）
    await deleteRow(wrapper, 0)

    // 软删行仍以已删行展示（移到已删区尾部），本页行数不变，页码不动
    expect(paginationProps(wrapper).page).toBe(2)
    expect(rowTexts(wrapper)).toHaveLength(2)
  })

  it('页码与页大小不持久化：重挂（页签切走再切回）回第一页、默认 50', async () => {
    const wrapper = mount(MerchantManager)
    await gotoPage(wrapper, 2)
    expect(paginationProps(wrapper).page).toBe(2)
    wrapper.unmount()

    const remounted = mount(MerchantManager)
    expect(paginationProps(remounted).page).toBe(1)
    expect(paginationProps(remounted).pageSize).toBe(50)
    expect(rowTexts(remounted)).toHaveLength(50)
  })
})
