<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { NCard, NSpace, NText, useMessage } from 'naive-ui'
import AppSelect from '@/components/AppSelect.vue'
import { api } from '@/api'
import { errorMessage } from '@/utils/errors'
import { useReferenceStore } from '@/stores/reference'
import { t } from '@ledger/i18n'

// 本位币基准卡片（issue #858，LedgerLevelSetting 首个成员）：账本级设置，
// 随多端同步在所有设备一致生效；按 ADR-0022「归属领域决定合到哪」，币种域
// 设置落「分类」页签（参考数据域 Tab），不入「通用」（轻量资格线不含账本级）。
// 显示值一律来自命令返回（AppSettings 权威），不走 localStorage。
const reference = useReferenceStore()
const message = useMessage()

const baseCurrency = ref<string>('CNY')

onMounted(async () => {
  try {
    baseCurrency.value = (await api.getBaseCurrency()).code
  } catch (e) {
    message.error(t('settings.appearance.baseCurrencyLoadFailed', { msg: errorMessage(e) }))
  }
})

async function setBaseCurrency(code: string) {
  try {
    baseCurrency.value = (await api.setBaseCurrency(code)).code
  } catch (e) {
    message.error(t('settings.appearance.baseCurrencySaveFailed', { msg: errorMessage(e) }))
  }
}
</script>

<template>
  <NCard :title="t('settings.appearance.baseCurrency')" size="small">
    <NSpace vertical :size="8">
      <AppSelect
        :value="baseCurrency"
        :options="reference.currencies.map((c) => ({ label: `${c.code} - ${c.name}`, value: c.code }))"
        @update:value="(val: string) => setBaseCurrency(val)"
        style="max-width: 280px"
      />
      <NText depth="3" style="font-size: 12px">
        {{ t('settings.appearance.baseCurrencyHint') }}
      </NText>
    </NSpace>
  </NCard>
</template>
