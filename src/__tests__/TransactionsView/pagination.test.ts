import { mockInvoke, merchantDb, mockCurrencies, mockAccounts, mountView, mountViewSync, listCalls, lastListFilter, tablePagination, bodyRows, openMenuOnRow, selectRowMenu, clickDialogButton } from './common'
import { describe, it, expect } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { NDataTable } from 'naive-ui'

describe('TransactionsView 服务端分页', () => {
  it('默认以 page=1 page_size=20 查询并渲染「共 N 条」总数', async () => {
    const wrapper = await mountView()
    expect(lastListFilter()).toMatchObject({ page: 1, page_size: 20 })
    expect(wrapper.text()).toContain('共 45 条')
    // 只渲染当前页数据（不全量加载）
    expect(bodyRows(wrapper).length).toBe(20)
  })

  it('翻页以新的 page 重新查询', async () => {
    const wrapper = await mountView()
    const before = listCalls().length
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    expect(listCalls().length).toBe(before + 1)
    expect(lastListFilter()).toMatchObject({ page: 2, page_size: 20 })
    expect(bodyRows(wrapper).length).toBe(20)
  })

  it('切换页大小以新的 page_size 查询并重置到第 1 页', async () => {
    const wrapper = await mountView()
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    tablePagination(wrapper).onUpdatePageSize(50)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 1, page_size: 50 })
    expect(bodyRows(wrapper).length).toBe(45)
  })

  it('删除当前页一条后以当前页码刷新', async () => {
    const wrapper = await mountView()
    tablePagination(wrapper).onChange(2)
    await flushPromises()
    const before = listCalls().length
    await openMenuOnRow(wrapper, 0)
    await selectRowMenu(wrapper, 'delete')
    await clickDialogButton('删除')
    await flushPromises()
    // 第 2 页原本 20 条，删 1 条不触发回退，仍刷新第 2 页
    expect(listCalls().length).toBe(before + 1)
    expect(lastListFilter()).toMatchObject({ page: 2, page_size: 20 })
  })

  it('删除当前页最后一条后回退到上一页避免空页', async () => {
    const wrapper = await mountView()
    tablePagination(wrapper).onChange(3) // 第 3 页共 5 条
    await flushPromises()
    expect(bodyRows(wrapper).length).toBe(5)
    // 删除前 4 条（每删一条都会重新渲染，行列表需重新获取）
    for (let i = 0; i < 4; i++) {
      await openMenuOnRow(wrapper, 0)
      await selectRowMenu(wrapper, 'delete')
      await clickDialogButton('删除')
      await flushPromises()
    }
    expect(bodyRows(wrapper).length).toBe(1)
    // 删除最后一条 → 自动回退到第 2 页，不出现空页
    await openMenuOnRow(wrapper, 0)
    await selectRowMenu(wrapper, 'delete')
    await clickDialogButton('删除')
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 2, page_size: 20 })
    expect(bodyRows(wrapper).length).toBe(20)
  })

  it('查询期间 loading 状态可见', async () => {
    let resolveList!: (v: unknown) => void
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve(merchantDb)
      if (cmd === 'list_transactions') {
        return new Promise((resolve) => {
          resolveList = resolve
        })
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = mountViewSync()
    await flushPromises()
    // 参考数据已就绪（self-init），list_transactions 挂起中 → loading 应为 true
    expect(wrapper.findComponent(NDataTable).props('loading')).toBe(true)
    resolveList({ items: [], total: 0 })
    await flushPromises()
    expect(wrapper.findComponent(NDataTable).props('loading')).toBe(false)
  })
})

