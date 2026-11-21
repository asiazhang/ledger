import { useRouter } from "vue-router";
import { useSidebarOrderStore } from "@/stores/sidebar-order";
import { useFeatureToggleStore } from "@/settings/feature-toggles";
import { resolveSourceJumpTarget, type TransactionSourceKind } from "@/components/source-jump";
import { copyNoteWithToast } from "@ledger/ui-kit/note-copy";
import type { MessageApi } from "naive-ui";

/**
 * 交易表格单元格动作的表级事件委托（issue #2189 行内高频组件瘦身）：
 * 单元格内的可交互元素（账户/商户/来源轻量链接、备注复制轻量按钮、操作列「⋯」）
 * 不再逐格绑定组件回调与业务逻辑，只携带语义数据属性（`data-tx-action` 声明
 * 动作闭集 + 载荷属性）；点击事件冒泡到表格容器统一分发——路由导航、来源跳转
 * 深模块调用、剪贴板写入与行菜单打开全部收口到本模块，行内元素零业务逻辑。
 *
 * 动作闭集五（与列渲染函数的 data 属性逐键对齐）：
 * - `account-drill`（载荷 data-tx-account）：跳转该账户过滤视图（黑洞/隐藏账户
 *   不携带本动作，渲染占位文本）；
 * - `merchant-drill`（载荷 data-tx-merchant）：跳转该商户过滤视图（未知商户
 *   不携带本动作）；
 * - `source-jump`（载荷 data-tx-source-kind / data-tx-source-entity）：经来源
 *   跳转深模块 resolveSourceJumpTarget 解析路由目标（侧栏收纳与功能开关谓词
 *   在本模块注入）；软删保单不携带本动作（不提供落空的跳转）；
 * - `copy-note`（载荷 data-tx-note）：剪贴板写入 + 统一 toast（@ledger/ui-kit
 *   note-copy 单一出口）；
 * - `row-menu`（载荷 data-tx-row-key = 显示行键）：与行右键共用 RowContextMenu
 *   open 入口；行对象由调用方按键解析（displayRowKey 唯一）。
 *
 * 键盘可达性由原生 <button> 承担：Tab 聚焦 + Enter 触发 click，click 冒泡到
 * 容器即走同一分发，无需额外键盘分支。
 */

/** 动作数据属性键（列渲染与分发的唯一对齐面）。 */
export const CELL_ACTION_ATTR = "data-tx-action";

/** 表格单元格动作处理闭集（视图装配：路由与消息实例由调用环境提供）。 */
export interface TableCellActionHandlers {
  accountDrill(accountId: string): void;
  merchantDrill(merchantId: string): void;
  sourceJump(kind: TransactionSourceKind, entityId: string): void;
  copyNote(note: string): void;
  rowMenu?(event: MouseEvent, rowKey: string): void;
}

/**
 * 统一分发：从事件目标向上找最近的动作元素，按动作闭集分派；无动作元素
 * （普通单元格/空白）静默返回。事件对象原样透传（行菜单打开需要点击坐标）。
 */
export function dispatchTableCellAction(
  event: MouseEvent,
  handlers: TableCellActionHandlers,
): void {
  const target = event.target as HTMLElement | null;
  const el = target?.closest?.(`[${CELL_ACTION_ATTR}]`) as HTMLElement | null;
  if (!el) return;
  const dataset = el.dataset;
  switch (dataset.txAction) {
    case "account-drill":
      if (dataset.txAccount) handlers.accountDrill(dataset.txAccount);
      return;
    case "merchant-drill":
      if (dataset.txMerchant) handlers.merchantDrill(dataset.txMerchant);
      return;
    case "source-jump":
      if (dataset.txSourceKind && dataset.txSourceEntity) {
        handlers.sourceJump(dataset.txSourceKind as TransactionSourceKind, dataset.txSourceEntity);
      }
      return;
    case "copy-note":
      handlers.copyNote(dataset.txNote ?? "");
      return;
    case "row-menu":
      if (handlers.rowMenu && dataset.txRowKey) handlers.rowMenu(event, dataset.txRowKey);
      return;
    default:
      return;
  }
}

/**
 * 交易列表/搜索结果共用的表格动作装配（视图 setup 期消费）：账户/商户下钻与
 * 来源跳转的路由目标、来源跳转的收纳与功能开关谓词、备注复制的剪贴板+toast
 * 全部内化；调用方只装配行菜单解析（交易列表）与消息实例（搜索页无行菜单）。
 * 返回值即表格容器的 @click 处理器。
 */
export function useTransactionTableCellActions(options: {
  /** 消息实例（备注复制 toast；视图 setup 期 useMessage()） */
  message: MessageApi;
  /** 行菜单打开（交易列表传入：解析显示行键 → RowContextMenu.open；搜索结果缺省） */
  onRowMenu?: (event: MouseEvent, rowKey: string) => void;
}): (event: MouseEvent) => void {
  const router = useRouter();
  const sidebarOrder = useSidebarOrderStore();
  const featureToggles = useFeatureToggleStore();
  return (event: MouseEvent) =>
    dispatchTableCellAction(event, {
      accountDrill: (accountId) => {
        void router.push({ name: "transactions", query: { account: accountId } });
      },
      merchantDrill: (merchantId) => {
        void router.push({ name: "transactions", query: { merchant: merchantId } });
      },
      sourceJump: (kind, entityId) => {
        // 来源跳转深模块（spec #704）：落点分流与 focus 装配收口在深模块，
        // 本处只注入侧栏收纳与功能开关两轴谓词（SourceLink 组件同款约定）。
        void router.push(
          resolveSourceJumpTarget(
            kind,
            entityId,
            (v) => sidebarOrder.isViewContained(v),
            (v) => featureToggles.isFeatureClosed(v),
          ),
        );
      },
      copyNote: (note) => {
        void copyNoteWithToast(options.message, note);
      },
      rowMenu: options.onRowMenu,
    });
}
