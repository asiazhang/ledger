<script setup lang="ts">
import { computed } from "vue";
import { useRouter } from "vue-router";
import { useAppStore } from "@/stores/app";
import { accentColor } from "@ledger/theme/overrides";
import { t } from "@ledger/i18n";
import {
  INSTRUMENT_LINK_CLASS,
  INSTRUMENT_PLACEHOLDER_CLASS,
} from "@/investment/instrument-link.css";

const props = defineProps<{
  instrumentId: string;
  label: string | null;
  accountId?: string | null;
}>();

const router = useRouter();
const app = useAppStore();

const accent = computed(() => accentColor(app.theme));

function go() {
  router.push({
    name: "investments",
    query: props.accountId
      ? { tab: "detail", account: props.accountId, instrument: props.instrumentId }
      : { tab: "detail", instrument: props.instrumentId },
  });
}
</script>

<template>
  <button
    v-if="label"
    type="button"
    :class="INSTRUMENT_LINK_CLASS"
    :title="t('common.link.viewInstrument')"
    :style="{ color: accent.base, '--accent-hover': accent.hover }"
    @click="go"
  >
    {{ label }}
  </button>
  <span v-else :class="INSTRUMENT_PLACEHOLDER_CLASS">-</span>
</template>
