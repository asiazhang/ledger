// 视图标题文案（common.nav.<路由 name>，issue #342）：侧栏菜单与内容区标题同源，
// 随界面语言即时切换；路由 meta.title 已删除（避免第二份单语清单漂移）。
// key 构造收口在此并配单元测试——消息树顶层键即域名，漏写 common. 前缀会
// 原样渲染 key 代号（回归先例：侧栏整排显示 nav.dashboard）。
import { t } from './index'

/** 视图名 → 界面文案（如 dashboard → 概览 / Overview） */
export function viewLabel(name: string): string {
  return t(`common.nav.${name}`)
}
