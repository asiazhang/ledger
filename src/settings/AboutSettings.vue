<script setup lang="ts">
import { computed, ref } from "vue";
import {
  NButton,
  NCard,
  NDescriptions,
  NDescriptionsItem,
  NSpace,
  NText,
  useMessage,
} from "naive-ui";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useLoadable } from "@ledger/loadable";
import { gitShaFull, gitVersionLabel } from "@ledger/utils/git-info";
import { t } from "@ledger/i18n";
import { ISSUES_URL, PROJECT_HOME_URL } from "@/settings/app-links";
import { formatPlatform, useAppBuildInfo } from "@/settings/useAppBuildInfo";

const message = useMessage();
const { buildInfo } = useAppBuildInfo();
const gitVersion = gitVersionLabel();

const LICENSE_NAME = "MIT";
const COPYRIGHT_NOTICE = "© 2026 asiazhang";

interface AboutRow {
  key: string;
  label: string;
  value: string;
}

const rows = computed<AboutRow[]>(() => {
  const list: AboutRow[] = [
    { key: "app-name", label: t("settings.about.appName"), value: t("common.app.name") },
  ];
  if (buildInfo.value !== null) {
    list.push({
      key: "version",
      label: t("settings.about.version"),
      value: buildInfo.value.version,
    });
  }
  if (gitVersion !== "") {
    list.push({ key: "git", label: t("settings.about.gitVersion"), value: gitVersion });
  }
  if (buildInfo.value !== null) {
    list.push({
      key: "platform",
      label: t("settings.about.platform"),
      value: formatPlatform(buildInfo.value),
    });
  }
  list.push({ key: "license", label: t("settings.about.license"), value: LICENSE_NAME });
  list.push({ key: "copyright", label: t("settings.about.copyright"), value: COPYRIGHT_NOTICE });
  return list;
});

function rowLine(row: AboutRow): string {
  return `${row.label}${t("settings.about.valueSeparator")}${row.value}`;
}

async function copyToClipboard(text: string, okKey: string): Promise<void> {
  await navigator.clipboard.writeText(text);
  message.success(t(okKey));
}

const pendingCopy = ref<{ text: string; okKey: string } | null>(null);

const copyTask = useLoadable(async () => {
  const job = pendingCopy.value;
  if (job === null) return;
  await copyToClipboard(job.text, job.okKey);
});

function requestCopy(text: string, okKey: string): void {
  pendingCopy.value = { text, okKey };
  void copyTask.run();
}

function copyVersionInfo(): void {
  requestCopy(rows.value.map(rowLine).join("\n"), "settings.about.copyInfoOk");
}

function copyGitSha(): void {
  requestCopy(gitShaFull(), "settings.about.copyShaOk");
}

function onRowClick(row: AboutRow): void {
  if (row.key === "git") copyGitSha();
}

const pendingLink = ref<string | null>(null);

const linkTask = useLoadable(async () => {
  const url = pendingLink.value;
  if (url === null) return;
  await openUrl(url);
});

function openLink(url: string): void {
  pendingLink.value = url;
  void linkTask.run();
}
</script>

<template>
  <NCard :title="t('settings.about.title')" size="small">
    <NSpace vertical :size="12">
      <!-- 静态简介（issue #1959）：定位语加粗 + 短说明弱化，纯只读无入口，
           且不参与复制版本信息的行清单（报障粘贴格式不变）。 -->
      <NSpace vertical :size="4" data-testid="about-intro">
        <NText strong>{{ t("settings.about.tagline") }}</NText>
        <NText depth="3">{{ t("settings.about.intro") }}</NText>
      </NSpace>
      <NDescriptions :column="1" label-placement="left" size="small">
        <NDescriptionsItem v-for="row in rows" :key="row.key" :label="row.label">
          <NText
            :data-testid="`about-row-${row.key}`"
            :title="row.key === 'git' ? t('settings.about.gitVersionTitle') : undefined"
            :style="row.key === 'git' ? 'cursor: pointer' : undefined"
            @click="onRowClick(row)"
          >
            {{ row.value }}
          </NText>
        </NDescriptionsItem>
      </NDescriptions>

      <NSpace :size="8">
        <NButton size="small" data-testid="about-copy-info" @click="copyVersionInfo">
          {{ t("settings.about.copyInfo") }}
        </NButton>
        <NButton
          size="small"
          quaternary
          data-testid="about-link-home"
          @click="openLink(PROJECT_HOME_URL)"
        >
          {{ t("settings.about.homepage") }}
        </NButton>
        <NButton
          size="small"
          quaternary
          data-testid="about-link-issues"
          @click="openLink(ISSUES_URL)"
        >
          {{ t("settings.about.issues") }}
        </NButton>
      </NSpace>
    </NSpace>
  </NCard>
</template>
