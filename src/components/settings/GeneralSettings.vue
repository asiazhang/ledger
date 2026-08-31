<script setup lang="ts">
import { computed } from 'vue'
import { NCard, NSpace, NSwitch, NText } from 'naive-ui'
import AppSelect from '@/components/AppSelect.vue'
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'
import { t, type LocaleSetting } from '@/i18n'

const store = useAppStore()
const reference = useReferenceStore()

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

    <NCard :title="t('settings.appearance.defaultCurrency')" size="small">
      <AppSelect
        :value="store.defaultCurrency"
        :options="currencyOptions"
        @update:value="(val: string) => store.setDefaultCurrency(val)"
        style="max-width: 280px"
      />
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
