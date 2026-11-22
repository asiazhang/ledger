import { defineStore } from "pinia";
import { ref } from "vue";
import { api } from "@ledger/api";
import { createPushFirstList } from "@/composables/push-first-list";
import type {
  PhysicalAsset,
  PhysicalAssetDisposeInput,
  PhysicalAssetInput,
  PhysicalAssetUpdateInput,
  PhysicalAssetValuationInput,
} from "@ledger/types";

export const usePhysicalAssetsStore = defineStore("physicalAssets", () => {
  const assets = ref<PhysicalAsset[]>([]);
  const holdingTotalNativeCents = ref(0);
  const nativeCurrency = ref("");
  const statusFilter = ref<"holding" | "disposed">("holding");

  const { status, version, refresh, invalidate } = createPushFirstList(
    () => api.listPhysicalAssets(statusFilter.value),
    (list) => {
      assets.value = list.assets;
      holdingTotalNativeCents.value = list.holding_total_native_cents;
      nativeCurrency.value = list.native_currency;
    },
  );

  async function setStatusFilter(filter: "holding" | "disposed"): Promise<void> {
    if (statusFilter.value === filter) return;
    statusFilter.value = filter;
    invalidate();
    await refresh();
  }

  async function create(input: PhysicalAssetInput): Promise<string> {
    const id = await api.createPhysicalAsset(input);
    await refresh().catch(() => {});
    return id;
  }

  async function update(id: string, input: PhysicalAssetUpdateInput): Promise<void> {
    await api.updatePhysicalAsset(id, input);
    await refresh().catch(() => {});
  }

  async function updateValuation(id: string, input: PhysicalAssetValuationInput): Promise<void> {
    await api.updatePhysicalAssetValuation(id, input);
    await refresh().catch(() => {});
  }

  async function dispose(id: string, input: PhysicalAssetDisposeInput): Promise<void> {
    await api.disposePhysicalAsset(id, input);
    await refresh().catch(() => {});
  }

  async function remove(id: string): Promise<void> {
    await api.deletePhysicalAsset(id);
    await refresh().catch(() => {});
  }

  return {
    assets,
    holdingTotalNativeCents,
    nativeCurrency,
    status,
    version,
    statusFilter,
    refresh,
    setStatusFilter,
    create,
    update,
    updateValuation,
    dispose,
    remove,
  };
});
