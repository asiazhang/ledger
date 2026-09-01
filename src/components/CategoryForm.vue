<script setup lang="ts">
import {
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NButton,
  NSpace,
} from 'naive-ui'
import PinyinSelect from '@/components/PinyinSelect.vue'
import AppSelect from '@/components/AppSelect.vue'
import AppTreeSelect from '@/components/AppTreeSelect.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import { useCategoryForm } from '@/composables/useCategoryForm'
import { t } from '@/i18n'
import type { Transaction } from '@/types'

// 编辑模式（issue #178）：传入 editing 时回填既有交易并走更新命令，
// kind 由父层按 editing.kind 锁死传入，本组件内不可切换。
const props = defineProps<{
  kind: 'expense' | 'income'
  submitLabel: string
  editing?: Transaction | null
}>()
const emit = defineEmits<{ created: []; saved: [] }>()

const ctx = useCategoryForm(props.kind, {
  onCreated: () => emit('created'),
  onUpdated: () => emit('saved'),
  editing: () => props.editing ?? null,
})
</script>

<template>
  <NForm label-placement="left" :show-feedback="false" size="small">
    <NSpace vertical :size="12">
      <NFormItem :label="t('settings.categories.txForm.amount')">
        <NInputNumber
          v-model:value="ctx.amount.value"
          :min="0"
          :precision="2"
          :placeholder="t('settings.categories.txForm.amountPlaceholder')"
          style="width: 160px"
        />
        <AppSelect
          v-model:value="ctx.currencyCode.value"
          :options="ctx.currencyOptions.value"
          style="width: 130px; margin-left: 8px"
        />
      </NFormItem>

      <NFormItem :label="t('settings.categories.txForm.account')">
        <PinyinSelect
          v-model:value="ctx.accountId.value"
          :options="ctx.accountOptions.value"
          :placeholder="t('settings.categories.txForm.accountPlaceholder')"
          style="width: 200px"
        />
      </NFormItem>

      <NFormItem :label="t('settings.categories.txForm.category')">
        <AppTreeSelect
          v-model:value="ctx.categoryId.value"
          :options="ctx.treeOptions.value"
          filterable
          clearable
          :placeholder="t('settings.categories.txForm.categoryPlaceholder')"
          :consistent-menu-width="false"
          style="width: 220px"
        />
      </NFormItem>

      <NFormItem :label="t('settings.categories.txForm.merchant')">
        <PinyinSelect
          v-model:value="ctx.merchantRef.value"
          :options="ctx.merchantOptions.value"
          tag
          clearable
          :placeholder="t('settings.categories.txForm.merchantPlaceholder')"
          style="width: 220px"
        />
      </NFormItem>

      <NFormItem :label="t('settings.categories.txForm.date')">
        <AppDatePicker v-model:value="ctx.date.value" type="date" style="width: 200px" />
      </NFormItem>

      <NFormItem :label="t('settings.categories.txForm.note')">
        <NInput v-model:value="ctx.note.value" :placeholder="t('settings.categories.txForm.notePlaceholder')" style="width: 280px" />
      </NFormItem>

      <NButton type="primary" @click="ctx.submit">
        {{ editing ? t('settings.categories.txForm.saveEdits') : submitLabel }}
      </NButton>
    </NSpace>
  </NForm>
</template>
