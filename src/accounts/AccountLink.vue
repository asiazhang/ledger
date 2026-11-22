<script setup lang="ts">
import { computed } from "vue";
import { useRouter } from "vue-router";
import { useAppStore } from "@/stores/app";
import { useReferenceStore } from "@/stores/reference";
import { accentColor } from "@ledger/theme/overrides";
import { t } from "@ledger/i18n";

const props = defineProps<{
  accountId: string;
}>();

const reference = useReferenceStore();
const router = useRouter();
const app = useAppStore();

const account = computed(() => reference.accountMap.get(props.accountId));
const name = computed(() => account.value?.name ?? "-");
const isLink = computed(() => !!account.value);

const accent = computed(() => accentColor(app.theme));

function go() {
  router.push({ name: "transactions", query: { account: props.accountId } });
}
</script>

<template>
  <button
    v-if="isLink"
    type="button"
    class="account-link"
    :title="t('common.link.viewAccount')"
    :style="{ color: accent.base, '--accent-hover': accent.hover }"
    @click="go"
  >
    {{ name }}
  </button>
  <span v-else class="account-placeholder">{{ name }}</span>
</template>

<style scoped>
.account-link {
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
.account-link:hover {
  color: var(--accent-hover);
  background: rgba(255, 255, 255, 0.06);
  text-decoration: underline;
}
.account-link:focus-visible {
  outline: 2px solid var(--accent-hover);
  outline-offset: 2px;
}
.account-placeholder {
  color: inherit;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
