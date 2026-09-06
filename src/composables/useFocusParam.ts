import type { LocationQuery } from 'vue-router'

/**
 * focus 消费助手（spec #704 / issue #705，词汇表「实体定位参数（focus 参数）」）：
 * 落地视图侧对路由 query 中 focus 参数的读一次语义唯一实现——目标视图挂载时
 * 消费一次即失效，视图只写「拿到 id 后做什么」（开弹窗 / 滚动高亮 / 走势选中），
 * 读一次时序全部内化本工厂。四个目标视图各写一遍必然漂移（spec #704 动机），
 * 与 useModalIntent / useRowContextMenu 同型：工厂形态 composable，命名照先例。
 *
 * 语义（词汇表「统一参数名、消费一次」+ issue #705 验收标准 3）：
 * - 挂载时消费一次即失效：consume() 的第一次调用耗尽本实例唯一一次读取——
 *   focus 在场则回调并封闸，此后重复 / 迟到消费一律丢弃（页签切换 router.replace
 *   会保留 query 里的 focus，读一次语义使「URL 残留反复弹窗 / 高亮」从机制上消亡）。
 * - 无 focus 安全空转：首次 consume 读不到可用 id 则静默封闸、不回调（空转也是
 *   一次——不保留「稍后再消费」，同实例后续出现的 focus 一律视为迟到意图）。
 * - URL 在场刷新可重定位：工厂不持久化任何状态，刷新 / 重进视图 = 新实例 =
 *   重新消费；focus 不写回 URL（工厂无 router 依赖、只读 query，机制上写不回），
 *   URL 在场即重新定位、深链可分享（刷新自然复现）。
 * - 消费时机归视图：常规接线为挂载时 consume()（onMounted）；先拿 id 后等数据
 *   到位的视图（如保单行高亮）在回调里暂存 id、渲染后生效——读取时刻与生效
 *   时刻解耦，时序仍只有一份。
 *
 * 纯度：零外部依赖（不接 router、不接 store、不接组件）——query 经 getter 注入
 * （调用方传 () => route.query），测试以普通对象直打（先例：弹窗意图编排工厂
 * 测试形态）。跳转入口侧（来源点击 → 路由目标）见 source-jump.ts，两文件共同
 * 构成来源跳转深模块。
 */

/** focus 参数词表：一名一义（不按页语义命名，避免与下钻过滤参数混淆）。 */
export const FOCUS_QUERY_KEY = 'focus'

export interface UseFocusParamOptions {
  /** focus 读取源：目标视图的路由 query（getter 注入，工厂不依赖 router）。 */
  query: () => LocationQuery
  /** 消费回调：focus 在场时触发一次，携带实体 id；回调体 100% 是视图业务代码。 */
  onFocus: (entityId: string) => void
}

export interface UseFocusParamReturn {
  /** 消费：本实例仅首次调用生效（在场则回调、空转则封闸），此后一律丢弃。 */
  consume(): void
}

/**
 * focus 消费助手工厂：每次调用返回独立实例（消费闸门不串扰——同一 query 上
 * 多实例各自消费一次，是「刷新 / 重进视图重定位」的机制基础）。
 */
export function useFocusParam(options: UseFocusParamOptions): UseFocusParamReturn {
  let spent = false

  return {
    consume() {
      if (spent) return
      spent = true
      const entityId = readFocusId(options.query())
      if (entityId !== null) options.onFocus(entityId)
    },
  }
}

/**
 * query 中读出实体 id：取 focus 首个非空字符串值（重复键取第一、数组首元
 * null 按缺席）；无可用值返回 null（空转）。
 */
function readFocusId(query: LocationQuery): string | null {
  const raw = query[FOCUS_QUERY_KEY]
  const value = Array.isArray(raw) ? raw[0] : raw
  return typeof value === 'string' && value !== '' ? value : null
}
