<script setup lang="ts">
import { ref } from "vue";
import { NButton, NIcon } from "naive-ui";
import { AddOutline } from "@vicons/ionicons5";
import type { CreateTransactionKind } from "@ledger/types";
import { t } from "@ledger/i18n";
import AppPopover from "@ledger/ui-kit/AppPopover.vue";
import {
  CREATE_FAB_CLASS,
  CREATE_FAB_SHEET_CLASS,
  CREATE_FAB_OPTION_CLASS,
} from "./create-fab.css.ts";

const props = defineProps<{
  kinds: readonly CreateTransactionKind[];
}>();

const emit = defineEmits<{
  select: [kind: CreateTransactionKind];
}>();

const show = ref(false);

function pick(kind: CreateTransactionKind): void {
  show.value = false;
  emit("select", kind);
}
</script>

<template>
  <AppPopover trigger="click" placement="top-end" :show="show" @update:show="show = $event">
    <template #trigger>
      <NButton
        :class="CREATE_FAB_CLASS"
        circle
        type="primary"
        :aria-label="t('transactions.create.button')"
      >
        <NIcon :component="AddOutline" />
      </NButton>
    </template>
    <div :class="CREATE_FAB_SHEET_CLASS" role="menu" :aria-label="t('transactions.create.title')">
      <NButton
        v-for="kind in props.kinds"
        :key="kind"
        :class="CREATE_FAB_OPTION_CLASS"
        size="large"
        role="menuitem"
        block
        @click="pick(kind)"
      >
        {{ t(`transactions.kind.${kind}`) }}
      </NButton>
    </div>
  </AppPopover>
</template>
