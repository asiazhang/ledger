<script setup lang="ts">
import {
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NSelect,
  NTreeSelect,
  NDatePicker,
  NButton,
  NSpace,
} from 'naive-ui'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { useCategoryForm } from '@/composables/useCategoryForm'
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
      <NFormItem label="金额">
        <NInputNumber
          v-model:value="ctx.amount.value"
          :min="0"
          :precision="2"
          placeholder="金额"
          style="width: 160px"
        />
        <NSelect
          v-model:value="ctx.currencyCode.value"
          :options="ctx.currencyOptions.value"
          style="width: 130px; margin-left: 8px"
        />
      </NFormItem>

      <NFormItem label="账户">
        <PinyinSelect
          v-model:value="ctx.accountId.value"
          :options="ctx.accountOptions.value"
          placeholder="选择账户"
          style="width: 200px"
        />
      </NFormItem>

      <NFormItem label="分类">
        <NTreeSelect
          v-model:value="ctx.categoryId.value"
          :options="ctx.treeOptions.value"
          filterable
          clearable
          placeholder="选择分类"
          :consistent-menu-width="false"
          style="width: 220px"
        />
      </NFormItem>

      <NFormItem label="日期">
        <NDatePicker v-model:value="ctx.date.value" type="date" style="width: 200px" />
      </NFormItem>

      <NFormItem label="备注">
        <NInput v-model:value="ctx.note.value" placeholder="备注（可选）" style="width: 280px" />
      </NFormItem>

      <NButton type="primary" @click="ctx.submit">
        {{ editing ? '保存修改' : submitLabel }}
      </NButton>
    </NSpace>
  </NForm>
</template>
