<script setup lang="ts">
import { computed, ref } from 'vue'
import { NAlert, NButton, NCard, NSpace, NText, useMessage } from 'naive-ui'
import { api } from '@/api'
import { t } from '@/i18n'
import { errorMessage } from '@/utils/errors'
import type { NotePinyinRepairReport } from '@/types'

// 拼音搜索数据卡片（issue #513）：交易搜索的拼音辅助数据（备注拼音冗余列）
// 一键修复入口。修复语义全部在命令层（幂等回填全部积压、返回报告），本组件
// 只触发命令与呈现报告——回填行数 / 是否收敛 / 失败原因就地展示，不静默。

const message = useMessage()

const report = ref<NotePinyinRepairReport | null>(null)
const repairing = ref(false)

/** 报告呈现形态：失败 → warning（带阶段文案与底层消息）；未收敛 → warning；
 *  收敛 → success（区分「本次有回填」与「无需修复」）。 */
const reportView = computed(() => {
  const r = report.value
  if (!r) return null
  if (r.failure) {
    return {
      type: 'warning' as const,
      title: t('settings.data.search.report.notConvergedTitle'),
      body: t('settings.data.search.report.failureBody', {
        stage: t(`settings.data.search.report.stage.${r.failure.stage}`),
        msg: r.failure.message,
      }),
    }
  }
  if (!r.converged) {
    return {
      type: 'warning' as const,
      title: t('settings.data.search.report.notConvergedTitle'),
      body: t('settings.data.search.report.notConvergedBody'),
    }
  }
  return {
    type: 'success' as const,
    title:
      r.backfilled > 0
        ? t('settings.data.search.report.doneTitle', { n: r.backfilled })
        : t('settings.data.search.report.noopTitle'),
    body:
      r.backfilled > 0
        ? t('settings.data.search.report.doneBody', { n: r.backfilled })
        : t('settings.data.search.report.noopBody'),
  }
})

/** 触发一键修复：幂等，重复执行安全；报告就地覆盖呈现。 */
async function repair() {
  repairing.value = true
  try {
    report.value = await api.repairNotePinyin()
    if (report.value.failure || !report.value.converged) {
      message.warning(t('settings.data.search.msg.incomplete'))
    } else {
      message.success(
        t('settings.data.search.msg.done', { n: report.value.backfilled }),
      )
    }
  } catch (e: any) {
    message.error(t('settings.data.search.msg.repairFailed', { msg: errorMessage(e) }))
  } finally {
    repairing.value = false
  }
}
</script>

<template>
  <NCard :title="t('settings.data.search.title')" size="small">
    <NSpace vertical :size="12">
      <NText depth="3">
        {{ t('settings.data.search.hint') }}
      </NText>

      <NAlert
        v-if="reportView"
        :type="reportView.type"
        :show-icon="true"
        :title="reportView.title"
      >
        {{ reportView.body }}
      </NAlert>

      <NSpace align="center" :size="12">
        <NButton size="small" type="primary" :loading="repairing" @click="repair">
          {{ t('settings.data.search.repair') }}
        </NButton>
      </NSpace>
    </NSpace>
  </NCard>
</template>
