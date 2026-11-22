<script setup lang="ts">
import { NDatePicker } from "naive-ui";
import { useOverlayReporting } from "./useOverlayReporting";

const { onUpdateShow, resolvedShow } = useOverlayReporting("date-picker");
</script>

<template>
  <NDatePicker :show="resolvedShow" @update:show="onUpdateShow">
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NDatePicker>
</template>
