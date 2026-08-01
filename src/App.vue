<script setup lang="ts">
import { h, onMounted } from 'vue'
import { RouterView, useRouter, useRoute } from 'vue-router'
import {
  NConfigProvider,
  NMessageProvider,
  NDialogProvider,
  NLayout,
  NLayoutSider,
  NLayoutContent,
  NMenu,
  NSpace,
  NText,
  darkTheme,
  type MenuOption,
} from 'naive-ui'
import { useAppStore } from '@/stores/app'

const router = useRouter()
const route = useRoute()
const store = useAppStore()

const menuOptions: MenuOption[] = [
  { label: '概览', key: 'dashboard' },
  { label: '交易', key: 'transactions' },
  { label: '账户', key: 'accounts' },
  { label: '报表', key: 'reports' },
  { label: '投资', key: 'investments' },
  { label: '预算', key: 'budget' },
  { label: '设置', key: 'settings' },
]

function handleSelect(key: string) {
  router.push({ name: key })
}

onMounted(() => {
  store.loadAll()
})

const title = () => h('div', { style: 'padding: 16px 18px; font-size: 18px; font-weight: 600' }, '📒 Ledger')
</script>

<template>
  <NConfigProvider :theme="store.theme === 'dark' ? darkTheme : null">
    <NMessageProvider>
      <NDialogProvider>
        <NLayout has-sider style="height: 100vh">
          <NLayoutSider
            bordered
            :width="200"
            :collapsed-width="0"
            show-trigger="arrow-circle"
            collapse-mode="width"
          >
            <NSpace vertical :size="0">
              <component :is="title" />
              <NMenu
                :options="menuOptions"
                :value="route.name as string"
                @update:value="handleSelect"
              />
            </NSpace>
          </NLayoutSider>
          <NLayout>
            <NLayoutContent content-style="padding: 20px;" :native-scrollbar="false">
              <NSpace vertical :size="16">
                <NText strong style="font-size: 20px">
                  {{ (route.meta.title as string) ?? '' }}
                </NText>
                <RouterView />
              </NSpace>
            </NLayoutContent>
          </NLayout>
        </NLayout>
      </NDialogProvider>
    </NMessageProvider>
  </NConfigProvider>
</template>
