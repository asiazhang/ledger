<script setup lang="ts">
/**
 * 设备偏好镜像推送宿主（issue #570 配套）：渲染的空壳组件，唯一职责是把
 * useDevicePreferenceSync 的挂载点搬进 App.vue 的「已解锁」分支——锁定期间
 * 不挂载本组件，设备偏好推送（set_auto_backup_dir / set_auto_execution_enabled）
 * 就不会在解锁前发出（解锁先于一切业务读写）；解锁成功后随主界面一起挂载，
 * immediate watch 立即补推一次，推送语义与搬迁前完全一致。
 */
import { useDevicePreferenceSync } from '@/composables/useDevicePreferenceSync'

useDevicePreferenceSync()
</script>

<template>
  <!-- 渲染空壳：仅承载 composable 生命周期 -->
</template>
