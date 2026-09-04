import { describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { useRowContextMenu } from '@/composables/useRowContextMenu'

/**
 * RowContextMenu（行右键菜单编排）模块测试（spec #522 / issue #550）：
 * 工厂零外部依赖（无 store、无 api、无组件，只等下一帧），裸调直打接口——
 * open/close/select 的状态终态与「收起 → 下一帧重开」重定位舞步，
 * select 收尾回调的 (key, row) 交付；不断言内部 nextTick 接线细节。
 */

/** 测试用目标行：工厂对行类型泛型、只存储回传、永不读行内容。 */
interface TestRow {
  id: string
  label: string
}

/** 测试用鼠标事件：工厂只读 clientX/clientY 坐标（不调 preventDefault）。 */
function mouseAt(x: number, y: number): MouseEvent {
  return { clientX: x, clientY: y } as MouseEvent
}

const rowA: TestRow = { id: 'row-a', label: 'A' }
const rowB: TestRow = { id: 'row-b', label: 'B' }

describe('useRowContextMenu 初始状态', () => {
  it('状态为 null（关闭终态，可见性由非空派生）', () => {
    const menu = useRowContextMenu<TestRow>(vi.fn())
    expect(menu.state.value).toBeNull()
  })
})

describe('useRowContextMenu open（统一「收起 → 下一帧重开」单路径）', () => {
  it('从关闭态打开：同样延迟一帧，下一帧落位事件坐标与目标行（无快路径）', async () => {
    const menu = useRowContextMenu<TestRow>(vi.fn())
    menu.open(mouseAt(100, 200), rowA)
    // 同步批次内尚未落位（单路径：一律下一帧重开）
    expect(menu.state.value).toBeNull()
    await nextTick()
    expect(menu.state.value).toEqual({ x: 100, y: 200, row: rowA })
  })

  it('已开时 open 重定位：状态过 null 一跳，下一帧新坐标新行重开', async () => {
    const menu = useRowContextMenu<TestRow>(vi.fn())
    menu.open(mouseAt(100, 200), rowA)
    await nextTick()
    expect(menu.state.value).toEqual({ x: 100, y: 200, row: rowA })

    menu.open(mouseAt(300, 400), rowB)
    // 重定位同步收起：过 null 一跳（可见性瞬断即重定位舞步）
    expect(menu.state.value).toBeNull()
    await nextTick()
    expect(menu.state.value).toEqual({ x: 300, y: 400, row: rowB })
  })

  it('同一同步批次内连续 open：最后一次开启胜出', async () => {
    const menu = useRowContextMenu<TestRow>(vi.fn())
    menu.open(mouseAt(100, 200), rowA)
    menu.open(mouseAt(300, 400), rowB)
    await nextTick()
    expect(menu.state.value).toEqual({ x: 300, y: 400, row: rowB })
  })
})

describe('useRowContextMenu select（收起并交付收起瞬间的 (key, row)）', () => {
  it('收起菜单，回调收到收起瞬间的目标行（捕获先于清空）', async () => {
    const onSelect = vi.fn()
    const menu = useRowContextMenu<TestRow>(onSelect)
    menu.open(mouseAt(100, 200), rowA)
    await nextTick()

    menu.select('edit')
    // 回调先于/独立于下一帧：交付的是收起瞬间的 (key, row)
    expect(onSelect).toHaveBeenCalledTimes(1)
    expect(onSelect).toHaveBeenCalledWith('edit', rowA)
    // 菜单已收起：清回全空终态
    expect(menu.state.value).toBeNull()
    await nextTick()
    expect(menu.state.value).toBeNull()
  })

  it('菜单未开时 select：只收起（无状态可交付），回调不触发', () => {
    const onSelect = vi.fn()
    const menu = useRowContextMenu<TestRow>(onSelect)
    menu.select('delete')
    expect(onSelect).not.toHaveBeenCalled()
    expect(menu.state.value).toBeNull()
  })
})

describe('useRowContextMenu close', () => {
  it('close 清回全空终态，无滞留目标行', async () => {
    const menu = useRowContextMenu<TestRow>(vi.fn())
    menu.open(mouseAt(100, 200), rowA)
    await nextTick()
    menu.close()
    expect(menu.state.value).toBeNull()
  })
})

describe('useRowContextMenu 工厂形态（每次调用独立实例）', () => {
  it('两个实例互不串扰', async () => {
    const menu1 = useRowContextMenu<TestRow>(vi.fn())
    const menu2 = useRowContextMenu<TestRow>(vi.fn())
    menu1.open(mouseAt(100, 200), rowA)
    await nextTick()
    expect(menu1.state.value).toEqual({ x: 100, y: 200, row: rowA })
    expect(menu2.state.value).toBeNull()
    menu2.open(mouseAt(300, 400), rowB)
    await nextTick()
    expect(menu1.state.value).toEqual({ x: 100, y: 200, row: rowA })
    expect(menu2.state.value).toEqual({ x: 300, y: 400, row: rowB })
    menu1.close()
    expect(menu1.state.value).toBeNull()
    expect(menu2.state.value).toEqual({ x: 300, y: 400, row: rowB })
  })
})
