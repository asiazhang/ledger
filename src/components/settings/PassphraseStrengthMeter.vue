<script setup lang="ts">
import { NProgress, NText } from 'naive-ui'
import { computed } from 'vue'
import { t } from '@/i18n'
import type { PassphraseStrengthAssessment, PassphraseStrengthTier } from '@/utils/passphrase-strength'

// 口令强度条（issue #685，词汇表「口令强度」）：纯展示组件——色条＋四档文字，
// 消费 utils/passphrase-strength.ts 的评估结果，不做任何判定。
// 颜色用组件库语义色（不复用业务语义色）：弱 error / 中 warning / 强 success /
// 极强 info（以 info 区分于强的 success）；不显示破解时间等伪精确数值。

const props = defineProps<{ assessment: PassphraseStrengthAssessment }>()

/** 档位 → 组件库语义色与文案 key。 */
const TIER_VIEW: Record<PassphraseStrengthTier, {
  status: 'error' | 'warning' | 'success' | 'info'
  labelKey: string
}> = {
  weak: { status: 'error', labelKey: 'settings.data.encryption.passphraseStrength.weak' },
  medium: { status: 'warning', labelKey: 'settings.data.encryption.passphraseStrength.medium' },
  strong: { status: 'success', labelKey: 'settings.data.encryption.passphraseStrength.strong' },
  'very-strong': {
    status: 'info',
    labelKey: 'settings.data.encryption.passphraseStrength.veryStrong',
  },
}

const view = computed(() => TIER_VIEW[props.assessment.tier])
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
  /* 上移抵消 NFormItem 底部留白，让色条贴住其所属输入框（视觉归属，非间距语义） */
  margin-top: -10px;
}

.strength-label {
  font-size: 12px;
  flex: none;
}
</style>
