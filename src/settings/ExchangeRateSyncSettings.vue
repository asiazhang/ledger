<script setup lang="ts">
import { computed } from "vue";
import { NAlert, NButton, NCard, NSpace, NText, useMessage } from "naive-ui";
import { t } from "@ledger/i18n";
import { useFxSync } from "@/settings/useFxSync";

// 汇率同步卡片（issue #1545）：设置页「同步汇率」手动入口——一键触发一次汇率
// 同步，进行中不可重复触发（loading 禁用），结果就地呈现（覆盖区间 / 条数）。
// 同步语义全部在命令层（取数深度由窗口判据分派：深度未达走 ECB 全量历史回填、
// 已达走 90 天增量，#1544；幂等落库后返回报告），本组件只触发接缝与呈现报告；
// 失败原因码化三态（数据源不可达 / 该来源无数据 / 报文异常）经 errorMessage 按
// 码本地化后可分辨——toast 走 Loadable 默认策略，错误位就地重显。纯就地反馈、
// 无弹层，不涉 Overlay Suppression 登记。
//
// 任务状态归宿（issue #1762）：syncing、阶段文字与结果报告收进设置域前端的模块级
// 共享单例接缝（`useFxSync`，形态仿 `useInstrumentInfoSync`）——切页卸载重挂不丢
// 状态：按钮保持禁用、阶段文字继续跟随实际进度、完成后报告照常显示。同步期间
// 按钮下方出现一行阶段文字（正在读取 → 携带天数的正在写入），为真实文本内容
// （屏幕阅读器可朗读）；撞车（每日自动同步在途时的手动触发）经后端在途互斥报
// `fx.sync-in-progress`，就地错误位与 toast 同 Loadable 默认策略呈现。

const message = useMessage();

const { syncing, syncError, report, stageText, sync } = useFxSync();

// 成功文案单一拼装点（评审收口）：toast 与成功告警位同文案（同一键，避免双源
// 漂移，#513 先例），插值只在这里发生一次。零痕迹跳过（#1544 判据层合法成功，
// 无覆盖区间）时区间位以 "-" 占位。
const successText = computed(() =>
  report.value
    ? t("settings.fxSync.ok", {
        points: report.value.persist.points,
        earliest: report.value.persist.earliest ?? "-",
        latest: report.value.persist.latest ?? "-",
      })
    : "",
);

/** 触发一次同步：成功 toast 与成功告警位同文案。 */
async function syncNow(): Promise<void> {
  const result = await sync();
  if (result) {
    message.success(successText.value);
  }
}
</script>

<template>
  <NCard :title="t('settings.fxSync.title')" size="small">
    <NSpace vertical :size="12">
      <NText depth="3">{{ t("settings.fxSync.hint") }}</NText>

      <!-- 失败原因优先于上次成功报告（错误位就地重显，错误文案按码本地化可分辨） -->
      <NAlert v-if="syncError" type="error" :show-icon="true">
        {{ syncError }}
      </NAlert>
      <NAlert v-else-if="report" type="success" :show-icon="true">
        {{ successText }}
      </NAlert>

      <NButton size="small" type="primary" :loading="syncing" :disabled="syncing" @click="syncNow">
        {{ t("settings.fxSync.action") }}
      </NButton>

      <!-- 阶段文字（issue #1762）：按钮下方一行真实文本，随真实阶段流转 -->
      <NText v-if="stageText" depth="3">{{ stageText }}</NText>
    </NSpace>
  </NCard>
</template>
