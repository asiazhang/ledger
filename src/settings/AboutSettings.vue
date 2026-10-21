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

/**
 * 「关于」Tab 卡片（issue #930 / #1957 / #1959 / ADR-0022 修订注记）：只读元信息 +
 * 读型动作 + 静态简介（#1959：卡片顶部定位语与简介，纯静态只读、无任何入口，
 * 不进复制文本）。设置入口不在此——日志归「通用」日志卡片、应用更新归「通用」
 * 更新卡片、数据位置归「数据」页签（ADR-0022 的「一问一处」）。
 *
 * 三个字段的真源各归其位：版本取壳层命令 `app_build_info`（= 安装包版本，改前
 * 读前端 `package.json`，与更新比较用的版本两处双写）；平台与架构取编译期常量
 * （改前那行写的是技术栈 `Tauri + Vue 3 + TypeScript`，名实不符）；Git 版本是
 * 构建期注入的 sha。
 *
 * 信息清单与复制文本同源：屏幕上的每一行都由同一个 `rows` 派生，复制出的文本
 * 就是这些行本身——不出现"复制的里面有、屏幕上没有"的隐藏字段。
 *
 * 失败反馈走 `useLoadable` 的统一错误通道（#1008 单点 toast sink），本组件不自带
 * `message.error`——异步守门的存量基线随之收缩（ADR-0040 / #1039）。
 */

const message = useMessage();
const { buildInfo } = useAppBuildInfo();
const gitVersion = gitVersionLabel();

/** 许可证与版权：逐字取仓库 LICENSE（MIT / Copyright (c) 2026 asiazhang）。
 *  年份刻意不动态取当前年——动态年份跨年后就与 LICENSE 不一致。 */
const LICENSE_NAME = "MIT";
const COPYRIGHT_NOTICE = "© 2026 asiazhang";

/** 元信息行：key 只作渲染与测试定位（`about-row-<key>`），label/value 即用户所见。 */
interface AboutRow {
  key: string;
  label: string;
  value: string;
}

/** 屏幕行清单（复制文本的唯一来源）。构建元信息未到位时版本/平台两行不渲染，
 *  与 Git 版本行同形：宁缺不填假值。 */
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

/** 一行文本（`标签：值`）：分隔符归 i18n（中文全角、英文半角加空格）。 */
function rowLine(row: AboutRow): string {
  return `${row.label}${t("settings.about.valueSeparator")}${row.value}`;
}

async function copyToClipboard(text: string, okKey: string): Promise<void> {
  await navigator.clipboard.writeText(text);
  message.success(t(okKey));
}

/** 复制动作的待办载荷：`useLoadable` 的任务是不带参闭包（闭包内自读响应式参数，
 *  见模块接口纪律），故待复制文本经 ref 传递。 */
const pendingCopy = ref<{ text: string; okKey: string } | null>(null);

/** 复制失败经 `useLoadable` 的统一错误通道（#1008 单点 toast sink）反馈：本组件
 *  不再自带 `message.error(t(errorMessage(…)))`——异步守门的存量基线随之下调。 */
const copyTask = useLoadable(async () => {
  const job = pendingCopy.value;
  if (job === null) return;
  await copyToClipboard(job.text, job.okKey);
});

/** 发起一次复制（成功提示归任务末尾，失败归统一错误通道）。 */
function requestCopy(text: string, okKey: string): void {
  pendingCopy.value = { text, okKey };
  void copyTask.run();
}

/** 复制版本信息：整屏可见行（报障时一次粘贴即可，替代手抄版本号与 sha）。 */
function copyVersionInfo(): void {
  requestCopy(rows.value.map(rowLine).join("\n"), "settings.about.copyInfoOk");
}

/** Git 行点击：复制完整 40 位 sha（行内展示的是 7 位短 sha）。 */
function copyGitSha(): void {
  requestCopy(gitShaFull(), "settings.about.copyShaOk");
}

/** 行点击：只有 Git 行是动作，其余行无交互（不把整张卡变成可点区域）。 */
function onRowClick(row: AboutRow): void {
  if (row.key === "git") copyGitSha();
}

/** 外部链接的待打开地址（同复制动作的 ref 载荷形态）。 */
const pendingLink = ref<string | null>(null);

/** 打开链接：失败同样归统一错误通道（不静默吞，也不另起 message.error）。 */
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
