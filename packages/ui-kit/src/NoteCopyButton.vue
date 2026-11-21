<script setup lang="ts">
import { NButton, NIcon } from "naive-ui";
import { CopyOutline } from "@vicons/ionicons5";
import { t } from "@ledger/i18n";
import { useCopyNote } from "./note-copy";

/**
 * 交易备注复制按钮（显式复制通道，见 CONTEXT-ui-interaction「界面文本不可选」）：
 * 全局文本不可选，备注复制经本按钮 + clipboard API 显式完成，复制完整备注而非
 * 截断显示文本；成功/失败 toast 与无障碍标签均经 i18n。悬停显现样式收口
 * global.css（.note-copy-btn：指针轴悬停行/键盘聚焦显现，触控轴常显），组件不持显隐状态。
 */
// 复制动作与成功/失败 toast 收口共享出口（issue #2189：桌面表格轻量复制按钮经
// 表级事件委托消费同一函数，两处零口径漂移）。
const props = defineProps<{ note: string }>();
const copyNote = useCopyNote();

/** 点击处理：备注取 props（统一出口签名带载荷），事件对象不进复制语义。 */
function onCopy() {
  void copyNote(props.note);
}
</script>

<template>
  <NButton
    size="tiny"
    quaternary
    circle
    class="note-copy-btn"
    :title="t('transactions.list.copyNote')"
    :aria-label="t('transactions.list.copyNote')"
    @click="onCopy"
  >
    <template #icon>
      <NIcon :size="14"><CopyOutline /></NIcon>
    </template>
  </NButton>
</template>
