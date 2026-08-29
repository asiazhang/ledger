import { routeMock, makeTxn, merchantDb, mountView, mountViewSync, listCalls, lastListFilter, tablePagination, bodyRows, setTxnDb, setMerchantDb } from './common'
import { describe, it, expect, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { NSelect, NDatePicker, NButton } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import type { Merchant, Transaction } from '@/types'

describe('TransactionsView 涉及账户 URL 过滤（issue #97）', () => {
  it('带有效 account 参数进入时自动按该账户过滤（含转入转账语义的参数）', async () => {
    routeMock.query = { account: 'acc-1' }
    const wrapper = await mountView()
    expect(lastListFilter()).toMatchObject({
      page: 1,
      page_size: 20,
      involving_account_id: 'acc-1',
    })
    // 45 笔中奇数序号（acc-1）共 22 笔（偶数序号在 acc-2）
    expect(wrapper.text()).toContain('共 22 条')
    expect(bodyRows(wrapper).length).toBe(20)
  })

  it('带无效 account 参数（账户不存在）进入时回退全量且不报错', async () => {
    routeMock.query = { account: 'missing-acc' }
    const wrapper = await mountView()
    expect(lastListFilter()).not.toHaveProperty('involving_account_id')
    expect(wrapper.text()).toContain('共 45 条')
  })

  it('不带 account 参数进入时复位为全量列表', async () => {
    routeMock.query = {}
    const wrapper = await mountView()
    expect(lastListFilter()).not.toHaveProperty('involving_account_id')
    expect(wrapper.text()).toContain('共 45 条')
  })

  it('已挂载时清除 account 参数复位为全量并回到第 1 页', async () => {
    routeMock.query = { account: 'acc-1' }
    const wrapper = await mountView()
    expect(lastListFilter()).toMatchObject({ involving_account_id: 'acc-1', page: 1 })
    // 先翻到第 2 页
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 2, involving_account_id: 'acc-1' })
    // 导航清除 query（如从侧边栏重新进入交易页）→ 复位全量 + 回第 1 页
    routeMock.query = {}
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 1 })
    expect(lastListFilter()).not.toHaveProperty('involving_account_id')
    expect(wrapper.text()).toContain('共 45 条')
  })

  it('冷启动直连深链：参考数据晚到时有效 account 参数仍被应用（不静默丢失）', async () => {
    // 全新 pinia：参考数据尚未加载（self-init 在途），立即以带参 URL 挂载
    setActivePinia(createPinia())
    routeMock.query = { account: 'acc-1' }
    const wrapper = mountViewSync()
    await flushPromises()
    // 参考数据就绪后自动补判：过滤被应用，而非永久回退全量
    expect(lastListFilter()).toMatchObject({ involving_account_id: 'acc-1' })
    expect(wrapper.text()).toContain('共 22 条')
  })
})

describe('TransactionsView 手动过滤（issue #98）', () => {
  // 富数据集：不同账户/日期/类型，供单条件、组合、空态断言（每 describe 前置重置）。
  const richDb: Transaction[] = [
    makeTxn(1, 'acc-1', { kind: 'expense', date: '2026-01-05' }),
    makeTxn(2, 'acc-2', { kind: 'income', date: '2026-02-10' }),
    makeTxn(3, 'acc-1', { kind: 'transfer', date: '2026-03-15', to_account_id: 'acc-2' }),
    makeTxn(4, 'acc-2', { kind: 'expense', date: '2026-01-20' }),
    makeTxn(5, 'acc-1', { kind: 'refund', date: '2026-02-25' }),
  ]

  beforeEach(() => {
    setTxnDb([...richDb])
  })

  // 过滤行控件定位：账户下拉 = 第 1 个 NSelect（PinyinSelect 内层），
  // 商户下拉 = 第 2 个（issue #191），类型下拉 = 第 3 个
  const accountSelect = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NSelect)[0]
  const merchantSelect = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NSelect)[1]
  const kindSelect = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NSelect)[2]
  const datePickers = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NDatePicker)
  const clearButton = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NButton).find((b) => b.text().includes('清除筛选'))!

  /** 直接向过滤行控件 emit 变更事件（与 SearchView.test 的 setDate 模式一致）。 */
  async function setAccount(wrapper: ReturnType<typeof mount>, id: string | null) {
    accountSelect(wrapper).vm.$emit('update:value', id)
    await flushPromises()
  }
  async function setMerchant(wrapper: ReturnType<typeof mount>, id: string | null) {
    merchantSelect(wrapper).vm.$emit('update:value', id)
    await flushPromises()
  }
  async function setKind(wrapper: ReturnType<typeof mount>, k: string | null) {
    kindSelect(wrapper).vm.$emit('update:value', k)
    await flushPromises()
  }
  async function setDateFrom(wrapper: ReturnType<typeof mount>, v: string | null) {
    datePickers(wrapper)[0].vm.$emit('update:formattedValue', v)
    await flushPromises()
  }
  async function setDateTo(wrapper: ReturnType<typeof mount>, v: string | null) {
    datePickers(wrapper)[1].vm.$emit('update:formattedValue', v)
    await flushPromises()
  }

  it('顶部渲染过滤行：账户/商户/类型下拉可清除、起止日期、清除筛选按钮', async () => {
    const wrapper = await mountView()
    // 账户下拉：可清除，选项来自参考数据账户映射
    const account = accountSelect(wrapper)
    expect(account.props('clearable')).toBe(true)
    expect(
      (account.props('options') as { value: string; label: string }[]).map((o) => o.value),
    ).toEqual(['acc-1', 'acc-2'])
    // 商户下拉：可清除，选项来自参考数据商户映射（在用 + 软删，issue #191）
    const merchant = merchantSelect(wrapper)
    expect(merchant.props('clearable')).toBe(true)
    expect(
      (merchant.props('options') as { value: string }[]).map((o) => o.value),
    ).toEqual(['mch-1'])
    // 日期起止
    expect(datePickers(wrapper).length).toBe(2)
    // 类型下拉：可清除，6 种交易类型（income/expense/transfer/refund/buy/sell）
    const kind = kindSelect(wrapper)
    expect(kind.props('clearable')).toBe(true)
    expect((kind.props('options') as { value: string }[]).map((o) => o.value)).toEqual([
      'income',
      'expense',
      'transfer',
      'refund',
      'buy',
      'sell',
    ])
    // 清除筛选按钮：无过滤时禁用
    expect(clearButton(wrapper).attributes('disabled')).toBeDefined()
  })

  it('选择账户即重新查询：involving_account_id 正确传后端（含转账转入侧）', async () => {
    const wrapper = await mountView()
    const before = listCalls().length
    await setAccount(wrapper, 'acc-2')
    expect(listCalls().length).toBe(before + 1)
    expect(lastListFilter()).toMatchObject({
      page: 1,
      page_size: 20,
      involving_account_id: 'acc-2',
    })
    // 涉及 acc-2：income(txn-2)、expense(txn-4)、transfer 转入侧(txn-3)
    expect(wrapper.text()).toContain('共 3 条')
  })

  it('选择日期范围即重新查询：from/to 正确传后端（含边界）', async () => {
    const wrapper = await mountView()
    await setDateFrom(wrapper, '2026-02-01')
    expect(lastListFilter()).toMatchObject({ from: '2026-02-01' })
    expect(wrapper.text()).toContain('共 3 条') // txn-2 (02-10) / txn-3 (03-15) / txn-5 (02-25)
    await setDateTo(wrapper, '2026-02-20')
    expect(lastListFilter()).toMatchObject({ from: '2026-02-01', to: '2026-02-20' })
    expect(wrapper.text()).toContain('共 1 条') // 边界含：仅 txn-2
  })

  it('选择类型即重新查询：kind 正确传后端', async () => {
    const wrapper = await mountView()
    await setKind(wrapper, 'income')
    expect(lastListFilter()).toMatchObject({ kind: 'income' })
    expect(wrapper.text()).toContain('共 1 条')
  })

  it('多条件组合：账户 + 日期 + 类型同时传入后端', async () => {
    const wrapper = await mountView()
    await setAccount(wrapper, 'acc-1')
    await setDateFrom(wrapper, '2026-01-01')
    await setDateTo(wrapper, '2026-03-31')
    await setKind(wrapper, 'transfer')
    expect(lastListFilter()).toMatchObject({
      involving_account_id: 'acc-1',
      from: '2026-01-01',
      to: '2026-03-31',
      kind: 'transfer',
    })
    expect(wrapper.text()).toContain('共 1 条') // 唯一同时命中：txn-3
  })

  it('清除筛选复位全部条件并回到全量列表（第 1 页）', async () => {
    const wrapper = await mountView()
    await setAccount(wrapper, 'acc-1')
    await setKind(wrapper, 'transfer')
    await setDateFrom(wrapper, '2026-01-01')
    expect(wrapper.text()).toContain('共 1 条')
    // 先翻页，验证清除后回到第 1 页
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 2, involving_account_id: 'acc-1' })
    await clearButton(wrapper).trigger('click')
    await flushPromises()
    const f = lastListFilter()
    expect(f).toMatchObject({ page: 1, page_size: 20 })
    expect(f).not.toHaveProperty('from')
    expect(f).not.toHaveProperty('to')
    expect(f).not.toHaveProperty('involving_account_id')
    expect(f).not.toHaveProperty('kind')
    expect(wrapper.text()).toContain('共 5 条')
  })

  it('手动改动过滤不回写 URL（组件状态为唯一事实源）', async () => {
    routeMock.query = { account: 'acc-1' }
    const wrapper = await mountView()
    await setAccount(wrapper, 'acc-2')
    await setKind(wrapper, 'income')
    await setDateFrom(wrapper, '2026-01-01')
    expect(routeMock.query).toEqual({ account: 'acc-1' })
  })

  it('侧边栏重进（清除 account 参数）同时复位日期/类型过滤，回到全量列表（#96 决策 3）', async () => {
    routeMock.query = { account: 'acc-1' }
    const wrapper = await mountView()
    await setDateFrom(wrapper, '2026-02-01')
    await setKind(wrapper, 'income')
    expect(lastListFilter()).toMatchObject({
      involving_account_id: 'acc-1',
      from: '2026-02-01',
      kind: 'income',
    })
    // 模拟从侧边栏重新进入交易页：导航清除 query
    routeMock.query = {}
    await flushPromises()
    const f = lastListFilter()
    expect(f).toMatchObject({ page: 1, page_size: 20 })
    expect(f).not.toHaveProperty('involving_account_id')
    expect(f).not.toHaveProperty('from')
    expect(f).not.toHaveProperty('kind')
    expect(wrapper.text()).toContain('共 5 条')
  })

  it('参考数据重拉不把手动改动覆盖回 URL 值（URL 初始化仅结算一次）', async () => {
    routeMock.query = { account: 'acc-1' }
    const wrapper = await mountView()
    await setAccount(wrapper, 'acc-2')
    expect(lastListFilter()).toMatchObject({ involving_account_id: 'acc-2' })
    // 触发一次参考数据重拉（status loading → ready，如 ledger:changed 后的重载）
    await useReferenceStore().refresh()
    await flushPromises()
    // 手动改动保持，不被 URL 值 acc-1 覆盖
    expect(lastListFilter()).toMatchObject({ involving_account_id: 'acc-2' })
  })

  it('分页与页大小切换保持过滤条件', async () => {
    const wrapper = await mountView()
    await setAccount(wrapper, 'acc-1') // 涉及 acc-1：txn-1 / txn-3 / txn-5 共 3 条
    // 页大小切换：保持 acc-1 过滤并回到第 1 页
    tablePagination(wrapper).onUpdatePageSize(2)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({
      page: 1,
      page_size: 2,
      involving_account_id: 'acc-1',
    })
    expect(bodyRows(wrapper).length).toBe(2)
    // 翻页：保持过滤
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({
      page: 2,
      page_size: 2,
      involving_account_id: 'acc-1',
    })
    expect(wrapper.text()).toContain('共 3 条')
    expect(bodyRows(wrapper).length).toBe(1)
  })

  it('过滤无结果时展示空态提示（与加载态区分），空态可一键清除', async () => {
    const wrapper = await mountView()
    await setKind(wrapper, 'buy') // richDb 无 buy → 空结果
    expect(wrapper.text()).toContain('没有符合条件的交易')
    expect(bodyRows(wrapper).length).toBe(0)
    // 空态中的「清除筛选」可一键复位到全量
    await clearButton(wrapper).trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('共 5 条')
  })
})


describe('TransactionsView 商户筛选（issue #191）', () => {
  // 富数据集：商户命中/未命中、软删商户历史交易并存，供筛选/组合/URL 直达断言
  const merchantDbAll: Merchant[] = [
    {
      id: 'mch-1', name: '京东',
      created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
      version: 1, device_id: 'test', is_deleted: false,
    },
    {
      id: 'mch-2', name: '红旗连锁',
      created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
      version: 1, device_id: 'test', is_deleted: true,
    },
  ]
  const merchantTxnDb: Transaction[] = [
    makeTxn(1, 'acc-1', { merchant_id: 'mch-1', date: '2026-01-05' }),
    makeTxn(2, 'acc-2', { merchant_id: 'mch-1', date: '2026-02-10' }),
    makeTxn(3, 'acc-1', { merchant_id: 'mch-2', date: '2026-03-15' }),
    makeTxn(4, 'acc-1', { merchant_id: null, date: '2026-01-20' }),
  ]

  beforeEach(async () => {
    setMerchantDb(merchantDbAll)
    setTxnDb([...merchantTxnDb])
    // 外层 beforeEach 已以默认字典加载并在新鲜度窗口内缓存，此处强制重拉
    await useReferenceStore().refresh()
  })

  const merchantSelect = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NSelect)[1]

  /** 直接向商户下拉 emit 变更事件。 */
  async function setMerchant(wrapper: ReturnType<typeof mount>, id: string | null) {
    merchantSelect(wrapper).vm.$emit('update:value', id)
    await flushPromises()
  }

  it('下拉选项含软删商户（仍可过滤历史交易），按名称排序', async () => {
    const wrapper = await mountView()
    const options = merchantSelect(wrapper).props('options') as {
      value: string
      label: string
    }[]
    // zh 拼音序：红(hong) < 京(jing)
    expect(options.map((o) => o.value)).toEqual(['mch-2', 'mch-1'])
    expect(options.map((o) => o.label)).toEqual(['红旗连锁', '京东'])
  })

  it('选择商户即重新查询：merchant_id 正确传后端，total 随筛选变化', async () => {
    const wrapper = await mountView()
    const before = listCalls().length
    await setMerchant(wrapper, 'mch-1')
    expect(listCalls().length).toBe(before + 1)
    expect(lastListFilter()).toMatchObject({ page: 1, page_size: 20, merchant_id: 'mch-1' })
    expect(wrapper.text()).toContain('共 2 条')
    // 软删商户同样可过滤（历史交易口径）
    await setMerchant(wrapper, 'mch-2')
    expect(lastListFilter()).toMatchObject({ merchant_id: 'mch-2' })
    expect(wrapper.text()).toContain('共 1 条')
    // 清除下拉回全量
    await setMerchant(wrapper, null)
    expect(lastListFilter()).not.toHaveProperty('merchant_id')
    expect(wrapper.text()).toContain('共 4 条')
  })

  it('商户与账户/类型组合筛选同时传后端', async () => {
    const wrapper = await mountView()
    wrapper.findAllComponents(NSelect)[0].vm.$emit('update:value', 'acc-1')
    await flushPromises()
    await setMerchant(wrapper, 'mch-1')
    wrapper.findAllComponents(NSelect)[2].vm.$emit('update:value', 'expense')
    await flushPromises()
    expect(lastListFilter()).toMatchObject({
      involving_account_id: 'acc-1',
      merchant_id: 'mch-1',
      kind: 'expense',
    })
    expect(wrapper.text()).toContain('共 1 条') // 仅 txn-1 同时命中
  })

  it('清除筛选复位商户条件', async () => {
    const wrapper = await mountView()
    await setMerchant(wrapper, 'mch-1')
    expect(wrapper.text()).toContain('共 2 条')
    const clearButton = wrapper
      .findAllComponents(NButton)
      .find((b) => b.text().includes('清除筛选'))!
    await clearButton.trigger('click')
    await flushPromises()
    const f = lastListFilter()
    expect(f).toMatchObject({ page: 1 })
    expect(f).not.toHaveProperty('merchant_id')
    expect(wrapper.text()).toContain('共 4 条')
  })

  it('手动改动商户筛选不回写 URL（组件状态为唯一事实源）', async () => {
    routeMock.query = { merchant: 'mch-1' }
    const wrapper = await mountView()
    await setMerchant(wrapper, 'mch-2')
    expect(routeMock.query).toEqual({ merchant: 'mch-1' })
  })
})

describe('TransactionsView 商户 URL 直达（issue #191）', () => {
  beforeEach(async () => {
    setTxnDb([
      makeTxn(1, 'acc-1', { merchant_id: 'mch-1', date: '2026-01-05' }),
      makeTxn(2, 'acc-2', { merchant_id: 'mch-1', date: '2026-02-10' }),
      makeTxn(3, 'acc-1', { merchant_id: 'mch-2', date: '2026-03-15' }),
      makeTxn(4, 'acc-1', { merchant_id: null, date: '2026-01-20' }),
    ])
    // 商户筛选测试用含软删字典（外层已以默认字典加载，强制重拉）
    setMerchantDb([
      { ...merchantDb[0] },
      {
        id: 'mch-2', name: '红旗连锁',
        created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
        version: 1, device_id: 'test', is_deleted: true,
      },
    ])
    await useReferenceStore().refresh()
  })

  it('带有效 merchant 参数进入时自动按该商户过滤（软删商户也可直达）', async () => {
    routeMock.query = { merchant: 'mch-2' }
    const wrapper = await mountView()
    expect(lastListFilter()).toMatchObject({ page: 1, page_size: 20, merchant_id: 'mch-2' })
    expect(wrapper.text()).toContain('共 1 条')
  })

  it('带无效 merchant 参数（商户不存在）进入时回退全量且不报错', async () => {
    routeMock.query = { merchant: 'missing-mch' }
    const wrapper = await mountView()
    expect(lastListFilter()).not.toHaveProperty('merchant_id')
    expect(wrapper.text()).toContain('共 4 条')
  })

  it('不带 merchant 参数进入时复位为全量列表', async () => {
    routeMock.query = {}
    const wrapper = await mountView()
    expect(lastListFilter()).not.toHaveProperty('merchant_id')
    expect(wrapper.text()).toContain('共 4 条')
  })

  it('已挂载时清除 merchant 参数复位为全量并回到第 1 页', async () => {
    routeMock.query = { merchant: 'mch-1' }
    const wrapper = await mountView()
    expect(lastListFilter()).toMatchObject({ merchant_id: 'mch-1', page: 1 })
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 2, merchant_id: 'mch-1' })
    routeMock.query = {}
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 1 })
    expect(lastListFilter()).not.toHaveProperty('merchant_id')
    expect(wrapper.text()).toContain('共 4 条')
  })

  it('account 与 merchant 参数可组合直达（同时生效）', async () => {
    routeMock.query = { account: 'acc-1', merchant: 'mch-1' }
    const wrapper = await mountView()
    expect(lastListFilter()).toMatchObject({
      involving_account_id: 'acc-1',
      merchant_id: 'mch-1',
    })
    expect(wrapper.text()).toContain('共 1 条')
  })

  it('仅 merchant 参数进入：账户维度不筛、日期/类型复位', async () => {
    routeMock.query = { merchant: 'mch-1' }
    const wrapper = await mountView()
    expect(lastListFilter()).toMatchObject({ merchant_id: 'mch-1' })
    expect(lastListFilter()).not.toHaveProperty('involving_account_id')
    expect(wrapper.text()).toContain('共 2 条')
  })

  it('冷启动直连深链：参考数据晚到时有效 merchant 参数仍被应用（不静默丢失）', async () => {
    setActivePinia(createPinia())
    routeMock.query = { merchant: 'mch-1' }
    mountViewSync()
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ merchant_id: 'mch-1' })
  })

  it('参考数据重拉不把手动改动覆盖回 URL 值（URL 初始化仅结算一次）', async () => {
    routeMock.query = { merchant: 'mch-1' }
    const wrapper = await mountView()
    await setMerchant(wrapper, 'mch-2')
    expect(lastListFilter()).toMatchObject({ merchant_id: 'mch-2' })
    await useReferenceStore().refresh()
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ merchant_id: 'mch-2' })
  })

  /** 商户下拉直接 emit 变更事件（与手动过滤测试同模式）。 */
  async function setMerchant(wrapper: ReturnType<typeof mount>, id: string | null) {
    wrapper.findAllComponents(NSelect)[1].vm.$emit('update:value', id)
    await flushPromises()
  }
})
