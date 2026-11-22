<script setup lang="ts">
import { NTreeSelect } from "naive-ui";
import { useOverlayReporting } from "./useOverlayReporting";

const { onUpdateShow, resolvedShow } = useOverlayReporting("tree-select");
</script>

<template>
  <NTreeSelect :show="resolvedShow" @update:show="onUpdateShow">
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NTreeSelect>
</template>
