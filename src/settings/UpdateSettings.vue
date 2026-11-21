<script setup lang="ts">
import { computed } from "vue";
import { NButton, NCard, NSpace, NSwitch, NText } from "naive-ui";
import { t } from "@ledger/i18n";
import { useAppStore } from "@/stores/app";
import { useAppUpdate } from "@/composables/useAppUpdate";
import { useAppBuildInfo } from "@/settings/useAppBuildInfo";

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

void init();

const currentVersion = computed(() => buildInfo.value?.version ?? "");

const isUpdaterSurface = computed(() => surface.value === "updater");
const isReleasePageSurface = computed(() => surface.value === "release-page");

const checking = computed(() => manualCheck.value.kind === "checking");

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
