import { onUnmounted, watch } from "vue";
import { hasOpenOverlay } from "@ledger/ui-kit/overlayRegistry";
import { useInputMode } from "@/composables/useInputMode";
import { isEditableTarget } from "@/composables/useCreateShortcuts";

/**
 * 裸键快捷键共用内核（issue #1904）：翻页快捷键（ADR-0140）与周期步进快捷键
 * （ADR-0141）同族同形，只有「键位 ⇄ 语义」的纯映射不同——内核独占两者共有的
 * 抑制闸门与监听面装配，键位闭集与语义留在各自模块（单一事实源不合并）。
 *
 * 抑制三闸门（Overlay Suppression ADR-0035 / 输入轴 ADR-0088 / CreateShortcut 家族
 * 纪律）：焦点在可编辑元素或任一弹层打开时不触发；触控轴不绑监听（含运行中换轴
 * 实时拆装——输入轴信号变化即重接线）；卸载时清理。
 *
 * match 是纯函数（KeyboardEvent → 命中语义 | null），与轴无关、可独立测试；
 * 命中即 preventDefault 后 dispatch——「命中但语义不可用」的放行要求（记一笔
 * 可用性闸门，ADR-0135 决策 5）需 preventDefault 前判定，因而不走本内核
 * （useCreateShortcuts 自持闸门）。
 */
export function useBareKeyShortcuts<T>(
  match: (e: KeyboardEvent) => T | null,
  dispatch: (hit: T) => void,
): void {
  const onKeydown = (e: KeyboardEvent) => {
    const hit = match(e);
    if (hit === null) return;
    if (isEditableTarget(e) || hasOpenOverlay()) return;
    e.preventDefault();
    dispatch(hit);
  };
  const inputMode = useInputMode();
  watch(
    inputMode,
    (mode) => {
      if (mode === "pointer") window.addEventListener("keydown", onKeydown);
      else window.removeEventListener("keydown", onKeydown);
    },
    { immediate: true },
  );
  onUnmounted(() => window.removeEventListener("keydown", onKeydown));
}
