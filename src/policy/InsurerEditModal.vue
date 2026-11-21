<script setup lang="ts">
import { ref, watch } from "vue";
import { NButton, NForm, NFormItem, NInput, NSpace, useMessage } from "naive-ui";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import type { Insurer, InsurerUpdateInput } from "@ledger/types";

const props = defineProps<{
  show: boolean;
  insurer: Insurer | null;
}>();
const emit = defineEmits<{ "update:show": [value: boolean] }>();

const message = useMessage();

const editName = ref("");

watch(
  () => [props.show, props.insurer],
  () => {
    if (!props.show || !props.insurer) return;
    editName.value = props.insurer.name;
  },
  { immediate: true },
);

async function saveEdit() {
  const insurer = props.insurer;
  if (!insurer) return;
  const name = editName.value.trim();
  if (!name) {
    message.warning(t("policies.insurers.msg.nameRequired"));
    return;
  }
  const input: InsurerUpdateInput = { name };
  try {
    await api.updateInsurer(insurer.id, input);
    message.success(t("policies.insurers.msg.updated"));
    emit("update:show", false);
  } catch (e) {
    message.error(t("policies.insurers.msg.updateFailed", { msg: e }));
  }
}
</script>

<template>
  <!-- 卡片外观走 AppModal 对话框排版规范（spec #630）：sm 档 + 默认无边框；
       按钮行右对齐单主键（轻量单字段编辑保留无取消键语义，issue #637）。 -->
  <AppModal
    :show="show"
    :title="t('policies.insurers.editModal.title')"
    preset="card"
    card-size="sm"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="left" :show-feedback="false" size="small">
      <!-- 行距由 NSpace 12 统一提供（对话框排版规范，issue #699）：NFormItem 默认零行距 -->
      <NSpace vertical :size="12">
        <NFormItem :label="t('policies.insurers.form.name')">
          <NInput
            v-model:value="editName"
            :placeholder="t('policies.insurers.form.namePlaceholder')"
          />
        </NFormItem>
        <NSpace justify="end">
          <NButton type="primary" @click="saveEdit">{{ t("policies.insurers.form.save") }}</NButton>
        </NSpace>
      </NSpace>
    </NForm>
  </AppModal>
</template>
