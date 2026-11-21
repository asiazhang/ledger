import { computed, onScopeDispose, ref } from "vue";
import type { ComputedRef } from "vue";

/**
 * 输入轴（Input Mode，ADR-0088 决策 6 / 词汇表「输入轴」）：交互形态轴唯一事实源，
 * 与窗口分级的宽度轴正交——宽度档位不决定交互形态（平板横屏 = 桌面档 + 触控轴）。
 * 本模块只做「hover / pointer 信号 → 输入轴」纯映射，交互替换的落地
 * （快捷键退役渲染、hover 一击可达等）归消费方与各实施票。
 */

/** hover 能力查询：主指针环境可悬停（鼠标 / 触控板）。测试消费同一常量（唯一事实源）。 */
export const HOVER_QUERY = "(hover: hover)";
/** 主指针精度查询：精细主指针。测试消费同一常量（唯一事实源）。 */
export const FINE_POINTER_QUERY = "(pointer: fine)";

/** 输入轴闭集：触控轴 / 指针轴。 */
export type InputMode = "touch" | "pointer";

/**
 * 输入轴映射矩阵（保守方向：宁可触控轴兜底，不可指针轴漏判）——
 * - hover 可用 **且** 主指针精细 → 指针轴（桌面鼠标 / 触控板）；
 * - 其余全部组合 → 触控轴：不可悬停或主指针粗糙，任一触控信号成立即触控轴。
 *   误判触控的代价是多一份一击可达替代（常显按钮、点按展开），
 *   误判指针的代价是悬停盲区与快捷键失达，后者不可接受。
 */

// —— 模块级共享订阅单例（issue #2187）——
// 两路信号是模块级 ref，hover / pointer 查询的监听是模块级闭集：同页数百个
// 消费方共享固定两路媒体查询监听（引用计数管理登记/解绑），不再每消费者
// 各自注册独立监听。hover / pointer 查询收口于本文件，与断点常量并列为
// 两根轴的唯一事实源。

const canHover = ref(false);
const hasFinePointer = ref(false);

const onHoverChange = (event: MediaQueryListEvent): void => {
  canHover.value = event.matches;
};
const onPointerChange = (event: MediaQueryListEvent): void => {
  hasFinePointer.value = event.matches;
};

/** 当前持有监听的查询对象（引用计数归零解绑后置空）。 */
let boundHoverMql: MediaQueryList | null = null;
let boundPointerMql: MediaQueryList | null = null;
/** 在册消费作用域计数：> 0 期间维持共享监听，归零即干净解绑（零泄漏）。 */
let consumerCount = 0;

/**
 * 换绑到当前宿主发放的查询对象：每次消费登记都重新取 matchMedia 并重绑。
 * 生产环境该成本可忽略；在测试环境中它天然感知媒体查询宿主替换——
 * 测试接缝复位会清空注册表使旧查询对象失联（不再接收 change），
 * 新消费登记重绑到当前宿主的新对象并按当前状态重读信号，用例间复位无残留污染。
 */
function bindSharedListeners(): void {
  const hoverMql = window.matchMedia(HOVER_QUERY);
  const finePointerMql = window.matchMedia(FINE_POINTER_QUERY);
  if (boundHoverMql) boundHoverMql.removeEventListener("change", onHoverChange);
  if (boundPointerMql) boundPointerMql.removeEventListener("change", onPointerChange);
  boundHoverMql = hoverMql;
  boundPointerMql = finePointerMql;
  hoverMql.addEventListener("change", onHoverChange);
  finePointerMql.addEventListener("change", onPointerChange);
  // 重读当前状态：换档在无消费者期间发生时（浏览器侧或测试接缝重编程），
  // 共享 ref 不会收到 change，登记时以宿主当前应答校准。
  canHover.value = hoverMql.matches;
  hasFinePointer.value = finePointerMql.matches;
}

function releaseSharedListeners(): void {
  consumerCount -= 1;
  if (consumerCount > 0) return;
  boundHoverMql?.removeEventListener("change", onHoverChange);
  boundPointerMql?.removeEventListener("change", onPointerChange);
  boundHoverMql = null;
  boundPointerMql = null;
}

/**
 * 输入轴 composable：hover / pointer 媒体查询 → 触控轴 / 指针轴。
 *
 * 消费即登记共享订阅（模块级两路监听，引用计数），作用域销毁时注销；
 * 最后一个消费作用域销毁后监听全部解绑。返回值是本消费方自己的 computed，
 * 随共享信号 ref 实时换轴。
 *
 * 前置条件：须在活动 effect scope（组件 setup / effectScope.run）内消费——
 * 注销登记在 scope 上，scope 外调用会泄漏一个永远不释放的计数。
 */
export function useInputMode(): ComputedRef<InputMode> {
  consumerCount += 1;
  bindSharedListeners();
  onScopeDispose(releaseSharedListeners);
  return computed<InputMode>(() => (canHover.value && hasFinePointer.value ? "pointer" : "touch"));
}
