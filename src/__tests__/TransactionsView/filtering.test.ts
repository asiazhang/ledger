import { routeMock, makeTxn, setTxnDb, setMerchantDb, mountView, listCalls, lastListFilter, bodyRows } from './common'
import { describe, it, expect, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { NSelect, NButton } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import type { Merchant, Transaction } from '@/types'

describe('TransactionsView URL 下钻接线（issue #97/#191，冒烟级）', () => {
  // account/merchant 参数的解析、校验、复位规则、就绪补判与字段级让位已内化在
  // TransactionFilter 参数表，用例迁到模块接口测试 useTransactionFilter.test.ts
  // （issue #234 / ADR-0030 决策 7）；此处仅验证视图把 route query 递给模块的接线。
  beforeEach(() => {
    setTxnDb([
      makeTxn(1, 'acc-1', { merchant_id: 'mch-1', date: '2026-01-05' }),
      makeTxn(2, 'acc-2', { merchant_id: 'mch-1', date: '2026-02-10' }),
      makeTxn(3, 'acc-1', { date: '2026-01-20' }),
      makeTxn(4, 'acc-2', { kind: 'transfer', to_account_id: 'acc-1', date: '2026-01-25' }),
    ])
  })

  it('带有效 account 参数进入时自动按该账户过滤（涉及语义含转入侧）', async () => {
    routeMock.query = { account: 'acc-1' }
    const wrapper = await mountView()
    expect(lastListFilter()).toMatchObject({
      page: 1,
      page_size: 20,
      involving_account_id: 'acc-1',
    })
    // 涉及 acc-1：txn-1 / txn-3（主账户）+ txn-4（转账转入侧）
    expect(wrapper.text()).toContain('共 3 条')
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

  it('已挂载时导航清除 account 参数复位为全量并回到第 1 页', async () => {
    routeMock.query = { account: 'acc-1' }
    const wrapper = await mountView()
    expect(lastListFilter()).toMatchObject({ involving_account_id: 'acc-1', page: 1 })
    // 导航清除 query（如从侧边栏重新进入交易页）→ 复位全量 + 回第 1 页
    routeMock.query = {}
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 1 })
    expect(lastListFilter()).not.toHaveProperty('involving_account_id')
    expect(wrapper.text()).toContain('共 4 条')
  })
})

describe('TransactionsView 过滤行与手动过滤接线（issue #98，冒烟级）', () => {
  // 过滤意图语义（单维/组合/复位/同值不动作 → 状态终态与请求参数）与 URL 初始化仅
  // 结算一次、参考数据重拉不重放等时序行为已迁到模块接口测试 useTransactionFilter.test.ts
  // （ADR-0030 决策 7）；此处仅保留过滤行渲染冒烟、「控件 → 意图 → 列表」交互路由
  // 与 URL 只读契约。
  // 富数据集：不同账户/日期/类型，供交互路由与空态断言（每 describe 前置重置）。
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
  // 商户下拉 = 第 2 个（issue #191），类型下拉 = 第 3 个；
  // 时间维度行（issue #382）的行为测试见 time-chips.test.ts
  const accountSelect = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NSelect)[0]
  const merchantSelect = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NSelect)[1]
  const kindSelect = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NSelect)[2]

  /** 直接向过滤行控件 emit 变更事件（与 SearchView.test 的 setDate 模式一致）。 */
  async function setAccount(wrapper: ReturnType<typeof mount>, id: string | null) {
    accountSelect(wrapper).vm.$emit('update:value', id)
    await flushPromises()
  }

  /** 清除筛选按钮（工具栏与空态各一个）。 */
  const clearButton = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAllComponents(NButton).find((b) => b.text().includes('清除筛选'))!

  it('顶部渲染过滤行：账户/商户/类型下拉可清除、清除筛选按钮（日期起止控件已移除，issue #382）', async () => {
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

  async function setKind(wrapper: ReturnType<typeof mount>, k: string | null) {
    kindSelect(wrapper).vm.$emit('update:value', k)
    await flushPromises()
  }

  it('选择账户即重新查询：意图经模块出口生效，involving_account_id 正确传后端（含转账转入侧）', async () => {
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

  it('清除筛选按钮走 resetFilters：复位全部条件并回到全量列表（第 1 页）', async () => {
    const wrapper = await mountView()
    await setAccount(wrapper, 'acc-1')
    await setKind(wrapper, 'transfer')
    expect(wrapper.text()).toContain('共 1 条')
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

  it('手动改动过滤不回写 URL（组件状态为唯一事实源，与维度无关）', async () => {
    routeMock.query = { account: 'acc-1' }
    const wrapper = await mountView()
    await setAccount(wrapper, 'acc-2')
    await setKind(wrapper, 'income')
    // 商户维度同样不写回（URL 只读是整层契约，非按维度分支）
    await merchantSelect(wrapper).vm.$emit('update:value', 'mch-1')
    await flushPromises()
    expect(routeMock.query).toEqual({ account: 'acc-1' })
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

describe('TransactionsView 商户筛选（issue #191，冒烟级）', () => {
  // 组合/复位等意图语义迁到模块接口测试，URL 只读契约由上方 account 维度用例覆盖
  // （视图整层不写回，与维度无关）；此处保留下拉选项渲染冒烟与「控件 → 意图 → 列表」
  // 交互路由（软删商户可被选中过滤的历史交易口径）。
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
    // 外层 beforeEach 已以默认字典加载，此处强制重拉
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
})
