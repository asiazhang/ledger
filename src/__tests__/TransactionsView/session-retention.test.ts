import { describe, it, expect, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { defineComponent, h } from 'vue'
import { NSelect } from 'naive-ui'
import { routeMock, makeTxn, setTxnDb, mountView, listCalls, lastListFilter, bodyRows, tablePagination, pushMock } from './common'
import { mockInvoke } from '../helpers/invoke-mock'
import { useWindowGuard } from '@/composables/useWindowGuard'
import { createOverlayToken, resetOverlays } from '@/composables/overlayRegistry'
import { fireViewReset, clearViewResets } from '@/composables/viewResetRegistry'
import type { VueWrapper } from '@vue/test-utils'

/**
 * TransactionsView 会话内保留（issue #893 / ADR-0094，spec #892）：只测外部行为——
 * 断言可观察的请求参数与渲染结果。「保留」的判据是同会话卸载重挂（侧栏切换、下钻
 * 往返、回退的共同形态）后以恢复状态现拉；「冷启动」的判据是新 pinia 回默认；
 * 「零写盘」的判据是全程无 localStorage 写入（报表页保留测试先例的三锚点）。
 */

/** 账户下拉 = 第 1 个 NSelect（PinyinSelect 内层，filtering.test 同款定位）。 */
async function setAccount(wrapper: VueWrapper, id: string | null) {
  wrapper.findAllComponents(NSelect)[0].vm.$emit('update:value', id)
  await flushPromises()
}

/** 默认 45 行库的生成（与 common.beforeEach 同构，供测试内自建数据集）。 */
function defaultDb(count = 45) {
  return Array.from({ length: count }, (_, i) => makeTxn(i + 1, i % 2 === 0 ? 'acc-2' : 'acc-1'))
}

beforeEach(() => {
  resetOverlays()
  clearViewResets()
})

describe('TransactionsView 会话内保留（issue #893）：同会话卸载重挂恢复，新会话冷启动', () => {
  it('筛选 + 翻页后卸载重挂（同一会话）：以恢复状态现拉，离开期间新账进来即见，不写回 URL', async () => {
    const first = await mountView()
    await setAccount(first, 'acc-1')
    tablePagination(first).onChange(2)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 2, page_size: 20, involving_account_id: 'acc-1' })
    first.unmount()

    // 离开期间新记的账：默认 45 行（22 行在 acc-1）追加 1 行 acc-1 → 23 行
    setTxnDb(defaultDb(46))
    mockInvoke.mockClear()
    const second = await mountView()
    // 以恢复的筛选与页码重新查询（不是缓存快照）
    expect(lastListFilter()).toMatchObject({ page: 2, page_size: 20, involving_account_id: 'acc-1' })
    expect(second.text()).toContain('共 23 条')
    expect(bodyRows(second).length).toBe(3) // 第 2 页 3 行
    // 保留不写回 URL：重挂本身不产生任何跳转
    expect(pushMock).not.toHaveBeenCalled()
    second.unmount()
  })

  it('新 pinia 表达冷启动：回默认无筛选态、第 1 页', async () => {
    const first = await mountView()
    await setAccount(first, 'acc-1')
    tablePagination(first).onChange(2)
    await flushPromises()
    first.unmount()

    // 新 pinia = 新会话（应用重启）：回默认
    setActivePinia(createPinia())
    mockInvoke.mockClear()
    const second = await mountView()
    const f = lastListFilter()
    expect(f).toMatchObject({ page: 1, page_size: 20 })
    expect(f).not.toHaveProperty('involving_account_id')
    expect(second.text()).toContain('共 45 条')
    second.unmount()
  })

  it('会话内保留零持久化：选择筛选与卸载重挂全程 localStorage 零写入', async () => {
    const first = await mountView()
    const keysBefore = Object.keys(localStorage)
    await setAccount(first, 'acc-1')
    tablePagination(first).onChange(2)
    await flushPromises()
    first.unmount()
    const second = await mountView()
    expect(lastListFilter()).toMatchObject({ page: 2, involving_account_id: 'acc-1' })
    expect(Object.keys(localStorage)).toEqual(keysBefore)
    second.unmount()
  })

  it('URL 下钻参数在场永远赢：覆盖保留态对应维度；无参数回访恢复离开时的选择（两种往返互不干扰）', async () => {
    const first = await mountView()
    await setAccount(first, 'acc-1') // 手动筛选留在会话里
    tablePagination(first).onChange(2)
    await flushPromises()
    first.unmount()

    // 下钻往返：带 account 参数进入 → 显式跳转意图覆盖账户维度、翻页归零
    routeMock.query = { account: 'acc-2' }
    const second = await mountView()
    expect(lastListFilter()).toMatchObject({ page: 1, page_size: 20, involving_account_id: 'acc-2' })
    expect(second.text()).toContain('共 23 条')
    second.unmount()

    // 侧栏往返（无参数）：恢复最近一次离开时的选择（下钻落点本身），不被复位回默认
    routeMock.query = {}
    setTxnDb(defaultDb())
    mockInvoke.mockClear()
    const third = await mountView()
    expect(lastListFilter()).toMatchObject({ page: 1, page_size: 20, involving_account_id: 'acc-2' })
    expect(wrapperText(third)).toContain('共 23 条')
    third.unmount()
  })

  it('页码恢复钳制：恢复页码超出当前数据有效范围时回落，不落空页（走既有回退出口）', async () => {
    const first = await mountView() // 默认 45 行：第 3 页 5 行
    tablePagination(first).onChange(3)
    await flushPromises()
    expect(bodyRows(first).length).toBe(5)
    first.unmount()

    // 离开期间数据缩到 1 页（20 行）
    setTxnDb(defaultDb(20))
    mockInvoke.mockClear()
    const second = await mountView()
    // 恢复页码 3 → 空页自愈逐页回落至有效范围（第 2 页同样超界 → 第 1 页）
    const pages = listCalls().map(([, args]) => (args as { filter: { page?: number } }).filter.page)
    expect(pages[0]).toBe(3) // 先按恢复页码请求
    expect(pages[pages.length - 1]).toBe(1) // 钳制回落
    expect(bodyRows(second).length).toBe(20)
    expect(tablePagination(second).page).toBe(1)
    second.unmount()
  })
})

describe('TransactionsView ESC 复位接线（spec #892）：注册 → 守卫消费 → 清除保留态本身', () => {
  /** 窗口行为守卫宿主（App.vue 同构：守卫全局唯一）。 */
  function mountGuardHost() {
    const Host = defineComponent({
      setup() {
        useWindowGuard()
        return () => h('div')
      },
    })
    return mount(Host)
  }

  function fireEscape() {
    document.body.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
    )
  }

  it('无弹层 ESC 触发模块复位出口：清全部过滤 + 翻页归零 + 重拉；复位后离开再回来 = 默认', async () => {
    const guard = mountGuardHost()
    const first = await mountView()
    await setAccount(first, 'acc-1')
    tablePagination(first).onChange(2)
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ page: 2, involving_account_id: 'acc-1' })

    fireEscape()
    await flushPromises()
    const f = lastListFilter()
    expect(f).toMatchObject({ page: 1, page_size: 20 })
    expect(f).not.toHaveProperty('involving_account_id')
    first.unmount()

    // 复位即清除保留态本身：复位后卸载重挂 = 默认态
    mockInvoke.mockClear()
    const second = await mountView()
    const f2 = lastListFilter()
    expect(f2).toMatchObject({ page: 1 })
    expect(f2).not.toHaveProperty('involving_account_id')
    expect(second.text()).toContain('共 45 条')
    second.unmount()
    guard.unmount()
  })

  it('无过滤时 ESC 幂等：不产生重拉（「无保留状态无操作」覆盖 ESC 通道）', async () => {
    const guard = mountGuardHost()
    const first = await mountView()
    await flushPromises()
    const before = listCalls().length
    fireEscape()
    await flushPromises()
    expect(listCalls().length).toBe(before)
    first.unmount()
    guard.unmount()
  })

  it('有弹层时 ESC 不复位：弹层库默认关闭行为接管，保留态不动（一次按键只做一件事）', async () => {
    const guard = mountGuardHost()
    const first = await mountView()
    await setAccount(first, 'acc-1')
    await flushPromises()
    // 弹层注册表登记打开的弹层（AppModal/AppSelect 等封装组件的上报形态）
    const token = createOverlayToken('modal')
    token.set(true)
    fireEscape()
    await flushPromises()
    expect(lastListFilter()).toMatchObject({ involving_account_id: 'acc-1' })
    token.set(false)
    first.unmount()
    guard.unmount()
  })

  it('视图卸载后复位注册自动撤销：守卫消费不再触达本视图（导航离开不滞留注册态）', async () => {
    const guard = mountGuardHost()
    const first = await mountView()
    expect(fireViewReset()).toBe(true) // 默认态：复位出口幂等不动作
    await setAccount(first, 'acc-1')
    await flushPromises()
    first.unmount()
    expect(fireViewReset()).toBe(false)
    guard.unmount()
  })
})

/** 视图文本（NDialogProvider 包裹挂载，wrapper.text 直取）。 */
function wrapperText(wrapper: VueWrapper): string {
  return wrapper.text()
}
