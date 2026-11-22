<script setup lang="ts">
import { NProgress, NText } from "naive-ui";
import { computed } from "vue";
import { t } from "@ledger/i18n";
import type {
  PassphraseStrengthAssessment,
  PassphraseStrengthTier,
} from "@ledger/utils/passphrase-strength";

const props = defineProps<{ assessment: PassphraseStrengthAssessment }>();

const TIER_VIEW: Record<
  PassphraseStrengthTier,
  {
    status: "error" | "warning" | "success" | "info";
    labelKey: string;
  }
> = {
  weak: { status: "error", labelKey: "settings.data.encryption.passphraseStrength.weak" },
  medium: { status: "warning", labelKey: "settings.data.encryption.passphraseStrength.medium" },
  strong: { status: "success", labelKey: "settings.data.encryption.passphraseStrength.strong" },
  "very-strong": {
    status: "info",
    labelKey: "settings.data.encryption.passphraseStrength.veryStrong",
  },
};

const view = computed(() => TIER_VIEW[props.assessment.tier]);
</script>

<template>
  <div class="strength-meter" data-testid="passphrase-strength">
    <NProgress
      type="line"
      :height="4"
      :show-indicator="false"
      :percentage="assessment.percent"
      :status="view.status"
    />
    <NText :type="view.status" class="strength-label">
      {{ t(view.labelKey) }}
    </NText>
  </div>
</template>

<style scoped>
.strength-meter {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: -10px;
}

.strength-label {
  font-size: 12px;
  flex: none;
}
</style>
