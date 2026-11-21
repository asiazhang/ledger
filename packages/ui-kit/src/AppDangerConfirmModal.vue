<script setup lang="ts">
import { NAlert, NButton, NSpace, NText } from "naive-ui";
import AppModal from "./AppModal.vue";

withDefaults(
  defineProps<{
    level: "error" | "warning";
    show: boolean;
    title: string;
    lead?: string;
    alertTitle?: string;
    strongWarning?: string;
    detail?: string;
    confirmText: string;
    cancelText: string;
    submitting?: boolean;
    onConfirm: () => void;
    onCancel: () => void;
  }>(),
  { submitting: false },
);

const emit = defineEmits<{ "update:show": [value: boolean] }>();

function onUpdateShow(value: boolean) {
  if (!value) emit("update:show", false);
}
</script>

<template>
  <AppModal :show="show" preset="card" :title="title" card-size="md" @update:show="onUpdateShow">
    <NSpace vertical :size="12">
      <NText v-if="lead" depth="3">{{ lead }}</NText>

      <!-- 加粗警示句：error 级红色警示块（后果说明必选）；warning 级琥珀警示块 -->
      <NAlert v-if="strongWarning" :type="level" :show-icon="true" :title="alertTitle">
        <NText strong :type="level === 'error' ? 'error' : undefined">{{ strongWarning }}</NText>
      </NAlert>

      <NText v-if="detail" depth="3">{{ detail }}</NText>

      <NSpace justify="end">
        <NButton :disabled="submitting" data-testid="danger-cancel" @click="onCancel">
          {{ cancelText }}
        </NButton>
        <NButton
          :type="level"
          :loading="submitting"
          :disabled="submitting"
          data-testid="danger-confirm"
          @click="onConfirm"
        >
          {{ confirmText }}
        </NButton>
      </NSpace>
    </NSpace>
  </AppModal>
</template>
