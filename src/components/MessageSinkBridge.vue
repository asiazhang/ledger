<script setup lang="ts">
import { useMessage } from "naive-ui";
import { registerToastSink } from "@ledger/loadable";

registerToastSink(useMessage());
</script>

<template></template>
