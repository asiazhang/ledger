// 交易表格级行为委托（issue #2010 验收③：账户/商户/来源下钻与备注复制）：
// 桌面表格单元格不再逐格挂链接组件（AccountLink / MerchantLink / SourceLink /
// NoteCopyButton），平替元素只带 `data-action` 载荷（transaction-columns 装配）；
// 本模块是**表格级单一 click 监听器**的分发端——视图以 useTransactionTableActions()
// 取处理器，绑在包裹 NDataTable 的壳层 div 上，一次点击冒泡分发：
// - account / merchant：跳交易页对应维度的下钻过滤视图（与链接组件同载荷）；
// - source：经来源跳转深模块 resolveSourceJumpTarget 计算路由目标（收纳/关闭两轴
//   谓词仍由 store 注入，深模块纯函数纪律不变）；
// - copy-note：clipboard 显式复制通道 + 成功/失败 toast（NoteCopyButton 语义迁移，
//   i18n 键不变；组件随 #2010 退役，本模块是其唯一后继）。
// 键盘可达性：平替元素是真 <button>，Tab 聚焦 + Enter 触发的 click 同样冒泡到本分发。

import { useMessage } from "naive-ui";
import { useRouter } from "vue-router";
import { t } from "@ledger/i18n";
import { errorMessage } from "@ledger/utils/errors";
import { useSidebarOrderStore } from "@/stores/sidebar-order";
import { useFeatureToggleStore } from "@/settings/feature-toggles";
import { resolveSourceJumpTarget, type TransactionSourceKind } from "@/components/source-jump";

/** 表格级行为委托处理器：绑在桌面表格壳层 div 的 click 监听器（单实例）。 */
export type TransactionTableActionHandler = (event: MouseEvent) => void;

/** 委托动作闭集（装配端 transaction-columns 的 data-action 载荷值与分发端 switch
 * 的共享绑定——两端任一侧拼写漂移即类型不匹配，不靠裸字符串对齐）。 */
export const CELL_ACTIONS = {
  account: "account",
  merchant: "merchant",
  source: "source",
  copyNote: "copy-note",
} as const;
export type CellAction = (typeof CELL_ACTIONS)[keyof typeof CELL_ACTIONS];

/**
 * 装配表格级行为委托（视图 setup 期调用一次；router / 消息 / 两轴谓词 store 就地解析）。
 * 分发闭集见文件头注；非委托目标（普通单元格/箭头分隔等）的点击静默无操作。
 */
export function useTransactionTableActions(): TransactionTableActionHandler {
  const router = useRouter();
  const sidebarOrder = useSidebarOrderStore();
  const featureToggles = useFeatureToggleStore();
  const message = useMessage();

  async function copyNote(note: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(note);
      message.success(t("transactions.list.copyNoteOk"));
    } catch (e) {
      message.error(t("transactions.list.copyNoteFailed", { msg: errorMessage(e) }));
    }
  }

  return (event: MouseEvent) => {
    const target = event.target as Element | null;
    // 最近委托祖先（嵌套元素命中内层唯一载荷；无命中即非交互区，无操作）
    const hit = target?.closest?.("[data-action]");
    if (!(hit instanceof HTMLElement)) return;
    switch (hit.dataset.action) {
      case CELL_ACTIONS.account:
        router.push({ name: "transactions", query: { account: hit.dataset.id! } });
        break;
      case CELL_ACTIONS.merchant:
        router.push({ name: "transactions", query: { merchant: hit.dataset.id! } });
        break;
      case CELL_ACTIONS.source:
        router.push(
          resolveSourceJumpTarget(
            hit.dataset.kind as TransactionSourceKind,
            hit.dataset.entity!,
            (v) => sidebarOrder.isViewContained(v),
            (v) => featureToggles.isFeatureClosed(v),
          ),
        );
        break;
      case CELL_ACTIONS.copyNote:
        void copyNote(hit.dataset.note ?? "");
        break;
      default:
        break;
    }
  };
}
