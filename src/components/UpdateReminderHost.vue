<script setup lang="ts">
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
