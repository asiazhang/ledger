<script setup lang="ts">
import { computed } from "vue";
import { NButton, NSpace } from "naive-ui";
import AppPopconfirm from "@ledger/ui-kit/AppPopconfirm.vue";
import { MOBILE_TOUCH_TARGET_STYLE } from "@/components/mobile-cells";
import type { ScheduledPlanRowAction } from "@ledger/scheduled-plan-list";

const props = defineProps<{
  actions: ScheduledPlanRowAction[];
  rowId: string;
  mobile?: boolean;
}>();

const visibleActions = computed(() => props.actions.filter((a) => a.available));
</script>

<template>
  <NSpace v-if="visibleActions.length" :size="mobile ? 2 : 4" :vertical="mobile">
    <template v-for="a in visibleActions" :key="a.key">
      <AppPopconfirm v-if="a.confirm" :on-positive-click="a.run">
        <template #default>{{ a.confirm }}</template>
        <template #trigger>
          <NButton
            :size="mobile ? 'small' : 'tiny'"
            type="error"
            quaternary
            :style="mobile ? MOBILE_TOUCH_TARGET_STYLE : undefined"
            :data-testid="`op-${a.key}-${rowId}`"
          >
            {{ a.label }}
          </NButton>
        </template>
      </AppPopconfirm>
      <NButton
        v-else
        :size="mobile ? 'small' : 'tiny'"
        :style="mobile ? MOBILE_TOUCH_TARGET_STYLE : undefined"
        :data-testid="`op-${a.key}-${rowId}`"
        @click="a.run"
      >
        {{ a.label }}
      </NButton>
    </template>
  </NSpace>
  <span v-else>—</span>
</template>
