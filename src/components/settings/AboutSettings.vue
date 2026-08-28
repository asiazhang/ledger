<script setup lang="ts">
import { NButton, NCard, NSpace, NText, useMessage } from "naive-ui";
import { invoke } from "@tauri-apps/api/core";
import pkg from "@/../package.json";
import { gitShaFull, gitVersionLabel } from "@/utils/git-info";

const message = useMessage();
const gitVersion = gitVersionLabel();

async function copyGitSha() {
  try {
    await navigator.clipboard.writeText(gitShaFull());
    message.success("已复制完整版本号");
  } catch (e: any) {
    message.error(`复制完整版本号失败: ${e}`);
  }
}

async function openLogDir() {
  try {
    await invoke("plugin:log|open_log_dir");
  } catch (e: any) {
    message.error(`打开日志目录失败: ${e}`);
  }
}
</script>

<template>
  <NCard title="关于 Ledger" size="small">
    <NSpace vertical :size="8">
      <NText>应用名称：Ledger</NText>
      <NText>版本号：{{ pkg.version }}</NText>
      <NText
        v-if="gitVersion"
        data-testid="git-version"
        title="点击复制完整版本号"
        style="cursor: pointer"
        @click="copyGitSha"
      >
        Git 版本：{{ gitVersion }}
      </NText>
      <NText>构建平台：Tauri + Vue 3 + TypeScript</NText>
      <NButton size="small" @click="openLogDir">打开日志目录</NButton>
    </NSpace>
  </NCard>
</template>
