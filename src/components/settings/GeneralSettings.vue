<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { NCard, NSpace, NSwitch, NText, useMessage } from 'naive-ui'
import AppSelect from '@/components/AppSelect.vue'
import { api } from '@/api'
import { errorMessage } from '@/utils/errors'
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'
import { t, type LocaleSetting } from '@/i18n'

const store = useAppStore()
const reference = useReferenceStore()
const message = useMessage()

const currencyOptions = computed(() =>
  reference.currencies.map((c) => ({ label: `${c.code} - ${c.name}`, value: c.code })),
)

// 界面语言选项（issue #342 / ADR-0049）：具体语言用原生名（中文/English），
// 不随界面语言翻译；「跟随系统」走文案资源。
const languageOptions = computed<{ label: string; value: LocaleSetting }[]>(() => [
  { label: t('common.language.followSystem'), value: 'system' },
  { label: t('common.language.zh'), value: 'zh-CN' },
  { label: t('common.language.en'), value: 'en-US' },
])

// 本位币基准（issue #858，LedgerLevelSetting 首个成员）：账本级设置，随多端
// 同步在所有设备一致生效；显示值一律来自命令返回（AppSettings 权威），不走
// localStorage。展示币种（下方卡片）仍是轻量设备偏好，两者互不相干。
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
  <NSpace vertical :size="16">
    <NCard :title="t('settings.appearance.card')" size="small">
      <NSpace align="center" :size="12">
        <NText>{{ t('settings.appearance.darkMode') }}</NText>
        <NSwitch
          :value="store.theme === 'dark'"
          @update:value="(val: boolean) => store.setTheme(val ? 'dark' : 'light')"
        />
      </NSpace>
    </NCard>

    <NCard :title="t('settings.appearance.baseCurrency')" size="small">
      <NSpace vertical :size="8">
        <AppSelect
          :value="baseCurrency"
          :options="currencyOptions"
          @update:value="(val: string) => setBaseCurrency(val)"
          style="max-width: 280px"
        />
        <NText depth="3" style="font-size: 12px">
          {{ t('settings.appearance.baseCurrencyHint') }}
        </NText>
      </NSpace>
    </NCard>

    <NCard :title="t('settings.appearance.displayCurrency')" size="small">
      <NSpace vertical :size="8">
        <AppSelect
          :value="store.defaultCurrency"
          :options="currencyOptions"
          @update:value="(val: string) => store.setDefaultCurrency(val)"
          style="max-width: 280px"
        />
        <NText depth="3" style="font-size: 12px">
          {{ t('settings.appearance.displayCurrencyHint') }}
        </NText>
      </NSpace>
    </NCard>

    <NCard :title="t('common.language.label')" size="small">
      <AppSelect
        :value="store.localeSetting"
        :options="languageOptions"
        @update:value="(val: LocaleSetting) => store.setLocale(val)"
        style="max-width: 280px"
      />
    </NCard>
  </NSpace>
</template>
