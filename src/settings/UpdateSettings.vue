<script setup lang="ts">
import { computed } from "vue";
import { NButton, NCard, NSpace, NSwitch, NText } from "naive-ui";
import { t } from "@ledger/i18n";
import { useAppStore } from "@/stores/app";
import { useAppUpdate } from "@/composables/useAppUpdate";
import { useAppBuildInfo } from "@/settings/useAppBuildInfo";

/**
 * 设置页更新卡片（issue #1431 / ADR-0124 决策 4/7/8）：应用更新无自有业务域
 * 页签，按 ADR-0022 修订口径落「通用」Tab（不归属业务域页签的应用级设置）。
 *
 * 平台分支（ADR-0124 决策 1）：surface = `updater`（桌面三平台）呈现完整更新面
 * ——当前版本号、手动检查（结果就地呈现、失败就地报错，不受「已提醒版本」记忆
 * 限制）、自动检查开关（轻量设置项、新装默认开启，开关状态经设备偏好镜像推送
 * 交给后端运行期消费）；`release-page`（deb 与 Android）降级为「打开发布页」
 * 引导，不冒充可自更新——无检查、无开关、无下载/安装动作。
 *
 * 下载/安装进行中的流程状态经卡片状态行可观察（弹层被收起时的再入口），动作
 * 只转交编排模块，不在卡片重复发起。
 */
const store = useAppStore();
const { buildInfo } = useAppBuildInfo();
const {
  surface,
  stage,
  version,
  downloadPercent,
  modalOpen,
  flowError,
  manualCheck,
  init,
  checkManually,
  startDownload,
  reopenFlow,
  openReleasePage,
} = useAppUpdate();

// 形态探测随卡片挂载补一次（幂等：宿主已探测时直接复用；直挂卡片也不落空）。
void init();

/** 当前版本号：取安装包真源（`app_build_info`），与「关于」Tab 同源。
 *  改前读前端 `package.json`——与更新比较用的版本是两处独立双写（issue #1957），
 *  漂移时用户看到的版本与更新判断就不是同一个数。 */
const currentVersion = computed(() => buildInfo.value?.version ?? "");

const isUpdaterSurface = computed(() => surface.value === "updater");
/** release-page（deb 与 Android）：降级为发布页引导；null = 形态探测未完成，先不渲染分支。 */
const isReleasePageSurface = computed(() => surface.value === "release-page");

/** 手动检查进行中（按钮 loading + 禁用挡重复触发）。 */
const checking = computed(() => manualCheck.value.kind === "checking");

/** 流程进行中且弹层被收起：卡片状态行作为再入口（下载中/已下载待安装）。 */
const showFlowStatus = computed(
  () => (stage.value === "downloading" || stage.value === "downloaded") && !modalOpen.value,
);

const flowStatusText = computed(() => {
  if (stage.value === "downloading") {
    return downloadPercent.value === null
      ? t("settings.update.cardDownloading")
      : t("settings.update.cardDownloadingPercent", { percent: downloadPercent.value });
  }
  return t("settings.update.cardDownloaded", { version: version.value ?? "" });
});
</script>

<template>
  <NCard :title="t('settings.update.cardTitle')" size="small">
    <NSpace vertical :size="12">
      <!-- 当前版本号：两形态共有（反馈问题或核对更新状态有依据） -->
      <NText data-testid="update-current-version"
        >{{ t("settings.update.currentVersion") }}{{ currentVersion }}</NText
      >
      <!-- release-page（deb 与 Android）：只呈打开发布页引导 -->
      <template v-if="isReleasePageSurface">
        <NText depth="3">{{ t("settings.update.releasePageOnly") }}</NText>
        <NSpace>
          <NButton data-testid="update-open-release-page" @click="openReleasePage">
            {{ t("settings.update.openReleasePage") }}
          </NButton>
        </NSpace>
      </template>

      <!-- updater（桌面三平台）：完整更新面；探测未完成（null）两分支都不渲染 -->
      <template v-else-if="isUpdaterSurface">
        <!-- 手动检查：结果就地呈现、失败就地报错 -->
        <NSpace align="center" :size="12">
          <NButton
            data-testid="update-check-btn"
            :loading="checking"
            :disabled="checking"
            @click="checkManually"
          >
            {{ t("settings.update.checkNow") }}
          </NButton>
          <NText v-if="manualCheck.kind === 'latest'" data-testid="update-check-latest" depth="3">
            {{ t("settings.update.upToDate") }}
          </NText>
          <NText
            v-else-if="manualCheck.kind === 'error'"
            data-testid="update-check-error"
            type="error"
          >
            {{ manualCheck.message }}
          </NText>
        </NSpace>
        <template v-if="manualCheck.kind === 'available'">
          <NSpace vertical :size="4">
            <NText data-testid="update-check-version" strong>
              {{ t("settings.update.newVersion", { version: manualCheck.version }) }}
            </NText>
            <NText v-if="manualCheck.notes" depth="3" style="white-space: pre-wrap">
              {{ manualCheck.notes }}
            </NText>
          </NSpace>
          <NSpace>
            <NButton type="primary" data-testid="update-card-download-btn" @click="startDownload">
              {{ t("settings.update.download") }}
            </NButton>
          </NSpace>
        </template>

        <!-- 自动检查开关：轻量设置项（真源 localStorage，后端持运行期镜像） -->
        <NSpace align="center" :size="12">
          <NSwitch
            :value="store.autoUpdateEnabled"
            data-testid="update-auto-switch"
            @update:value="store.setAutoUpdateEnabled"
          />
          <NText>{{ t("settings.update.autoCheckLabel") }}</NText>
        </NSpace>
        <NText depth="3">{{ t("settings.update.autoCheckDesc") }}</NText>

        <!-- 流程状态行：弹层收起后下载/安装进度的再入口 -->
        <template v-if="showFlowStatus">
          <NText data-testid="update-card-flow-status">{{ flowStatusText }}</NText>
          <NText v-if="flowError" type="error">{{ flowError }}</NText>
          <NSpace>
            <NButton data-testid="update-resume-btn" @click="reopenFlow">
              {{
                stage === "downloaded"
                  ? t("settings.update.continueInstall")
                  : t("settings.update.viewProgress")
              }}
            </NButton>
          </NSpace>
        </template>
      </template>
    </NSpace>
  </NCard>
</template>
