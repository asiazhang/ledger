<script setup lang="ts">
import { pinyinFilter } from "@ledger/utils/pinyin-filter";
import AppSelect from "./AppSelect.vue";
</script>

<template>
  <AppSelect filterable :filter="pinyinFilter">
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </AppSelect>
</template>
