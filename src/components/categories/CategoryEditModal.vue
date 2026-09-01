<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { computed, ref, watch } from 'vue'
import { NButton, NForm, NFormItem, NInput, useMessage } from 'naive-ui'
import AppModal from '@/components/AppModal.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { t } from '@/i18n'
import type { Category, CategoryUpdateInput } from '@/types'

const props = defineProps<{
  show: boolean
  category: Category | null
}>()
const emit = defineEmits<{ 'update:show': [value: boolean] }>()

const reference = useReferenceStore()
const message = useMessage()

const editName = ref('')
const editIcon = ref('')
const editParentId = ref<string>('')

const editParentOptions = computed(() => [
  { label: t('settings.categories.form.parentNone'), value: '' },
  ...reference.rootCategories
    .filter((c) => c.kind === props.category?.kind)
    .map((c) => ({ label: c.name, value: c.id })),
])

// 打开弹窗时同步待编辑字段（弹窗内容在关闭后仍保留在 DOM，需在打开时回填）
watch(
  () => [props.show, props.category],
  () => {
    if (!props.show || !props.category) return
    editName.value = props.category.name
    editIcon.value = props.category.icon ?? ''
    editParentId.value = props.category.parent_id ?? ''
  },
)

async function saveEdit() {
  const cat = props.category
  if (!cat) return
  if (!editName.value.trim()) {
    message.warning(t('settings.categories.msg.nameRequired'))
    return
  }
  const input: CategoryUpdateInput = {
    name: editName.value,
    icon: editIcon.value || null,
    parent_id: editParentId.value || null,
  }
  try {
    await api.updateCategory(cat.id, input)
    message.success(t('settings.categories.msg.updated'))
    emit('update:show', false)
    // 参考数据由 ledger:changed 信号自动重拉，分类树随之更新
  } catch (e) {
    message.error(t('settings.categories.msg.updateFailed', { msg: errorMessage(e) }))
  }
}
</script>

<template>
  <AppModal
    :show="show"
    :title="t('settings.categories.editModal.title')"
    preset="card"
    style="width: 420px"
    :bordered="false"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="left" :show-feedback="false" size="small">
      <NFormItem :label="t('settings.categories.form.name')">
        <NInput v-model:value="editName" :placeholder="t('settings.categories.form.namePlaceholder')" />
      </NFormItem>
      <NFormItem :label="t('settings.categories.form.icon')">
        <NInput v-model:value="editIcon" :placeholder="t('settings.categories.form.iconPlaceholder')" style="width: 120px" />
      </NFormItem>
      <NFormItem :label="t('settings.categories.form.parent')">
        <PinyinSelect
          v-model:value="editParentId"
          :options="editParentOptions"
          :placeholder="t('settings.categories.form.parentPlaceholder')"
          clearable
          style="width: 200px"
        />
      </NFormItem>
      <NButton type="primary" block @click="saveEdit">{{ t('settings.categories.form.save') }}</NButton>
    </NForm>
  </AppModal>
</template>
