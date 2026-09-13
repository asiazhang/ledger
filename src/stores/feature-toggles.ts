import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { clearClosedFeatures, getSavedClosedFeatures, saveClosedFeatures } from '@/utils/view-state'

/**
 * 功能开关状态基座（issue #1241 / ADR-0116 决策 2/6）：设备级「已关闭功能」闭集清单的
 * 唯一读写方——与侧栏顺序、收纳清单同族同归宿（localStorage、界面状态、不进 SQLite、
 * 无后端调用），跨账本共享（ADR-0017 轻量设置项，随本机不随账本迁移）。
 * 语义边界（ADR-0116 决策 1/3）：关闭只隐藏入口、不改写收纳清单——重新打开即回原位置。
 * 本票只做状态与读写：导航层过滤 / 设置页 UI / 路由守卫由后续票消费本 store，无用户可见变化。
 */

/**
 * 可关九项（开发者策展闭集，ADR-0116 决策 2）：预算、报表、定时、商户、投资、物品、
 * 保单、实物资产、保司。id 与侧栏视图名同词——后续消费面（侧栏 / 组「更多」 / 快捷键 /
 * 路由守卫 / 设置面）都按同一份视图名词表过滤，不另造映射层。改清单 = 修订 ADR。
 */
export const CLOSABLE_FEATURES = [
  'budget',
  'reports',
  'scheduled',
  'merchants',
  'investments',
  'items',
  'policies',
  'physicalAssets',
  'insurers',
] as const

/**
 * 不可关六项（ADR-0116 决策 2）：机制必需（交易 / 账户是记账动线基本依赖，设置承载
 * 开关本体——关掉即无回入口，属死锁）与定位必需（概览是启动落地页，搜索与 AI 导入是
 * 随时回头可用的检索与导入面）。写路径与解析防御一律拒绝之，闭集外能力面永不进入关闭集合。
 */
export const NON_CLOSABLE_FEATURES = [
  'dashboard',
  'transactions',
  'accounts',
  'search',
  'ai',
  'settings',
] as const

export type ClosableFeatureId = (typeof CLOSABLE_FEATURES)[number]
export type NonClosableFeatureId = (typeof NON_CLOSABLE_FEATURES)[number]

/** 闭集判定：仅可关九项为真（不可关六项、未知 id、非字符串一律为假）。 */
export function isClosableFeature(v: unknown): v is ClosableFeatureId {
  return typeof v === 'string' && (CLOSABLE_FEATURES as readonly string[]).includes(v)
}

/**
 * 关闭集合解析（纯函数，比照 parseGroupOrders / parseContainmentLists 口径）：
 * 已存原始值 → 解析后的合法关闭集合。
 * 整体形状防御：仅接受数组——非数组（null、标量、字符串、对象等）整体回退默认「全开」= 空集合。
 * 项级防御：只留可关九项（未知 id、不可关六项、非字符串项一律非法过滤）→ 去重。
 * 集合语义：关闭集合无序，输出归一为 CLOSABLE_FEATURES 清单序，解析结果稳定可复现。
 */
export function parseClosedFeatures(raw: unknown): ClosableFeatureId[] {
  if (!Array.isArray(raw)) return []
  const present = new Set<string>()
  for (const item of raw) {
    if (isClosableFeature(item)) present.add(item)
  }
  return CLOSABLE_FEATURES.filter((id) => present.has(id))
}

/**
 * 功能开关 store：设备级关闭集合的单一归宿（ViewState 词条覆盖的持久界面状态）。
 * 消费方（后续票）：侧栏菜单构建、GroupMoreView 页签、useViewShortcuts 键位带、
 * 路由守卫、设置页「功能」Tab 与「定时」Tab 联动。
 */
export const useFeatureToggleStore = defineStore('feature-toggles', () => {
  // 启动读路径：原始值经解析防御——脏数据整体回退全开、非法项过滤、去重（issue #1241）。
  const closed = ref<ClosableFeatureId[]>(parseClosedFeatures(getSavedClosedFeatures()))

  /** 已关闭功能集合（只读派生，清单序）：全部消费面的唯一读路径；写路径不经它。 */
  const closedFeatures = computed<readonly ClosableFeatureId[]>(() => closed.value)

  /** 关闭态查询（运行时）：不可关六项与未知 id 恒为 false（闭集外永不关闭）。 */
  function isFeatureClosed(id: unknown): boolean {
    return typeof id === 'string' && (closed.value as readonly string[]).includes(id)
  }

  /** 写路径（点选即写）：关闭 / 打开某项后立即持久化；集合清空即删除记录（默认态 = 无记录）。
   *  边界 no-op 不写存储：不可关项 / 未知 id、以及目标态与当前态相同时一律原样返回。 */
  function setFeatureClosed(id: unknown, shouldClose: boolean) {
    if (!isClosableFeature(id)) return
    if (shouldClose === closed.value.includes(id)) return
    const present = new Set<ClosableFeatureId>(closed.value)
    if (shouldClose) present.add(id)
    else present.delete(id)
    const next = CLOSABLE_FEATURES.filter((f) => present.has(f))
    closed.value = next
    if (next.length === 0) clearClosedFeatures()
    else saveClosedFeatures(next)
  }

  return {
    closedFeatures,
    isFeatureClosed,
    setFeatureClosed,
  }
})
