import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { NSelect, NInputNumber, NDatePicker, NModal } from 'naive-ui'
import { setActivePinia, createPinia } from 'pinia'
import { useReferenceStore } from '@/stores/reference'
import { todayStr } from '@/utils/date'
import BudgetView from '@/views/BudgetView.vue'
import { stubReferenceInvoke } from './helpers/reference-stubs'
import type { BudgetProgress, Category } from '@/types'


enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

// 覆写 setup.ts 的 useMessage mock：改用稳定实例以便断言反馈分支
// （spec：提交失败→把后端错误清晰呈现给用户）。
const messageApi = vi.hoisted(() => ({
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
  info: vi.fn(),
  loading: vi.fn(),
  destroyAll: vi.fn(),
}))
vi.mock('naive-ui', async (importOriginal) => {
  const actual = await importOriginal<typeof import('naive-ui')>()
  return { ...actual, useMessage: () => messageApi }
})

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

/** 基础 invoke 桩：参考数据 + 空预算进度（返回派发函数供用例内重桩委托）。 */
function baseStub(progress: BudgetProgress[] = []) {
  return stubReferenceInvoke({
    list_accounts: [],
    list_categories: mockCategories,
    list_insurers: [],
    list_merchants: [],
    budget_progress: progress,
  })
}

/** 挂载前注入进度行（编辑弹窗用例用）。 */
function withProgress(progress: BudgetProgress[]) {
  baseStub(progress)
}

/** 挂载视图（参考数据经 store 注入），flush 后就绪。
 *  需自定义 invoke 返回（进度行/override）时，在调用本函数前 mockImplementation。 */
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
  const add = wrapper.findAll('button').find((b) => b.text() === '添加')
  expect(add, '应存在「添加」按钮').toBeDefined()
  await add!.trigger('click')
  await flushPromises()
}

/** 打开编辑弹窗：点击列表首行「编辑」按钮。 */
async function openEditModal(wrapper: Awaited<ReturnType<typeof mountView>>) {
  const edit = wrapper.findAll('button').find((b) => b.text() === '编辑')
  expect(edit, '操作列应存在「编辑」按钮').toBeDefined()
  await edit!.trigger('click')
  await flushPromises()
}

/** 编辑弹窗内容由 NModal teleport 到 body，按钮与文案从 document.body 查询。 */
function bodyButtons() {
  return Array.from(document.body.querySelectorAll('button'))
}

function bodyButton(text: string, label: string) {
  const btn = bodyButtons().find((b) => b.textContent?.trim() === text)
  expect(btn, `应存在「${text}」按钮（${label}）`).toBeDefined()
  return btn!
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  messageApi.success.mockReset()
  messageApi.warning.mockReset()
  messageApi.error.mockReset()
  baseStub()
  const store = useReferenceStore()
  await store.refresh()
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
    const base = baseStub()
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'create_budget') return Promise.resolve('budget-1')
      return base(cmd)
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
    const base = baseStub()
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'create_budget') {
        return Promise.reject({ kind: 'Invalid', message: '该分类已存在按月预算，可编辑该预算的金额' })
      }
      return base(cmd)
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
    withProgress([subProgress])
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('餐饮 > 早餐')
  })

  it('编辑弹窗分类只读行显示路径名', async () => {
    withProgress([subProgress])
    const wrapper = await mountView()
    await openEditModal(wrapper)
    expect(document.body.textContent).toContain('餐饮 > 早餐')
  })

  it('孤儿预算（分类已删）回退显示「未分类」，列表与编辑弹窗均不报错', async () => {
    withProgress([orphanProgress])
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('未分类')
    await openEditModal(wrapper)
    expect(document.body.textContent).toContain('未分类')
  })

  it('路径名呈现与孤儿回退在同一列表共存（父预算 + 子预算 + 孤儿）', async () => {
    withProgress([mockProgress, subProgress, orphanProgress])
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('餐饮')
    expect(wrapper.text()).toContain('餐饮 > 早餐')
    expect(wrapper.text()).toContain('未分类')
  })
})

describe('BudgetView 编辑预算金额（issue #184）', () => {
  it('列表操作列有「编辑」入口，弹窗仅金额可改（分类/周期只读，无日期选择器）', async () => {
    withProgress([mockProgress])
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
    const base = baseStub([mockProgress])
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'update_budget') return Promise.resolve(null)
      return base(cmd)
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
    withProgress([mockProgress])
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
    const base = baseStub([mockProgress])
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'update_budget') {
        return Promise.reject({ kind: 'NotFound', message: '预算不存在: budget-1' })
      }
      return base(cmd)
    })
    const wrapper = await mountView()
    await openEditModal(wrapper)
    bodyButton('保存', '编辑弹窗').click()
    await flushPromises()
    expect(messageApi.error).toHaveBeenCalledWith('更新失败: 预算不存在: budget-1')
  })
})
