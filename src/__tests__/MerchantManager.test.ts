import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import MerchantManager from '@/components/MerchantManager.vue'
import type { Merchant } from '@/types'

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

const mockInvoke = vi.mocked(invoke)

const mockMerchants: Merchant[] = [
  {
    id: 'mch-1', name: '京东', icon: null, color: '#e37318',
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
  {
    id: 'mch-2', name: '红旗连锁', icon: null, color: null,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
]

let merchantDb: Merchant[] = mockMerchants

function mockBaseCommands() {
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve([])
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve(merchantDb)
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
    mockBaseCommands()
    messageMock.success.mockClear()
    messageMock.error.mockClear()
    messageMock.warning.mockClear()
    const store = useReferenceStore()
    await store.ensureFresh()
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
            id: 'mch-new', name: args!.input!.name, icon: null, color: null,
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
    expect(merchantCalls('create_merchant')[0][1]).toEqual({ input: { name: '盒马', icon: null, color: null } })
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
