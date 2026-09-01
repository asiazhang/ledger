<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { computed, ref } from 'vue'
import { NButton, NForm, NFormItem, NInput, useMessage } from 'naive-ui'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { t } from '@/i18n'
import type { CategoryInput, CategoryKind } from '@/types'

const props = defineProps<{ kind: CategoryKind }>()

const reference = useReferenceStore()
const message = useMessage()

const name = ref('')
const parentId = ref<string>('')
const icon = ref('')

const parentOptions = computed(() => [
  { label: t('settings.categories.form.parentNone'), value: '' },
  ...reference.rootCategories
    .filter((c) => c.kind === props.kind)
    .map((c) => ({ label: c.name, value: c.id })),
])

async function addCategory() {
  if (!name.value.trim()) {
    message.warning(t('settings.categories.msg.nameRequired'))
    return
  }
  const parent_id: string | null = parentId.value || null
  if (parent_id != null) {
    const parent = reference.categoryMap.get(parent_id)
    if (!parent) {
      message.warning(t('settings.categories.msg.parentMissing'))
      return
    }
    if (parent.kind !== props.kind) {
      message.warning(t('settings.categories.msg.parentKindMismatch'))
      return
    }
    if (parent.parent_id != null) {
      message.warning(t('settings.categories.msg.parentMustBeRoot'))
      return
    }
  }
  const input: CategoryInput = {
    name: name.value,
    kind: props.kind,
    parent_id,
    icon: icon.value || null,
  }
  try {
    await api.createCategory(input)
    message.success(t('settings.categories.msg.added'))
    name.value = ''
    parentId.value = ''
    icon.value = ''
    // 参考数据由 ledger:changed 信号自动重拉，分类树随之更新
  } catch (e) {
    message.error(t('settings.categories.msg.addFailed', { msg: errorMessage(e) }))
  }
}
</script>

<template>
  <NForm label-placement="left" :show-feedback="false" inline size="small">
    <NFormItem :label="t('settings.categories.form.name')">
      <NInput v-model:value="name" :placeholder="t('settings.categories.form.namePlaceholder')" style="width: 140px" />
    </NFormItem>
    <NFormItem :label="t('settings.categories.form.parent')">
      <PinyinSelect v-model:value="parentId" :options="parentOptions" style="width: 140px" />
    </NFormItem>
    <NFormItem :label="t('settings.categories.form.icon')">
      <NInput v-model:value="icon" :placeholder="t('settings.categories.form.iconPlaceholder')" style="width: 140px" />
    </NFormItem>
    <NButton type="primary" @click="addCategory">{{ t('settings.categories.form.add') }}</NButton>
  </NForm>
</template>
