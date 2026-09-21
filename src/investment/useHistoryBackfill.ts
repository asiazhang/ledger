import { ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import type { InstrumentSyncProgress } from "@ledger/types";

/**
 * 价格历史后台补全接缝（issue #1375 / ADR-0122）：订阅后端后台补全任务的
 * 静默标的级完成计数事件，供投资页渲染只读计数。
 *
 * 与手动同步接缝（useInstrumentInfoSync）的分界：本接缝**只读**——后台任务
 * 由壳层启动接线自拉，前端没有触发入口（不给按钮、不可点、不占全局忙碌条、
 * 不改变任何页面可用性）；事件走独立事件名（非手动同步的进度事件），手动
 * 同步的进度条不被后台推进点亮，反之亦然。基金首刷深回填期间事件另带页级
 * 明细（issue #1061 形状随迁）。
 *
 * 终态静默收起：`done ≥ total` 即本轮队列排空，计数收起（progress 归空）。
 * 后台任务无显式「开始/结束」承诺，因此不做在途门控——事件只在轮次进行中
 * 到达，下一轮从 `{ done: 0, total }` 重新点亮是正确行为（新一天的新队列）。
 *
 * **全部接缝状态为模块级共享单例**（先例：useInstrumentInfoSync）：后台补全
 * 只有一场在跑，投资页各处渲染同一份计数。
 */

/** 价格历史后台补全进度事件名（后端常量单点在行情同步域 progress 模块；
 *  与手动同步事件同 payload 形状、不同事件名）。 */
export const HISTORY_BACKFILL_PROGRESS_EVENT = "ledger:history-backfill-progress";

/** 静默完成计数：null = 无在途轮次（队列空或终态已收起）。 */
const progress = ref<InstrumentSyncProgress | null>(null);

/** 订阅登记：模块生命周期内只订阅一次（测试经 reset 重置后随新 mock 重订）。 */
let subscribed = false;

function ensureProgressSubscription(): void {
  if (subscribed) return;
  subscribed = true;
  // 模块级订阅与应用同生命周期：不随组件卸载注销（先例：useInstrumentInfoSync）；
  // 注册失败静默（本地事件，极少发生）。
  void listen<InstrumentSyncProgress>(HISTORY_BACKFILL_PROGRESS_EVENT, (event) => {
    // 载荷形状异常（脏数据/NaN）一并忽略；形状校验口径与手动同步接缝一致。
    const payload = event.payload as Partial<InstrumentSyncProgress> | undefined;
    if (
      !payload ||
      typeof payload.done !== "number" ||
      Number.isFinite(payload.done) === false ||
      typeof payload.total !== "number" ||
      Number.isFinite(payload.total) === false
    ) {
      return;
    }
    // 终态静默收起：本轮队列排空即收起计数（不残留 228/228 的完成态）。
    progress.value =
      payload.done >= payload.total ? null : { done: payload.done, total: payload.total };
  }).catch((e) => {
    console.warn(`订阅 ${HISTORY_BACKFILL_PROGRESS_EVENT} 失败`, e);
  });
}

/**
 * 测试隔离用：清空接缝状态、撤销订阅登记（先例：resetInstrumentInfoSyncForTest）。
 * 生产代码不得调用。
 */
export function resetHistoryBackfillForTest(): void {
  progress.value = null;
  subscribed = false;
}

/**
 * 价格历史后台补全接缝（投资页消费）：返回静默完成计数的只读投影。
 */
export function useHistoryBackfill() {
  ensureProgressSubscription();
  return { progress };
}
