<script setup lang="ts">
import { computed } from "vue";
import { NButton, NIcon } from "naive-ui";
import { InformationCircleOutline } from "@vicons/ionicons5";
import { t } from "@ledger/i18n";
import ConceptTipHost from "@/investment/ConceptTipHost.vue";
import {
  CONCEPT_SCOPE_KEY_SUFFIX,
  type ConceptKey,
  type ConceptScope,
} from "@/investment/concept-tips";
import { conceptLabel } from "./concept-label.css.ts";

const props = defineProps<{
  label: string;
  concept: ConceptKey;
  scope?: ConceptScope;
  testId?: string;
}>();

const ariaLabel = computed(() => t("investments.concepts.tipAria", { concept: props.label }));

const tip = computed(() => {
  const body = t(`investments.concepts.${props.concept}Tip`);
  if (!props.scope) return body;
  const suffix = CONCEPT_SCOPE_KEY_SUFFIX[props.scope];
  return t("investments.concepts.tipTemplate", {
    body,
    scope: t(`investments.concepts.scope${suffix}`),
  });
});
</script>

<template>
  <span :class="conceptLabel">
    <!-- 概念名自成一个元素：与触发器之间的纯空白节点被编译器去除，标签文案不因
         加图标而多出空格（消费方 .text() 逐字口径不变，合计三卡先例） -->
    <span>{{ label }}</span>
    <ConceptTipHost :text="tip">
      <template #default="{ isTouch }">
        <NButton
          text
          :class="isTouch ? 'touch-hit-area' : undefined"
          :style="isTouch ? { '--touch-hit-inset': '-10px -10px' } : undefined"
          :aria-label="ariaLabel"
          :data-testid="testId ? `${testId}-info` : undefined"
        >
          <NIcon :size="14" color="var(--n-label-text-color, #999)">
            <InformationCircleOutline />
          </NIcon>
        </NButton>
      </template>
    </ConceptTipHost>
  </span>
</template>
