import { defineStore } from "pinia";
import { ref } from "vue";
import { api } from "@ledger/api";
import { createPushFirstList } from "@/composables/push-first-list";
import type { ItemDisposeInput, ItemInput, ItemWithDailyCost } from "@ledger/types";

/**
 * 物品（Item）领域 store（issue #116）。
 *
 * 物品是独立领域实体（CONTEXT.md `Item` / ADR-0014），**不进** `useReferenceStore`
 * （那不是"可选值字典"），拥有自己的单一来源 store。
 *
 * 清单生命周期（self-init / `ledger:changed` 失效重拉 / stale-while-revalidate
 * 整体替换 / 在途合并 / status / version）内化在 push-first 工厂单点
 * （`createPushFirstList`，ADR-0123）；本店只留快照落位与领域动作。
 *
 * 每天成本随日历天数实时变化（无事件），列表展示时刻快照由后端计算；
 * 事件驱动的重拉保证写入后即刻可见。
 */
export const useItemsStore = defineStore("items", () => {
  const items = ref<ItemWithDailyCost[]>([]);

  const { status, version, refresh } = createPushFirstList(
    () => api.listItems(),
    (list) => {
      items.value = list;
    },
  );

  /** 创建物品：成功后立即重拉（后端同时发 ledger:changed，事件侧重拉被在途合并）；
   *  重拉失败不反转写动作成败（已落库，失败信号由 status 承载，ADR-0123 决策 3）。 */
  async function create(input: ItemInput): Promise<string> {
    const id = await api.createItem(input);
    await refresh().catch(() => {
      /* 重拉失败不阻断建档成功路径 */
    });
    return id;
  }

  /** 按 id 修改物品（名称/购买日期/总成本/备注）：成功后立即重拉（同 create）。 */
  async function update(id: string, input: ItemInput): Promise<void> {
    await api.updateItem(id, input);
    await refresh().catch(() => {
      /* 重拉失败不阻断编辑成功路径 */
    });
  }

  /** 处置物品（issue #120）：置 disposed 并记录处置日期（必填）与可选残值；
   *  对已处置物品再次处置 = 修正处置信息。成功后立即重拉（同 create）。 */
  async function dispose(id: string, input: ItemDisposeInput): Promise<void> {
    await api.disposeItem(id, input);
    await refresh().catch(() => {
      /* 重拉失败不阻断处置成功路径 */
    });
  }

  /** 软删除物品（后端打 is_deleted=1，不物理移除）：成功后立即重拉（同上）。 */
  async function remove(id: string): Promise<void> {
    await api.deleteItem(id);
    await refresh().catch(() => {
      /* 重拉失败不阻断删除成功路径 */
    });
  }

  return {
    items,
    status,
    version,
    refresh,
    create,
    update,
    dispose,
    remove,
  };
});
