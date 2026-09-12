import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { NDataTable } from 'naive-ui'
import { setFakeMedia } from './media-mock'

/**
 * 移动档表格横向滚动下限断言（issue #849 / ADR-0088 决策 11 票⑨，收纳页与标的页
 * 核对级适配共享）：宽度轴落移动档（<840）时表格挂 scroll-x = 固定列宽总和——
 * 窄屏由横向滚动吸收、列不压碎（词汇表「表格列形态」）；桌面档不挂零变化。
 * mountView 由调用方提供（各测试自带接缝布线与 Provider 包裹差异），返回 wrapper 即可。
 */
export async function assertMobileTierScrollX(mountView: () => VueWrapper): Promise<void> {
  setFakeMedia({ width: 400, hover: 'hover', pointer: 'fine' })
  const mobile = mountView()
  await flushPromises()
  const mobileTable = mobile.findComponent(NDataTable)
  const columns = mobileTable.props('columns') as unknown as Array<{ width?: number }>
  const fixedSum = columns.reduce(
    (sum, c) => sum + (typeof c.width === 'number' ? c.width : 0),
    0,
  )
  expect(fixedSum).toBeGreaterThan(0)
  expect(mobileTable.props('scrollX')).toBe(fixedSum)

  setFakeMedia({ width: 1280, hover: 'hover', pointer: 'fine' })
  const desktop = mountView()
  await flushPromises()
  expect(desktop.findComponent(NDataTable).props('scrollX')).toBeUndefined()
}
