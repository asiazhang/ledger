import { afterEach, describe, it, expect, beforeEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from './helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { NDataTable, NForm, NProgress, NSelect, NInputNumber, NDatePicker, NModal } from 'naive-ui'
import { setFakeMedia } from './helpers/media-mock'
import { formatAmount } from '@/utils/money'
import { useReferenceStore } from '@/stores/reference'
import { todayStr } from '@/utils/date'
import BudgetView from '@/views/BudgetView.vue'
import { messageApi } from './helpers/message-mock'
import { makeFakeSink, resetToastSink } from './factories'
import { registerToastSink } from '@/composables/useLoadable'
import { findButton, findBodyButton } from './helpers/dom'
import type { BudgetProgress, Category } from '@/types'


const mockCategories: Category[] = [
  {
    id: 'cat-1',
    name: '餐饮',
    kind: 'expense',
    parent_id: null,
    icon: null,
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
  {
    id: 'cat-1-sub',
    name: '早餐',
    kind: 'expense',
    parent_id: 'cat-1',
    icon: null,
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
  {
    id: 'cat-2',
    name: '工资',
    kind: 'income',
    parent_id: null,
    icon: null,
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
  {
    id: 'cat-2-sub',
    name: '理财收益',
    kind: 'income',
    parent_id: 'cat-2',
    icon: null,
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
]

/** 参考命令本场景需自定义值（overrides 优先于参考兜底）：断言消费自定义支出分类集。 */
const REFERENCE_OVERRIDES = { list_categories: mockCategories }

const mockProgress: BudgetProgress = {
  budget: {
    id: 'budget-1',
    category_id: 'cat-1',
    period: 'monthly',
    amount_cents: 50000,
    start_date: '2026-07-01',
    created_at: '2026-07-01T00:00:00Z',
    updated_at: '2026-07-01T00:00:00Z',
    version: 1,
    device_id: 'dev-1',
    is_deleted: false,
  },
  category_name: '餐饮',
  spent_cents: 20000,
  over_budget: false,
}

/** 子分类预算行（issue #356）：后端 category_name 只返回子分类自身名。 */
const subProgress: BudgetProgress = {
  budget: { ...mockProgress.budget, id: 'budget-sub', category_id: 'cat-1-sub' },
  category_name: '早餐',
  spent_cents: 12000,
  over_budget: false,
}

/** 孤儿预算行（issue #356）：分类已删，后端回退「未分类」。 */
const orphanProgress: BudgetProgress = {
  budget: { ...mockProgress.budget, id: 'budget-orphan', category_id: 'cat-gone' },
  category_name: '未分类',
  spent_cents: 3000,
  over_budget: false,
}

/** 挂载视图（参考数据经 store 注入），flush 后就绪。
 *  需自定义 invoke 返回（进度行/override）时，在调用本函数前重新 wireInvokeSeam。 */
async function mountView() {
  const wrapper = mount(BudgetView)
  await flushPromises()
  return wrapper
}

/** 从分类下拉（收窄为支出分类选项）选中指定分类。 */
function pickCategory(wrapper: Awaited<ReturnType<typeof mountView>>, id: string) {
  wrapper.findComponent(NSelect).vm.$emit('update:value', id)
}

/** 填金额并点击「添加」。 */
async function submitAmount(wrapper: Awaited<ReturnType<typeof mountView>>, amount: number) {
  wrapper.findComponent(NInputNumber).vm.$emit('update:value', amount)
  const add = findButton(wrapper, '添加', { exact: true })
  expect(add, '应存在「添加」按钮').toBeDefined()
  await add!.trigger('click')
  await flushPromises()
}

/** 打开编辑弹窗：点击列表首行「编辑」按钮。 */
async function openEditModal(wrapper: Awaited<ReturnType<typeof mountView>>) {
  const edit = findButton(wrapper, '编辑', { exact: true })
  expect(edit, '操作列应存在「编辑」按钮').toBeDefined()
  await edit!.trigger('click')
  await flushPromises()
}

/** 编辑弹窗内容由 NModal teleport 到 body，按钮与文案从 document.body 查询。 */
function bodyButton(text: string, label: string) {
  const btn = findBodyButton(text, { exact: true })
  expect(btn, `应存在「${text}」按钮（${label}）`).toBeDefined()
  return btn!.element
}

beforeEach(async () => {
  resetToastSink()
  wireInvokeSeam({
    defaults: { budget_progress: [] },
    overrides: { ...REFERENCE_OVERRIDES },
  })
  const store = useReferenceStore()
  await store.refresh()
})

afterEach(() => {
  resetToastSink()
})

describe('BudgetView 加载失败治愈（issue #1008）', () => {
  it('清单加载失败：默认策略弹裸 errorMessage（治愈原 try/finally 无 catch 的静默失败）', async () => {
    const sink = makeFakeSink()
    registerToastSink(sink)
    wireInvokeSeam({
      defaults: { budget_progress: [] },
      overrides: {
        ...REFERENCE_OVERRIDES,
        budget_progress: () => Promise.reject(new Error('数据库不可用')),
      },
    })
    await mountView()
    expect(sink.error).toHaveBeenCalledWith('数据库不可用')
  })
})

describe('BudgetView 预算表单（issue #183）', () => {
  it('分类下拉提供全部支出分类（顶级+子分类），子分类 label 为「父 > 子」路径名；收入分类（无论层级）不可选（issue #356）', async () => {
    const wrapper = await mountView()
    const options = wrapper.findComponent(NSelect).props('options') as {
      label: string
      value: string
    }[]
    expect(options).toEqual([
      { label: '餐饮', value: 'cat-1' },
      { label: '餐饮 > 早餐', value: 'cat-1-sub' },
    ])
  })

  it('子分类选项：输入父名或子名的拼音均可命中（issue #356）', async () => {
    const wrapper = await mountView()
    // 对下拉实际提供的选项（label 为路径名）走 PinyinSelect 收口的拼音过滤：
    // 用户输入父名或子名拼音，该选项保持可见
    const select = wrapper.findComponent(NSelect)
    const filter = select.props('filter') as (
      pattern: string,
      option: { label: string },
    ) => boolean
    const sub = (select.props('options') as { label: string; value: string }[]).find(
      (o) => o.value === 'cat-1-sub',
    )
    expect(sub, '下拉应包含子分类选项').toBeDefined()
    expect(filter('cy', sub!)).toBe(true) // 父名「餐饮」拼音首字母
    expect(filter('zc', sub!)).toBe(true) // 子名「早餐」拼音首字母
    expect(filter('早餐', sub!)).toBe(true) // 子名原文子串
  })

  it('表单无日期选择器（issue #184：设置预算只剩分类与金额）', async () => {
    const wrapper = await mountView()
    expect(wrapper.findComponent(NDatePicker).exists()).toBe(false)
  })

  it('金额非正前置拦截，不发起后端调用', async () => {
    const wrapper = await mountView()
    pickCategory(wrapper, 'cat-1')
    await submitAmount(wrapper, 0)
    expect(messageApi.warning).toHaveBeenCalledWith('预算金额必须为正数')
    expect(mockInvoke).not.toHaveBeenCalledWith('create_budget', expect.anything())
  })

  it('提交成功清空表单并提示；start_date 仅作记录字段传创建当日（issue #184）', async () => {
    wireInvokeSeam({
      defaults: { budget_progress: [] },
      overrides: {
        ...REFERENCE_OVERRIDES,
        create_budget: () => Promise.resolve('budget-1'),
      },
    })
    const wrapper = await mountView()
    pickCategory(wrapper, 'cat-1')
    await submitAmount(wrapper, 500)
    expect(mockInvoke).toHaveBeenCalledWith('create_budget', {
      input: {
        category_id: 'cat-1',
        amount_cents: 50000,
        // 本地日历日语义（issue #214）：不再用 UTC toISOString 切片
        start_date: todayStr(),
      },
    })
    expect(messageApi.success).toHaveBeenCalledWith('已创建预算')
  })

  it('查重失败把后端中文错误清晰呈现，提示引导编辑已有预算（issue #184）', async () => {
    wireInvokeSeam({
      defaults: { budget_progress: [] },
      overrides: {
        ...REFERENCE_OVERRIDES,
        create_budget: () =>
          Promise.reject({ kind: 'Invalid', message: '该分类已存在按月预算，可编辑该预算的金额' }),
      },
    })
    const wrapper = await mountView()
    pickCategory(wrapper, 'cat-1')
    await submitAmount(wrapper, 100)
    expect(messageApi.error).toHaveBeenCalledWith(
      '创建失败: 该分类已存在按月预算，可编辑该预算的金额',
    )
  })
})

describe('BudgetView 子分类预算路径名呈现与孤儿回退（issue #356）', () => {
  it('预算执行列表对子分类预算显示「父 > 子」路径名', async () => {
    wireInvokeSeam({ defaults: { budget_progress: [subProgress] }, overrides: { ...REFERENCE_OVERRIDES } })
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('餐饮 > 早餐')
  })

  it('编辑弹窗分类只读行显示路径名', async () => {
    wireInvokeSeam({ defaults: { budget_progress: [subProgress] }, overrides: { ...REFERENCE_OVERRIDES } })
    const wrapper = await mountView()
    await openEditModal(wrapper)
    expect(document.body.textContent).toContain('餐饮 > 早餐')
  })

  it('孤儿预算（分类已删）回退显示「未分类」，列表与编辑弹窗均不报错', async () => {
    wireInvokeSeam({ defaults: { budget_progress: [orphanProgress] }, overrides: { ...REFERENCE_OVERRIDES } })
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('未分类')
    await openEditModal(wrapper)
    expect(document.body.textContent).toContain('未分类')
  })

  it('路径名呈现与孤儿回退在同一列表共存（父预算 + 子预算 + 孤儿）', async () => {
    wireInvokeSeam({ defaults: { budget_progress: [mockProgress, subProgress, orphanProgress] }, overrides: { ...REFERENCE_OVERRIDES } })
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('餐饮')
    expect(wrapper.text()).toContain('餐饮 > 早餐')
    expect(wrapper.text()).toContain('未分类')
  })
})

describe('BudgetView 编辑预算金额（issue #184）', () => {
  it('列表操作列有「编辑」入口，弹窗仅金额可改（分类/周期只读，无日期选择器）', async () => {
    wireInvokeSeam({ defaults: { budget_progress: [mockProgress] }, overrides: { ...REFERENCE_OVERRIDES } })
    const wrapper = await mountView()
    expect(wrapper.text()).not.toContain('开始日期')
    await openEditModal(wrapper)
    const modal = wrapper.findComponent(NModal)
    expect(modal.exists()).toBe(true)
    // 弹窗内只有一个金额输入框，无分类下拉、无日期选择器
    expect(modal.findAllComponents(NInputNumber).length).toBe(1)
    expect(modal.findComponent(NSelect).exists()).toBe(false)
    expect(modal.findComponent(NDatePicker).exists()).toBe(false)
    // 分类/周期以只读文案展示（teleport 到 body）
    expect(document.body.textContent).toContain('餐饮')
    expect(document.body.textContent).toContain('按月')
  })

  it('弹窗回填当前金额，保存调用 update_budget 并刷新列表', async () => {
    wireInvokeSeam({
      defaults: { budget_progress: [mockProgress] },
      overrides: {
        ...REFERENCE_OVERRIDES,
        update_budget: () => Promise.resolve(null),
      },
    })
    const wrapper = await mountView()
    await openEditModal(wrapper)
    const modal = wrapper.findComponent(NModal)
    const input = modal.findComponent(NInputNumber)
    expect(input.props('value')).toBe(500) // 回填 50000 分 = 500 元
    input.vm.$emit('update:value', 800)
    const save = bodyButton('保存', '编辑弹窗')
    save.click()
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('update_budget', {
      id: 'budget-1',
      input: { amount_cents: 80000 },
    })
    expect(messageApi.success).toHaveBeenCalledWith('已更新预算')
    expect(messageApi.error).not.toHaveBeenCalled()
  })

  it('弹窗金额非正前置拦截，不发起后端调用', async () => {
    wireInvokeSeam({ defaults: { budget_progress: [mockProgress] }, overrides: { ...REFERENCE_OVERRIDES } })
    const wrapper = await mountView()
    await openEditModal(wrapper)
    const modal = wrapper.findComponent(NModal)
    modal.findComponent(NInputNumber).vm.$emit('update:value', 0)
    bodyButton('保存', '编辑弹窗').click()
    await flushPromises()
    expect(messageApi.warning).toHaveBeenCalledWith('预算金额必须为正数')
    expect(mockInvoke).not.toHaveBeenCalledWith('update_budget', expect.anything())
  })

  it('保存失败把后端错误清晰呈现', async () => {
    wireInvokeSeam({
      defaults: { budget_progress: [mockProgress] },
      overrides: {
        ...REFERENCE_OVERRIDES,
        update_budget: () =>
          Promise.reject({ kind: 'NotFound', message: '预算不存在: budget-1' }),
      },
    })
    const wrapper = await mountView()
    await openEditModal(wrapper)
    bodyButton('保存', '编辑弹窗').click()
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalledWith('更新失败: 预算不存在: budget-1')
  })
})

describe('BudgetView 移动档（issue #848 / ADR-0088 决策 11 票⑧，词汇表「窗口分级」）', () => {
  /** 带两行进度夹具布线（超支 + 正常），返回进度行集。 */
  function wireRows() {
    const over: typeof mockProgress = {
      ...mockProgress,
      budget: { ...mockProgress.budget, id: 'budget-over', amount_cents: 10000 },
      category_name: '餐饮',
      spent_cents: 12000,
      over_budget: true,
    }
    wireInvokeSeam({
      defaults: { budget_progress: [over, mockProgress] },
      overrides: { ...REFERENCE_OVERRIDES },
    })
    return [over, mockProgress]
  }

  function tableOf(wrapper: Awaited<ReturnType<typeof mountView>>) {
    return wrapper.findComponent(NDataTable)
  }

  it('桌面档零变化：七列、新增表单行内横排（回归红线）', async () => {
    wireRows()
    const wrapper = await mountView()
    expect((tableOf(wrapper).props('columns') as unknown[]).length).toBe(7)
    expect(wrapper.findComponent(NForm).props('inline')).toBe(true)
  })

  it('移动档列结构三分：分类（周期/状态并入副行）、进度（含已支/预算文案）、操作——窄屏无横向滚动前提', async () => {
    setFakeMedia({ width: 600 })
    wireRows()
    const wrapper = await mountView()
    const columns = tableOf(wrapper).props('columns') as Array<{ key?: string }>
    expect(columns.map((c) => c.key)).toEqual(['category_name', 'progress', 'actions'])
    const rows = wrapper.findAll('.n-data-table-tbody .n-data-table-tr')
    // 分类列副行不丢信息：周期（本地化标签）与状态随副行可读
    expect(rows[0].text()).toContain('按月')
    expect(rows[0].text()).toContain('超支')
    expect(rows[1].text()).toContain('正常')
    // 进度列文案不丢口径：已支出 / 预算两金额并存（期待值调同一 formatAmount 实现）
    expect(rows[1].text()).toContain(
      `已支出 ${formatAmount(20000)} / ${formatAmount(50000)}`,
    )
  })

  it('移动档进度语义零变化：超支行进度条 error、正常行 success（同一命令输出，仅布局适配）', async () => {
    setFakeMedia({ width: 600 })
    wireRows()
    const wrapper = await mountView()
    const bars = wrapper.findAllComponents(NProgress)
    expect(bars.length).toBe(2)
    expect(bars[0].props('status')).toBe('error')
    expect(bars[0].props('percentage')).toBe(100) // 12000/10000 封顶
    expect(bars[1].props('status')).toBe('success')
    expect(bars[1].props('percentage')).toBe(40) // 20000/50000
  })

  it('移动档操作 48px 触控目标（ADR-0088 全局验收基线）；编辑/删除语义不变', async () => {
    setFakeMedia({ width: 600 })
    wireRows()
    const wrapper = await mountView()
    for (const text of ['编辑', '删除']) {
      const btn = findButton(wrapper, text, { exact: true })
      expect(btn, `应存在「${text}」按钮`).toBeDefined()
      const el = btn!.element as HTMLElement
      expect(el.style.minWidth).toBe('48px')
      expect(el.style.minHeight).toBe('48px')
    }
    // 编辑：同一弹窗意图入口（移动档经 AppModal 全屏化，票④，不在本票范围）
    await openEditModal(wrapper)
    expect(wrapper.findComponent(NModal).exists()).toBe(true)
    // 删除：同一确认弹层语义（首行 = budget-over）
    await findButton(wrapper, '删除', { exact: true })!.trigger('click')
    await flushPromises()
    const positive = document.body.querySelector('.n-popconfirm .n-button--primary-type')
    expect(positive).not.toBeNull()
    ;(positive as HTMLButtonElement).click()
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('delete_budget', { id: 'budget-over' })
  })

  it('跨断点缩窗实时换列（七列 ⇄ 三列）', async () => {
    wireRows()
    const wrapper = await mountView()
    expect((tableOf(wrapper).props('columns') as unknown[]).length).toBe(7)
    setFakeMedia({ width: 600 })
    await flushPromises()
    expect((tableOf(wrapper).props('columns') as unknown[]).length).toBe(3)
    setFakeMedia({ width: 1280 })
    await flushPromises()
    expect((tableOf(wrapper).props('columns') as unknown[]).length).toBe(7)
  })

  it('移动档新增预算表单纵排（标签上置 + 控件满宽），桌面横排不变', async () => {
    setFakeMedia({ width: 600 })
    const wrapper = await mountView()
    const form = wrapper.findComponent(NForm)
    expect(form.props('inline')).toBe(false)
    expect(form.props('labelPlacement')).toBe('top')
  })
})
