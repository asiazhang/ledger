<script setup lang="ts">
import { computed } from "vue";
import { useRouter } from "vue-router";
import { useAppStore } from "@/stores/app";
import { useReferenceStore } from "@/stores/reference";
import { accentColor } from "@ledger/theme/overrides";
import { t } from "@ledger/i18n";

const props = defineProps<{
  merchantId: string;
  drillIntent?: boolean;
}>();

const emit = defineEmits<{ (e: "drill", merchantId: string): void }>();

const reference = useReferenceStore();
const router = useRouter();
const app = useAppStore();

const merchant = computed(() => reference.merchantMap.get(props.merchantId));
const name = computed(() => merchant.value?.name ?? "-");
const isLink = computed(() => !!merchant.value);

const accent = computed(() => accentColor(app.theme));

function go() {
  if (props.drillIntent) {
    emit("drill", props.merchantId);
    return;
  }
  router.push({ name: "transactions", query: { merchant: props.merchantId } });
}
</script>

<template>
  <button
    v-if="isLink"
    type="button"
    class="merchant-link"
    :title="t('common.link.viewMerchant')"
    :style="{ color: accent.base, '--accent-hover': accent.hover }"
    @click="go"
  >
    {{ name }}
  </button>
  <span v-else class="merchant-placeholder">{{ name }}</span>
</template>

<style scoped>
.merchant-link {
  border: none;
  padding: 0;
  background: none;
  font: inherit;
  cursor: pointer;
  border-radius: 4px;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.merchant-link:hover {
  color: var(--accent-hover);
  background: rgba(255, 255, 255, 0.06);
  text-decoration: underline;
}
.merchant-link:focus-visible {
  outline: 2px solid var(--accent-hover);
  outline-offset: 2px;
}
.merchant-placeholder {
  color: inherit;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
