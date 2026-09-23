import { computed, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import { useLoadable } from "@ledger/loadable";
import type { ExchangeRateSyncReport, FxSyncProgress, FxSyncStage } from "@ledger/types";

/**
 * 汇率手动同步（issue #1762，设置页「同步汇率」入口的跨页共享任务态）。
 *
 * 形态仿标的信息同步的既有先例（`useInstrumentInfoSync`，模块级单例 ref，切页
 * 不丢状态）：同步只有一场，进行中/阶段文字/结果报告就是同一份——按钮的
 * loading/disabled 绑定单例状态，切走再切回仍是禁用态、阶段文字继续跟随实际
 * 进度、完成后结果报告照常显示。不新增 Pinia store（临时任务态不符合参考数据/
 * 设备偏好 store 的定位），不用 keep-alive（已被 ADR-0061 否决的备选）。
 *
 * Loadable 接缝不变：发起动作仍经 Loadable（错误捕获、toast 归一、竞态裁决照旧）——
 * 单例持有的是跨实例共享的那一个 Loadable 实例，其 loading 即按钮禁用态的唯一
 * 来源；阶段文字另由后端阶段事件驱动（`FX_SYNC_PROGRESS_EVENT`，payload 为阶段
 * 闭集 + 可选已解析天数），与 Loadable 的 loading 正交并存。
 *
 * 在途语义分两层：前端不做短路复用——同步进行中再次触发会真实打到后端，由后端
 * 编排层在途互斥立即报 `fx.sync-in-progress` 码化错误（「已有汇率同步在进行」，
 * 经 Loadable 就地错误位 + toast 呈现）；按钮禁用态保证同页内不会误触，撞车提示
 * 面向的是每日自动同步在途时的手动触发等多入口并发。
 *
 * 阶段文字为真实文本内容（屏幕阅读器可朗读）；GlobalBusyBar 继续按既有机制自动
 * 覆盖本次 invoke，与阶段文字并存、互不替代。
 */

/** 汇率同步阶段事件名（issue #1762；带 payload，与无 payload 的失效信号族同处
 * `ledger:*` 命名空间；后端常量单点在 market-sync 域 progress 模块）。
 * 载荷形状见 `types/fx.ts` 的 FxSyncProgress。 */
export const FX_SYNC_PROGRESS_EVENT = "ledger:fx-sync-progress";

/** 最近一次同步报告（含覆盖区间 / 条数，成功终态的呈现面）。 */
const report = ref<ExchangeRateSyncReport | null>(null);
/** 当前阶段：null = 空闲（未开始或已终态，阶段文字收起、报告/错误接棒）。 */
const stage = ref<FxSyncStage | null>(null);
/** 读取完成时上报的本次共解析天数（随阶段同生命周期）。 */
const parsedDays = ref<number | null>(null);

/** 跨实例共享的唯一 Loadable 实例：发起动作、错误捕获与 toast 归一收口于此。 */
const syncLoad = useLoadable(async () => {
  const result = await api.syncExchangeRates();
  report.value = result;
  return result;
});

/** 在途同步计数：阶段事件只在该计数非零时被接受（终态后迟到事件不复活阶段文字）。 */
let inFlightSyncs = 0;

/** 订阅登记：模块生命周期内只订阅一次（测试经 reset 重置后随新 mock 重订）。 */
let subscribed = false;

function ensureProgressSubscription(): void {
  if (subscribed) return;
  subscribed = true;
  // 模块级订阅与应用同生命周期：不随组件卸载注销（阶段状态是模块单例，
  // 先例：globalBusy 聚合计数）；注册失败静默（本地事件，极少发生）。
  void listen<FxSyncProgress>(FX_SYNC_PROGRESS_EVENT, (event) => {
    // 守卫：阶段事件只在同步在途时有意义——终态后迟到的事件不得让阶段文字复活；
    // 阶段闭集外取值与缺天数的 persisting 一并忽略（防脏 payload 渲染）。
    const payload = event.payload as Partial<FxSyncProgress> | undefined;
    if (inFlightSyncs <= 0 || !payload) return;
    if (payload.stage === "fetching") {
      stage.value = "fetching";
      return;
    }
    if (
      payload.stage === "persisting" &&
      typeof payload.days_parsed === "number" &&
      Number.isFinite(payload.days_parsed)
    ) {
      stage.value = "persisting";
      parsedDays.value = payload.days_parsed;
    }
  }).catch((e) => {
    console.warn(`订阅 ${FX_SYNC_PROGRESS_EVENT} 失败`, e);
  });
}

/**
 * 测试隔离用：清空全部接缝状态与在途计数、撤销订阅登记
 *（先例：resetInstrumentInfoSyncForTest）。生产代码不得调用。
 */
export function resetFxSyncForTest(): void {
  report.value = null;
  stage.value = null;
  parsedDays.value = null;
  syncLoad.invalidate();
  syncLoad.error.value = null;
  inFlightSyncs = 0;
  subscribed = false;
}

/**
 * 汇率手动同步接缝（设置页共用；状态为模块级单例，切页不丢——模拟组件卸载重挂，
 * syncing/阶段文字/报告恢复，行为一致性由同一接缝同一份状态保证）。
 */
export function useFxSync() {
  ensureProgressSubscription();

  /** 阶段文字：按真实阶段流转（正在读取 → 携带天数的正在写入），终态收起。 */
  const stageText = computed(() => {
    if (stage.value === "fetching") return t("settings.fxSync.stageFetching");
    if (stage.value === "persisting" && parsedDays.value !== null) {
      return t("settings.fxSync.stagePersisting", { days: parsedDays.value });
    }
    return "";
  });

  /**
   * 触发一次同步：新一次开始即清空旧阶段与旧报告（与结果消息同生命周期）；
   * 切页重挂不调 sync，在途/终态自然保留。成功回报告、失败回空且错误经
   * Loadable 置位（含后端在途互斥的「已有汇率同步在进行」）。
   */
  async function sync(): Promise<ExchangeRateSyncReport | null> {
    stage.value = null;
    parsedDays.value = null;
    report.value = null;
    inFlightSyncs += 1;
    try {
      return await syncLoad.run();
    } finally {
      inFlightSyncs -= 1;
      // 终态收起清空：阶段文字收起、报告/错误接棒（成功/失败同路）。
      stage.value = null;
      parsedDays.value = null;
    }
  }

  return {
    /** 同步进行中：按钮 loading/disabled 的唯一来源（跨页共享）。 */
    syncing: syncLoad.loading,
    /** 就地错误位（按码本地化，含撞车提示；toast 走 Loadable 默认策略）。 */
    syncError: syncLoad.error,
    report,
    stage,
    parsedDays,
    stageText,
    sync,
  };
}
