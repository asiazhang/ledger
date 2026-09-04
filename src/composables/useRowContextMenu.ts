import { nextTick, readonly, ref } from 'vue'
import type { Ref } from 'vue'

/**
 * useRowContextMenu 行右键菜单编排工厂（spec #522 / issue #550，词汇表
 * 「RowContextMenu（行右键菜单编排）」）：行菜单「打开 / 重定位 / 关闭 / 选中」
 * 全部时序的单一实现——调用方以事件坐标与目标行声明打开意图，工厂内化
 * 「单判别状态非空即显示、重定位统一收起下一帧重开、选中即收起并交付收起瞬间
 * 的 (key, row)」的完整编排。命名照 useModalIntent / useTransactionFilter 先例
 * （工厂形态 composable，ADR-0030）。
 *
 * 收编的四份同构拷贝（原各自维护 show/x/y/row 四状态 + 一段非显然定位时序）：
 * 交易行（TransactionsView）、账户行（AccountsView）、侧栏排序菜单（App.vue）、
 * 组内收纳页签菜单（GroupMoreView）——后两份为目标语义不同的非行拷贝，排除裁决
 * 见 ADR-0077；行拷贝的迁移以「既有视图测试零断言变化全绿」为等价证据。
 *
 * 接口四面：
 * - state：单判别状态 `{ x, y, row } | null`（只读），可见性由非空派生、
 *   无独立开关布尔；close 清回全空终态，无滞留行，「行非空但菜单已关」的
 *   非法中间态由形态消灭（ADR-0045「show 由意图派生」同款）；
 * - open(event, row)：幂等升级——未开即开、已开即重定位；统一「收起 → 下一帧
 *   重开」舞步单路径，从关闭态打开同样延迟一帧（与既有行为严格等价，不做快
 *   路径优化）；同一同步批次内连续 open，最后一次开启胜出；
 * - close()：清回全空终态；
 * - select(key)：收起菜单并把**收起瞬间**的 (key, row) 交给工厂入参回调
 *   onSelect——「捕获目标行要在收起前」的时序内化进工厂（回调注入先例：
 *   ADR-0041 ScheduledPlanList）；回调体 100% 是视图业务代码，工厂不认识任何
 *   key；菜单未开时无可交付，回调不触发。
 *
 * 事件纪律与弹层纯度：工厂不调 preventDefault、只读事件坐标（原生菜单拦截
 * 单点归窗口行为守卫，issue #154）；不接弹层注册表（ADR-0035——开/关上报由
 * 既有薄封装 AppDropdown 的 attrs watch 承担，视图以 `:show` 绑定即自动生效）；
 * 菜单选项构建、菜单项结构与业务动作分派全留视图。
 */

/** 行右键菜单单判别状态：非空即菜单打开（x/y 为弹出坐标、row 为目标行）。 */
export interface RowContextMenuState<TRow> {
  x: number
  y: number
  row: TRow
}

export interface UseRowContextMenuReturn<TRow> {
  /** 当前状态（只读）：null = 关闭终态（菜单不显示）；非空即「菜单显示」。 */
  readonly state: Readonly<Ref<RowContextMenuState<TRow> | null>>
  /** 打开（未开即开、已开即重定位）：收起 → 下一帧以事件坐标与目标行重开。 */
  open(event: MouseEvent, row: TRow): void
  /** 关闭：清回全空终态（无滞留目标行）。 */
  close(): void
  /** 选中：收起菜单并把收起瞬间的 (key, row) 交给工厂入参回调 onSelect。 */
  select(key: string | number): void
}

/**
 * 行右键菜单编排工厂：每次调用返回独立实例（状态与回调不串扰）。
 * TRow 由调用方声明为目标行类型；工厂永不读行内容、只存储回传（纪律门槛）。
 */
export function useRowContextMenu<TRow>(
  onSelect: (key: string | number, row: TRow) => void,
): UseRowContextMenuReturn<TRow> {
  const state = ref(null) as Ref<RowContextMenuState<TRow> | null>

  function open(event: MouseEvent, row: TRow) {
    // 坐标在调用瞬间捕获（事件对象随传播结束失效）；先收起再下一帧重开，
    // 已开重定位与关闭态打开同一路径（单路径，无快路径）。
    const x = event.clientX
    const y = event.clientY
    state.value = null
    void nextTick(() => {
      state.value = { x, y, row }
    })
  }

  function close() {
    state.value = null
  }

  function select(key: string | number) {
    // 捕获先于收起：交付「收起瞬间」的目标行；未开时无可交付，只收起。
    const current = state.value
    close()
    if (current) onSelect(key, current.row)
  }

  return {
    // 泛型 TRow 下 readonly() 的 DeepReadonly 无法结构赋值给 Readonly<Ref<...>>，
    // 在接缝处显式收窄（消费方只读 .value，行为不变；先例 useModalIntent）。
    state: readonly(state) as Readonly<Ref<RowContextMenuState<TRow> | null>>,
    open,
    close,
    select,
  }
}
