<script setup lang="ts">
import { NButton, NIcon } from "naive-ui";
import { CopyOutline } from "@vicons/ionicons5";
import { t } from "@ledger/i18n";
import { useCopyNote } from "./note-copy";

const props = defineProps<{ note: string }>();
const copyNote = useCopyNote();

function onCopy() {
  void copyNote(props.note);
}
</script>

<template>
  <NButton
    size="tiny"
    quaternary
    circle
    class="note-copy-btn"
    :title="t('transactions.list.copyNote')"
    :aria-label="t('transactions.list.copyNote')"
    @click="onCopy"
  >
    <template #icon>
      <NIcon :size="14"><CopyOutline /></NIcon>
    </template>
  </NButton>
</template>
