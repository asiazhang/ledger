<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { NTabs, NTabPane, NCard, NDataTable, NSpace, NSelect, NSwitch, NText } from 'naive-ui'
import { useAppStore } from '@/stores/app'
import CategoryManager from '@/components/CategoryManager.vue'
import pkg from '@/../package.json'

const store = useAppStore()

const currencyColumns = [
  { title: '代码', key: 'code', width: 80 },
  { title: '名称', key: 'name' },
  { title: '符号', key: 'symbol', width: 80 },
  { title: '小数位', key: 'decimal_places', width: 80 },
]

const currencyOptions = computed(() =>
  store.currencies.map((c) => ({ label: `${c.code} - ${c.name}`, value: c.code })),
)

onMounted(async () => {
  await store.loadAll()
})

function toggleTheme() {
  store.setTheme(store.theme === 'dark' ? 'light' : 'dark')
}
</script>

<template>
  <NTabs type="line" animated>
    <NTabPane name="categories" tab="分类">
      <CategoryManager />
    </NTabPane>

    <NTabPane name="currencies" tab="币种">
      <NSpace vertical :size="16">
        <NCard title="默认币种" size="small">
          <NSelect
            :value="store.defaultCurrency"
            :options="currencyOptions"
            @update:value="(val: string) => store.setDefaultCurrency(val)"
            style="max-width: 280px"
          />
        </NCard>

        <NCard title="支持币种" size="small">
          <NDataTable :columns="currencyColumns" :data="store.currencies" :bordered="false" size="small" />
        </NCard>
      </NSpace>
    </NTabPane>

    <NTabPane name="appearance" tab="外观">
      <NSpace vertical :size="16">
        <NCard title="主题模式" size="small">
          <NSpace align="center" :size="12">
            <NText>深色模式</NText>
            <NSwitch
              :value="store.theme === 'dark'"
              @update:value="toggleTheme"
            />
          </NSpace>
        </NCard>
      </NSpace>
    </NTabPane>

    <NTabPane name="about" tab="关于">
      <NCard title="关于 Ledger" size="small">
        <NSpace vertical :size="8">
          <NText>应用名称：Ledger</NText>
          <NText>版本号：{{ pkg.version }}</NText>
          <NText>构建平台：Tauri + Vue 3 + TypeScript</NText>
        </NSpace>
      </NCard>
    </NTabPane>
  </NTabs>
</template>
