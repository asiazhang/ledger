<script setup lang="ts">
import { NSelect } from "naive-ui";
import { useOverlayReporting } from "./useOverlayReporting";

const { onUpdateShow, resolvedShow } = useOverlayReporting("select");
</script>

<template>
  <NSelect :show="resolvedShow" @update:show="onUpdateShow">
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NSelect>
</template>
