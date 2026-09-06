import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { DOMWrapper, mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { NPopconfirm } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import { stubReferenceInvoke } from './helpers/reference-stubs'
import InsurerManager from '@/components/InsurerManager.vue'
import InsurerEditModal from '@/components/insurers/InsurerEditModal.vue'
import type { Insurer } from '@/types'

const { messageMock } = vi.hoisted(() => ({
  messageMock: {
    success: vi.fn(),
    warning: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
    loading: vi.fn(),
    destroyAll: vi.fn(),
  },
}))

// 覆盖 setup.ts 的全局 naive-ui mock：message 实例可断言（重名错误提示等）
vi.mock('naive-ui', async (importOriginal) => {
  const actual = await importOriginal<typeof import('naive-ui')>()
  return {
    ...actual,
    useMessage: () => messageMock,
  }
})


// NModal 内容传送至 document.body：每测后卸载，避免前一用例的弹窗残留在 body 污染查询
enableAutoUnmount(afterEach)

const mockInsurers: Insurer[] = [
  {
    id: 'ins-1', name: '平安人寿',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
  {
    id: 'ins-2', name: '人保财险',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
]

let insurerDb: Insurer[] = mockInsurers

/** 参考数据桩（issue #725）：管理页只消费保司表（可变库函数型覆写），其余走共享助手规范夹具。 */
function mockBaseCommands() {
  stubReferenceInvoke({ list_insurers: () => insurerDb })
}

function insurerCalls(cmd: string) {
  return mockInvoke.mock.calls.filter(([c]) => c === cmd)
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  insurerDb = mockInsurers
  mockBaseCommands()
  messageMock.success.mockClear()
  messageMock.error.mockClear()
  messageMock.warning.mockClear()
  const store = useReferenceStore()
  await store.refresh()
})

/** 弹窗内容传送至 document.body：直接在 body 上查询。 */
function findBodyInputByPlaceholder(placeholder: string): DOMWrapper<HTMLInputElement> {
  const el = document.body.querySelector(`input[placeholder="${placeholder}"]`)
  expect(el, `input[placeholder=${placeholder}] 应存在`).not.toBeNull()
  return new DOMWrapper(el as HTMLInputElement)
}

function rowTexts(wrapper: ReturnType<typeof mount>) {
  return wrapper.findAll('tbody tr').map((r) => r.text())
}

describe('InsurerManager.vue 管理（issue #714 / ADR-0082 决策 3）', () => {
  it('挂载并渲染保司列表（后端名称序）', () => {
    const wrapper = mount(InsurerManager)
    const rows = rowTexts(wrapper)
    expect(rows).toHaveLength(2)
    expect(rows[0]).toContain('平安人寿')
    expect(rows[1]).toContain('人保财险')
    expect(wrapper.text()).toContain('新增保险公司')
  })

  it('空名称不调用 create_insurer', async () => {
    const wrapper = mount(InsurerManager)
    const addBtn = wrapper.findAll('button').find((b) => b.text() === '添加')!
    await addBtn.trigger('click')
    expect(insurerCalls('create_insurer')).toHaveLength(0)
    expect(messageMock.warning).toHaveBeenCalled()
  })

  it('添加保司：调用 create_insurer，重拉后列表出现新保司、表单清空', async () => {
    stubReferenceInvoke({
      create_insurer: (args?: { input?: { name: string } }) => {
        insurerDb = [
          ...insurerDb,
          {
            id: 'ins-new', name: args!.input!.name,
            updated_at: '2026-01-01T00:00:00Z',
            version: 1, device_id: 'test', is_deleted: false,
          },
        ]
        return Promise.resolve('ins-new')
      },
      list_insurers: () => insurerDb,
    })
    const wrapper = mount(InsurerManager)
    const nameInput = wrapper
      .findAll('input')
      .find((i) => i.attributes('placeholder') === '保险公司名称')!
    await nameInput.setValue('泰康人寿')
    const addBtn = wrapper.findAll('button').find((b) => b.text() === '添加')!
    await addBtn.trigger('click')
    await flushPromises()

    expect(insurerCalls('create_insurer')).toHaveLength(1)
    expect(insurerCalls('create_insurer')[0][1]).toEqual({ input: { name: '泰康人寿' } })
    expect(messageMock.success).toHaveBeenCalled()
    // 表单清空
    expect(nameInput.element.value).toBe('')
    // 重拉后列表出现新保司（store 由失效信号驱动，测试中手动 refresh 模拟）
    await useReferenceStore().refresh()
    expect(wrapper.text()).toContain('泰康人寿')
  })

  it('重名创建失败：显示可理解的错误提示，表单不清空', async () => {
    stubReferenceInvoke({
      create_insurer: () => Promise.reject(new Error('参数错误: 保司已存在: 泰康人寿')),
      list_insurers: () => insurerDb,
    })
    const wrapper = mount(InsurerManager)
    const nameInput = wrapper
      .findAll('input')
      .find((i) => i.attributes('placeholder') === '保险公司名称')!
    await nameInput.setValue('泰康人寿')
    const addBtn = wrapper.findAll('button').find((b) => b.text() === '添加')!
    await addBtn.trigger('click')
    await flushPromises()

    expect(messageMock.error).toHaveBeenCalledWith('添加失败: Error: 参数错误: 保司已存在: 泰康人寿')
    // 表单不清空，用户可直接修正
    expect(nameInput.element.value).toBe('泰康人寿')
  })

  it('改名：编辑弹窗回填，保存调用 update_insurer，重拉后即时显示新名并关窗', async () => {
    const wrapper = mount(InsurerManager)
    const firstRow = wrapper.findAll('tbody tr')[0]!
    await firstRow.findAll('button').find((b) => b.text() === '编辑')!.trigger('click')
    await flushPromises()

    // 弹窗回填当前名
    const nameInput = findBodyInputByPlaceholder('保险公司名称')
    expect(nameInput.element.value).toBe('平安人寿')

    stubReferenceInvoke({
      update_insurer: (args?: { id?: string; input?: { name?: string } }) => {
        insurerDb = insurerDb.map((i) =>
          i.id === args!.id ? { ...i, name: args!.input!.name! } : i,
        )
        return Promise.resolve(undefined)
      },
      list_insurers: () => insurerDb,
    })
    await nameInput.setValue('平安人寿股份')
    const saveBtn = Array.from(document.body.querySelectorAll('button')).find(
      (b) => b.textContent?.trim() === '保存',
    )!
    await new DOMWrapper(saveBtn).trigger('click')
    await flushPromises()

    expect(insurerCalls('update_insurer')).toHaveLength(1)
    expect(insurerCalls('update_insurer')[0][1]).toEqual({
      id: 'ins-1',
      input: { name: '平安人寿股份' },
    })
    // 改名即时生效（引用指向 id）：重拉后列表显示新名
    await useReferenceStore().refresh()
    expect(wrapper.text()).toContain('平安人寿股份')
    // 保存成功关窗
    expect(
      wrapper.findComponent(InsurerEditModal).emitted('update:show')?.at(-1),
    ).toEqual([false])
  })

  it('改名重名失败：错误提示、弹窗不关、内容不丢', async () => {
    const wrapper = mount(InsurerManager)
    await wrapper.findAll('tbody tr')[0]!.findAll('button').find((b) => b.text() === '编辑')!.trigger('click')
    await flushPromises()

    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'update_insurer') {
        return Promise.reject(new Error('参数错误: 保司已存在: 人保财险'))
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const nameInput = findBodyInputByPlaceholder('保险公司名称')
    await nameInput.setValue('人保财险')
    const saveBtn = Array.from(document.body.querySelectorAll('button')).find(
      (b) => b.textContent?.trim() === '保存',
    )!
    await new DOMWrapper(saveBtn).trigger('click')
    await flushPromises()

    expect(messageMock.error).toHaveBeenCalledWith('更新失败: Error: 参数错误: 保司已存在: 人保财险')
    // 弹窗不关、内容不丢
    expect(
      wrapper.findComponent(InsurerEditModal).emitted('update:show'),
    ).toBeUndefined()
    expect(nameInput.element.value).toBe('人保财险')
  })

  it('软删：走行内确认弹层调用 delete_insurer，重拉后默认列表不含', async () => {
    const wrapper = mount(InsurerManager)
    const row = wrapper.findAll('tbody tr')[0]!
    await row.findAll('button').find((b) => b.text() === '删除')!.trigger('click')
    await flushPromises()
    // popconfirm 内容 teleport 到 body，直接对其组件 emit 正向点击（PoliciesView 先例）
    wrapper.findComponent(NPopconfirm).vm.$emit('positiveClick')
    await flushPromises()

    expect(insurerCalls('delete_insurer')).toHaveLength(1)
    expect(insurerCalls('delete_insurer')[0][1]).toEqual({ id: 'ins-1' })

    // 重拉后默认列表不含已删保司
    insurerDb = insurerDb.map((i) => (i.id === 'ins-1' ? { ...i, is_deleted: true } : i))
    await useReferenceStore().refresh()
    expect(rowTexts(wrapper)).toHaveLength(1)
    expect(wrapper.text()).toContain('人保财险')
  })
})

describe('InsurerManager.vue 拼音模糊搜索（issue #714，统一模糊搜索语义 ADR-0027）', () => {
  /** 五家保司：检索词构造覆盖原文子串（平安 ⊂ 平安人寿/平安财险）
   * 与拼音首字母子序列（rs ⊂ 人寿 initials、zabx = 众安保险 initials）双入口。 */
  const searchInsurers: Insurer[] = [
    '平安人寿', '平安财险', '中国人寿', '泰康人寿', '众安保险',
  ].map((name, i) => ({
    id: `ins-s-${i}`, name,
    updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  }))

  beforeEach(async () => {
    insurerDb = searchInsurers
    const store = useReferenceStore()
    await store.refresh()
  })

  function searchInput(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .findAll('input')
      .find((i) => i.attributes('placeholder') === '搜索保险公司（名称/拼音）')!
  }

  it('汉字关键字过滤：未命中项隐藏、命中项保留', async () => {
    const wrapper = mount(InsurerManager)
    await searchInput(wrapper).setValue('泰康')
    const rows = rowTexts(wrapper)
    expect(rows).toHaveLength(1)
    expect(rows[0]).toContain('泰康人寿')
  })

  it('拼音首字母入口：子序列命中（rs → 三家人寿按原序全中），不误命中无关节点', async () => {
    const wrapper = mount(InsurerManager)
    await searchInput(wrapper).setValue('rs')
    const rows = rowTexts(wrapper)
    // 人寿（renshou）是三家共有字样：子序列命中全部三家，顺序保持原序
    expect(rows).toHaveLength(3)
    expect(rows[0]).toContain('平安人寿')
    expect(rows[1]).toContain('中国人寿')
    expect(rows[2]).toContain('泰康人寿')

    // 财产险公司不误命中（tk ⊂ 泰康 initials tkrs，且不命中他行）
    await searchInput(wrapper).setValue('tk')
    expect(rowTexts(wrapper)).toHaveLength(1)
    expect(rowTexts(wrapper)[0]).toContain('泰康人寿')

    await searchInput(wrapper).setValue('zabx')
    expect(rowTexts(wrapper)).toHaveLength(1)
    expect(rowTexts(wrapper)[0]).toContain('众安保险')
  })

  it('只过滤不重排：命中项保持列表原有相对顺序（汉字与拼音双路径）', async () => {
    const wrapper = mount(InsurerManager)
    await searchInput(wrapper).setValue('平安')
    let rows = rowTexts(wrapper)
    expect(rows).toHaveLength(2)
    expect(rows[0]).toContain('平安人寿')
    expect(rows[1]).toContain('平安财险')

    // 拼音首字母子序列路径（pa ⊂ pingan）同序
    await searchInput(wrapper).setValue('pa')
    rows = rowTexts(wrapper)
    expect(rows).toHaveLength(2)
    expect(rows[0]).toContain('平安人寿')
    expect(rows[1]).toContain('平安财险')
  })

  it('清空搜索词恢复完整列表', async () => {
    const wrapper = mount(InsurerManager)
    await searchInput(wrapper).setValue('平安')
    expect(rowTexts(wrapper)).toHaveLength(2)
    await searchInput(wrapper).setValue('')
    expect(rowTexts(wrapper)).toHaveLength(5)
    expect(wrapper.text()).toContain('中国人寿')
    expect(wrapper.text()).toContain('众安保险')
  })
})

describe('InsurerManager.vue 显示已删切换（issue #714）', () => {
  const deletedInsurer: Insurer = {
    id: 'ins-del', name: '已裁撤保司',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: true,
  }

  beforeEach(async () => {
    insurerDb = [...mockInsurers, deletedInsurer]
    const store = useReferenceStore()
    await store.refresh()
  })

  /** 「显示已删」开关（checkbox 语义定位）。 */
  function showDeletedToggle(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('.n-checkbox').find((c) => c.text() === '显示已删')!
  }

  it('默认列表不含已软删保司', () => {
    const wrapper = mount(InsurerManager)
    expect(wrapper.text()).not.toContain('已裁撤保司')
    expect(rowTexts(wrapper)).toHaveLength(2)
  })

  it('切换「显示已删」后已删保司以行展示，条数照常计入列表', async () => {
    const wrapper = mount(InsurerManager)
    await showDeletedToggle(wrapper).trigger('click')
    expect(wrapper.text()).toContain('已裁撤保司')
    expect(rowTexts(wrapper)).toHaveLength(3)
  })

  it('已删行只读：无编辑/删除操作（在用行操作不受影响）', async () => {
    const wrapper = mount(InsurerManager)
    await showDeletedToggle(wrapper).trigger('click')
    expect(wrapper.findAll('button').filter((b) => b.text() === '编辑')).toHaveLength(2)
    expect(wrapper.findAll('button').filter((b) => b.text() === '删除')).toHaveLength(2)
    // 行级精确断言（按按钮而非裸文本，避免与「已删除」标记的子串相撞）
    const deletedRowEl = wrapper
      .findAll('tbody tr')
      .find((r) => r.text().includes('已裁撤保司'))!
    expect(
      deletedRowEl.findAll('button').filter((b) => b.text() === '编辑'),
    ).toHaveLength(0)
    expect(
      deletedRowEl.findAll('button').filter((b) => b.text() === '删除'),
    ).toHaveLength(0)
  })

  it('已删行带删除标记（与在用行可区分）', async () => {
    const wrapper = mount(InsurerManager)
    await showDeletedToggle(wrapper).trigger('click')
    const deletedRow = rowTexts(wrapper).find((r) => r.includes('已裁撤保司'))!
    expect(deletedRow).toContain('已删除')
  })

  it('搜索与显示已删叠加：搜索词对已删行同样过滤', async () => {
    const wrapper = mount(InsurerManager)
    await showDeletedToggle(wrapper).trigger('click')
    const search = wrapper
      .findAll('input')
      .find((i) => i.attributes('placeholder') === '搜索保险公司（名称/拼音）')!
    await search.setValue('已裁撤')
    const rows = rowTexts(wrapper)
    expect(rows).toHaveLength(1)
    expect(rows[0]).toContain('已裁撤保司')
  })
})
