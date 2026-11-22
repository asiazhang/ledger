<script setup lang="ts">
import { computed, type Component } from "vue";
import { useRouter } from "vue-router";
import { NIcon, NTag } from "naive-ui";
import {
  CalendarClearOutline,
  CubeOutline,
  RepeatOutline,
  ShieldCheckmarkOutline,
  SwapHorizontalOutline,
  TrendingUpOutline,
} from "@vicons/ionicons5";
import { resolveSourceJumpTarget, type TransactionSourceKind } from "@/components/source-jump";
import { useAppStore } from "@/stores/app";
import { useSidebarOrderStore } from "@/stores/sidebar-order";
import { useFeatureToggleStore } from "@/settings/feature-toggles";
import { useInputMode } from "@/composables/useInputMode";
import { accentColor } from "@ledger/theme/overrides";
import { t } from "@ledger/i18n";
import type { TransactionSource } from "@ledger/types";

const KIND_ICONS = {
  installmentPlan: CalendarClearOutline,
  subscription: RepeatOutline,
  scheduledTransfer: SwapHorizontalOutline,
  policy: ShieldCheckmarkOutline,
  item: CubeOutline,
  instrument: TrendingUpOutline,
} satisfies Record<TransactionSourceKind, Component>;

const props = defineProps<{
  source: TransactionSource;
}>();

const router = useRouter();
const app = useAppStore();
const sidebarOrder = useSidebarOrderStore();
const featureToggles = useFeatureToggleStore();

const inputMode = useInputMode();
const isTouch = computed(() => inputMode.value === "touch");

const statusLabel = computed(() =>
  props.source.status ? t(`transactions.source.status.${props.source.status}`) : null,
);

const displayName = computed(
  () => props.source.display_name || t(`transactions.source.kind.${props.source.kind}`),
);

const clickable = computed(() => props.source.status !== "deleted");

const accent = computed(() => accentColor(app.theme));

function go() {
  if (!clickable.value) return;
  router.push(
    resolveSourceJumpTarget(
      props.source.kind,
      props.source.entity_id,
      (v) => sidebarOrder.isViewContained(v),
      (v) => featureToggles.isFeatureClosed(v),
    ),
  );
}
</script>

<template>
  <span class="source-cell" :title="t(`transactions.source.kind.${source.kind}`)">
    <NIcon :component="KIND_ICONS[source.kind]" class="source-cell-icon" />
    <button
      v-if="clickable"
      type="button"
      class="source-link"
      :style="{ color: accent.base, '--accent-hover': accent.hover }"
      @click="go"
    >
      {{ displayName }}
    </button>
    <span v-else class="source-name">{{ displayName }}</span>
    <span v-if="isTouch && source.display_name" class="source-type-label">{{
      t(`transactions.source.kind.${source.kind}`)
    }}</span>
    <NTag v-if="statusLabel" size="small" :bordered="false">{{ statusLabel }}</NTag>
  </span>
</template>

<style scoped>
.source-cell {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  max-width: 100%;
  color: inherit;
  opacity: 0.92;
}
.source-cell-icon {
  flex: none;
  opacity: 0.65;
}
.source-link {
  border: none;
  padding: 0;
  background: none;
  font: inherit;
  cursor: pointer;
  border-radius: 4px;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.source-link:hover {
  color: var(--accent-hover);
  background: rgba(255, 255, 255, 0.06);
  text-decoration: underline;
}
.source-link:focus-visible {
  outline: 2px solid var(--accent-hover);
  outline-offset: 2px;
}
.source-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.source-type-label {
  flex: none;
  font-size: 11px;
  opacity: 0.55;
}
</style>
