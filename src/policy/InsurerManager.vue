<script setup lang="ts">
import { h, ref, computed } from "vue";
import {
  NButton,
  NCard,
  NCheckbox,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NSpace,
  NTag,
  useMessage,
  type DataTableColumn,
} from "naive-ui";
import AppPopconfirm from "@ledger/ui-kit/AppPopconfirm.vue";
import InsurerEditModal from "@/policy/InsurerEditModal.vue";
import { api } from "@ledger/api";
import { useReferenceStore } from "@/stores/reference";
import { useModalIntent } from "@ledger/modal-intent";
import { matchLabel } from "@ledger/utils/pinyin-filter";
import { t } from "@ledger/i18n";
import type { Insurer, InsurerInput } from "@ledger/types";

const reference = useReferenceStore();
const message = useMessage();

const rows = computed<Insurer[]>(() => reference.insurers);

const showDeleted = ref(false);

const deletedRows = computed<Insurer[]>(() => [...reference.deletedInsurers.values()]);

const searchTerm = ref("");

const displayRows = computed<Insurer[]>(() => {
  const base = showDeleted.value ? [...rows.value, ...deletedRows.value] : rows.value;
  return base.filter((i) => matchLabel(searchTerm.value, i.name));
});

const name = ref("");

async function addInsurer() {
  const trimmed = name.value.trim();
  if (!trimmed) {
    message.warning(t("policies.insurers.msg.nameRequired"));
    return;
  }
  const input: InsurerInput = { name: trimmed };
  try {
    await api.createInsurer(input);
    message.success(t("policies.insurers.msg.added"));
    name.value = "";
  } catch (e) {
    message.error(t("policies.insurers.msg.addFailed", { msg: e }));
  }
}

interface InsurerEditIntent {
  insurer: Insurer;
}

const {
  intent: editIntent,
  seq: editSeq,
  open: openEditIntent,
  close: closeEdit,
} = useModalIntent<InsurerEditIntent>();

function openEdit(insurer: Insurer) {
  openEditIntent({ insurer });
}

async function removeInsurer(id: string) {
  try {
    await api.deleteInsurer(id);
    message.success(t("policies.insurers.msg.deleted"));
  } catch (e) {
    message.error(t("policies.insurers.msg.deleteFailed", { msg: e }));
  }
}

const columns: DataTableColumn<Insurer>[] = [
  {
    title: () => t("policies.insurers.columns.name"),
    key: "name",
    ellipsis: { tooltip: true },
    render: (i) =>
      i.is_deleted
        ? h(NSpace, { size: "small", align: "center", wrap: false }, () => [
            h("span", i.name),
            h(NTag, { size: "small", bordered: false }, () => t("policies.insurers.deletedTag")),
          ])
        : i.name,
  },
  {
    title: () => t("policies.insurers.columns.actions"),
    key: "actions",
    width: 140,
    render: (i) =>
      i.is_deleted
        ? null
        : h(NSpace, { size: "small" }, () => [
            h(
              NButton,
              { size: "tiny", quaternary: true, type: "primary", onClick: () => openEdit(i) },
              () => t("policies.insurers.rowActions.edit"),
            ),
            h(
              AppPopconfirm,
              { onPositiveClick: () => removeInsurer(i.id) },
              {
                default: () => t("policies.insurers.deleteConfirm"),
                trigger: () =>
                  h(NButton, { size: "tiny", type: "error", quaternary: true }, () =>
                    t("policies.insurers.rowActions.delete"),
                  ),
              },
            ),
          ]),
  },
];
</script>

<template>
  <NSpace vertical :size="16">
    <NCard :title="t('policies.insurers.addTitle')" size="small">
      <NForm label-placement="left" :show-feedback="false" inline size="small">
        <NFormItem :label="t('policies.insurers.form.name')">
          <NInput
            v-model:value="name"
            :placeholder="t('policies.insurers.form.namePlaceholder')"
            style="width: 160px"
          />
        </NFormItem>
        <NButton type="primary" @click="addInsurer">{{ t("policies.insurers.form.add") }}</NButton>
      </NForm>
    </NCard>

    <NCard :title="t('policies.insurers.listTitle')" size="small">
      <NSpace vertical :size="12">
        <NSpace align="center">
          <NInput
            v-model:value="searchTerm"
            clearable
            :placeholder="t('policies.insurers.searchPlaceholder')"
            style="width: 240px"
          />
          <NCheckbox v-model:checked="showDeleted">
            {{ t("policies.insurers.showDeleted") }}
          </NCheckbox>
        </NSpace>
        <NDataTable
          :columns="columns"
          :data="displayRows"
          :bordered="false"
          size="small"
          :row-key="(i: Insurer) => i.id"
        />
      </NSpace>
    </NCard>

    <!-- 保司编辑弹窗。显示由「意图非空」派生（无独立 show 布尔），关闭（✕ / ESC /
         取消 / 保存成功）统一经工厂清回 null 终态；序号作 key 强制重建（ADR-0072）。 -->
    <InsurerEditModal
      :key="editSeq"
      :show="editIntent !== null"
      :insurer="editIntent?.insurer ?? null"
      @update:show="(v: boolean) => (v ? undefined : closeEdit())"
    />
  </NSpace>
</template>
