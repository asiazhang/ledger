<script setup lang="ts">
import { NDropdown } from "naive-ui";
import { useOverlayReporting } from "./useOverlayReporting";

const { onUpdateShow, resolvedShow } = useOverlayReporting("dropdown");
</script>

<template>
  <NDropdown :show="resolvedShow" @update:show="onUpdateShow">
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NDropdown>
</template>
