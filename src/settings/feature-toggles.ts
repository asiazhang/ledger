import { computed, ref } from "vue";
import { defineStore } from "pinia";
import {
  clearClosedFeatures,
  getSavedClosedFeatures,
  saveClosedFeatures,
} from "@ledger/utils/view-state";

export const CLOSABLE_FEATURES = [
  "budget",
  "reports",
  "scheduled",
  "merchants",
  "investments",
  "items",
  "policies",
  "physicalAssets",
  "insurers",
] as const;

export const NON_CLOSABLE_FEATURES = [
  "dashboard",
  "transactions",
  "accounts",
  "search",
  "ai",
  "settings",
] as const;

export type ClosableFeatureId = (typeof CLOSABLE_FEATURES)[number];

export function isClosableFeature(v: unknown): v is ClosableFeatureId {
  return typeof v === "string" && (CLOSABLE_FEATURES as readonly string[]).includes(v);
}

export function parseClosedFeatures(raw: unknown): ClosableFeatureId[] {
  if (!Array.isArray(raw)) return [];
  const present = new Set<string>();
  for (const item of raw) {
    if (isClosableFeature(item)) present.add(item);
  }
  return CLOSABLE_FEATURES.filter((id) => present.has(id));
}

export const useFeatureToggleStore = defineStore("feature-toggles", () => {
  const closed = ref<ClosableFeatureId[]>(parseClosedFeatures(getSavedClosedFeatures()));

  const closedFeatures = computed<readonly ClosableFeatureId[]>(() => closed.value);

  function isFeatureClosed(id: unknown): boolean {
    return typeof id === "string" && (closed.value as readonly string[]).includes(id);
  }

  function setFeatureClosed(id: unknown, shouldClose: boolean) {
    if (!isClosableFeature(id)) return;
    if (shouldClose === closed.value.includes(id)) return;
    const present = new Set<ClosableFeatureId>(closed.value);
    if (shouldClose) present.add(id);
    else present.delete(id);
    const next = CLOSABLE_FEATURES.filter((f) => present.has(f));
    closed.value = next;
    if (next.length === 0) clearClosedFeatures();
    else saveClosedFeatures(next);
  }

  return {
    closedFeatures,
    isFeatureClosed,
    setFeatureClosed,
  };
});
