<script setup lang="ts">
/**
 * 应用更新提醒宿主（issue #1431）：渲染空壳 + 提醒弹层，唯一职责是把应用更新
 * 编排（useAppUpdate）的挂载点搬进 App.vue 的「已解锁」分支——锁定期间不挂载，
 * 更新入口形态探测与事件订阅不发出（先例 DevicePreferenceSyncHost：解锁先于
 * 一切业务读写）；解锁成功后随主界面一起挂载，探测/订阅随之就位，启动延迟
 * 静默检查发现的更新经提醒弹层呈现。
 */
import { onMounted } from "vue";
import { useAppUpdate } from "@/composables/useAppUpdate";
import AppUpdateModal from "@/components/AppUpdateModal.vue";

const update = useAppUpdate();
onMounted(() => {
  void update.init();
});
</script>

<template>
  <AppUpdateModal />
</template>
