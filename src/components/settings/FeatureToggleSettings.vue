<script setup lang="ts">
/**
 * 设置页「功能」Tab 内容（issue #1243 / ADR-0116 决策 2/7/8）：九项可关功能各一行 =
 * 功能名 + 一句简介 + NSwitch，不可关六项不出现（也不出现空行）。
 *
 * 功能名与侧栏同源（viewLabel → common.nav.<id>），不另造第二份名字；简介键
 * settings.features.descriptions.<id> 明示「关闭后什么仍然在跑」——关闭只隐藏入口、
 * 数据与后台行为照旧（ADR-0116 决策 1），知情权由静态文案承担，故不设确认、不设拦截
 * （ADR-0116 决策 8）。开关点选即写 feature-toggles store：设备级 localStorage、
 * 跨账本共享、零后端调用（ADR-0116 决策 6）。
 */
import { NCard, NSpace, NSwitch, NText } from 'naive-ui'
import { CLOSABLE_FEATURES, useFeatureToggleStore } from '@/stores/feature-toggles'
import { t } from '@ledger/i18n'
import { viewLabel } from '@ledger/i18n/view-label'

const store = useFeatureToggleStore()
</script>

<template>
  <NCard :title="t('settings.features.title')" size="small">
    <NSpace vertical :size="12">
      <NText depth="3" style="font-size: 12px">
        {{ t('settings.features.intro') }}
      </NText>
      <NSpace
        v-for="id in CLOSABLE_FEATURES"
        :key="id"
        :data-testid="`feature-toggle-${id}`"
        justify="space-between"
        align="center"
        :size="16"
      >
        <NSpace vertical :size="2">
          <NText>{{ viewLabel(id) }}</NText>
          <NText depth="3" style="font-size: 12px">
            {{ t(`settings.features.descriptions.${id}`) }}
          </NText>
        </NSpace>
        <NSwitch
          :value="!store.isFeatureClosed(id)"
          @update:value="(val: boolean) => store.setFeatureClosed(id, !val)"
        />
      </NSpace>
    </NSpace>
  </NCard>
</template>
