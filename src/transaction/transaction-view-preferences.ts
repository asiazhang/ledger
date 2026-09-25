import { computed, ref } from "vue";
import { defineStore } from "pinia";
import {
  clearHideInvestmentRelated,
  getSavedHideInvestmentRelated,
  saveHideInvestmentRelated,
} from "@ledger/utils/view-state";

/**
 * 交易页视图偏好（issue #1811 / ADR-0136 决策 1）：设备级「默认行集裁决」的具名
 * Pinia store——该偏好的唯一读写方（持久界面状态单一归宿纪律，仿功能开关 store
 * 先例 ADR-0116）。与交易页会话筛选分家：偏好是持久裁决不是 TransactionFilter
 * 维度，不入会话筛选、不参与清除筛选与 ESC 复位（ADR-0094 边界原样）；承载在
 * localStorage（ViewState 家族，`view_state:` 前缀），不入账本 SQLite、不进备份、
 * 不参与多端同步；默认关。
 */

/**
 * 「隐藏投资相关流水」布尔解析（纯函数，比照 parseClosedFeatures 口径）：
 * 仅布尔 true 为开——字符串 / 数字 / 对象等脏值（手工改写 localStorage、旧版本
 * 形态漂移）整体回默认关，不产生非法状态。
 */
export function parseHideInvestmentRelated(raw: unknown): boolean {
  return raw === true;
}

/**
 * 视图偏好 store：首实例是交易页「隐藏投资相关流水」开关（ADR-0136）。已消费方：
 * TransactionsView 主列表请求装配（偏好先决收窄 ∩ 既有筛选）与筛选栏开关。
 */
export const useTransactionViewPreferencesStore = defineStore(
  "transaction-view-preferences",
  () => {
    // 启动读路径：原始值经解析防御——脏值整体回默认关（issue #1811）。
    const hideInvestmentRelated = ref(parseHideInvestmentRelated(getSavedHideInvestmentRelated()));

    /**
     * 写路径（点选即写）：开启持久化、关闭删记录（默认态 = 无记录，ADR-0116
     * 家族同构）；同值守卫——目标态与当前态相同时不动作、不产生存储写入。
     */
    function setHideInvestmentRelated(value: boolean) {
      if (value === hideInvestmentRelated.value) return;
      hideInvestmentRelated.value = value;
      if (value) saveHideInvestmentRelated(true);
      else clearHideInvestmentRelated();
    }

    return {
      hideInvestmentRelated: computed(() => hideInvestmentRelated.value),
      setHideInvestmentRelated,
    };
  },
);
