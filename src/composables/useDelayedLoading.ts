import { ref, watch, type Ref } from "vue";
import { onScopeDispose } from "vue";

/**
 * 延迟出场的加载指示（issue #2010 翻页体验收尾）：把「原始 loading」整流成
 * 「可见 loading」——出场延迟（showAfter）+ 最小驻留（minShow）。
 *
 * 动机：翻页/筛选请求本地往返极快（IPC → SQLite 仅数毫秒，#2010 诊断）时，
 * 原始 loading 在一两帧内翻转，NDataTable / NSpin 的 loading 指示器（带
 * fade-in-scale-up 过场）被拦断成「半透明放大→立即淡出」的鬼影闪屏——#2010
 * 渲染提速前慢渲染把指示器垫成正常加载态，提速后成为唯一的视觉噪声。
 * AGENTS.md 体验标尺（「用户使用时基本感受不到迟滞感」）：快加载**不显示**
 * 指示器（内容直接换，零闪烁），慢加载延迟显示且驻留至过场完整淡出。
 *
 * 纯时序整流，不持业务语义：请求发起与数据落位归调用方；对调用方而言这就是
 * 一个「可以直接绑给指示器的 loading」。定时器随作用域销毁注销。
 */

/** 整流选项（缺省值见实现；时长为零可退化为直通）。 */
export interface DelayedLoadingOptions {
  /** 出场延迟：原始 loading 持续超过该时长才显示指示器（ms）。 */
  showAfterMs?: number;
  /** 最小驻留：指示器一旦显示，至少保持该时长再隐去（过场完整淡出，ms）。 */
  minShowMs?: number;
}

const DEFAULT_SHOW_AFTER_MS = 120;
const DEFAULT_MIN_SHOW_MS = 250;

/**
 * 整流原始 loading 为可见 loading。
 *
 * - 上升沿：不立即显示，`showAfterMs` 后仍在加载才显示（快加载全程不可见）；
 * - 下降沿：未显示过则取消出场定时器（零痕迹）；已显示则补足 `minShowMs` 再隐去；
 * - 连续加载（间隔极短的串行请求）按各自边沿独立整流，不跨请求记账。
 */
export function useDelayedLoading(
  loading: Ref<boolean>,
  options: DelayedLoadingOptions = {},
): Ref<boolean> {
  const { showAfterMs = DEFAULT_SHOW_AFTER_MS, minShowMs = DEFAULT_MIN_SHOW_MS } = options;
  const visible = ref(false);
  let showTimer: ReturnType<typeof setTimeout> | undefined;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  /** 本轮指示器登场时刻（minShow 的计时锚）。 */
  let shownAt = 0;

  function clearTimers(): void {
    clearTimeout(showTimer);
    clearTimeout(hideTimer);
    showTimer = undefined;
    hideTimer = undefined;
  }

  const stop = watch(
    loading,
    (value) => {
      if (value) {
        // 上升沿：showAfter 后仍在加载才登场；登场即记锚（minShow 驻留）
        clearTimeout(hideTimer);
        showTimer = setTimeout(() => {
          shownAt = performance.now();
          visible.value = true;
        }, showAfterMs);
      } else {
        // 下降沿：未登场则零痕迹退场；已登场则补足最小驻留
        clearTimeout(showTimer);
        if (!visible.value) return;
        const elapsed = performance.now() - shownAt;
        const remaining = Math.max(0, minShowMs - elapsed);
        hideTimer = setTimeout(() => {
          visible.value = false;
        }, remaining);
      }
    },
    { immediate: true },
  );

  onScopeDispose(() => {
    stop();
    clearTimers();
  });

  return visible;
}
