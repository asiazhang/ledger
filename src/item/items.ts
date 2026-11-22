import { defineStore } from "pinia";
import { ref } from "vue";
import { api } from "@ledger/api";
import { createPushFirstList } from "@/composables/push-first-list";
import type { ItemDisposeInput, ItemInput, ItemWithDailyCost } from "@ledger/types";

export const useItemsStore = defineStore("items", () => {
  const items = ref<ItemWithDailyCost[]>([]);

  const { status, version, refresh } = createPushFirstList(
    () => api.listItems(),
    (list) => {
      items.value = list;
    },
  );

  async function create(input: ItemInput): Promise<string> {
    const id = await api.createItem(input);
    await refresh().catch(() => {});
    return id;
  }

  async function update(id: string, input: ItemInput): Promise<void> {
    await api.updateItem(id, input);
    await refresh().catch(() => {});
  }

  async function dispose(id: string, input: ItemDisposeInput): Promise<void> {
    await api.disposeItem(id, input);
    await refresh().catch(() => {});
  }

  async function remove(id: string): Promise<void> {
    await api.deleteItem(id);
    await refresh().catch(() => {});
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
