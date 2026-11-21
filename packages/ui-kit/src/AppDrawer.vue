<script setup lang="ts">
import { NDrawer } from "naive-ui";
import { useOverlayReporting } from "./useOverlayReporting";

const { onUpdateShow, resolvedShow } = useOverlayReporting("drawer");
</script>

<template>
  <NDrawer :show="resolvedShow" @update:show="onUpdateShow">
    <slot />
  </NDrawer>
</template>
