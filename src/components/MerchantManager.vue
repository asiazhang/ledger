<script setup lang="ts">
import { h, ref } from 'vue'
import {
  NButton,
  NCard,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NPopconfirm,
  NSpace,
  useMessage,
  type DataTableColumn,
} from 'naive-ui'
import MerchantEditModal from '@/components/merchants/MerchantEditModal.vue'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import type { Merchant, MerchantInput } from '@/types'

// 商户管理（issue #189 / ADR-0028）：字典为扁平表（无层级、无 sort_order，按名称排序），
// 交互沿用分类管理先例——新增表单卡片 + 列表卡片 + 编辑弹窗；写入成功后参考数据
// 由 ledger:changed 信号自动重拉，交易列表/表单补全即时更新。

const reference = useReferenceStore()
const message = useMessage()

// —— 新增 ——
const name = ref('')
const icon = ref('')
const color = ref('')

async function addMerchant() {
  const trimmed = name.value.trim()
  if (!trimmed) {
    message.warning('请输入商户名称')
    return
  }
  const input: MerchantInput = {
    name: trimmed,
    icon: icon.value || null,
    color: color.value || null,
  }
  try {
    await api.createMerchant(input)
    message.success('已添加商户')
    name.value = ''
    icon.value = ''
    color.value = ''
  } catch (e) {
    // 重名错误（「商户已存在: X」）原样上抛展示，表单不清空、用户可直接修正
    message.error(`添加失败: ${e}`)
  }
}

// —— 编辑 ——
const showEditModal = ref(false)
const editingMerchant = ref<Merchant | null>(null)

function openEdit(m: Merchant) {
  editingMerchant.value = m
  showEditModal.value = true
}

// —— 删除（软删：历史引用照常显示，不可再被新交易选择） ——
async function removeMerchant(id: string) {
  try {
    await api.deleteMerchant(id)
    message.success('已删除商户')
  } catch (e) {
    message.error(`删除失败: ${e}`)
  }
}

// —— 列表 ——
const columns: DataTableColumn<Merchant>[] = [
  { title: '名称', key: 'name', width: 200, ellipsis: { tooltip: true } },
  {
    title: '图标',
    key: 'icon',
    width: 100,
    render: (m) => m.icon ?? '-',
  },
  {
    title: '颜色',
    key: 'color',
    width: 90,
    render: (m) =>
      h('span', {
        style: {
          display: 'inline-block',
          width: '14px',
          height: '14px',
          borderRadius: '3px',
          verticalAlign: 'middle',
          backgroundColor: m.color ?? 'transparent',
          border: '1px solid rgba(128,128,128,0.35)',
        },
      }),
  },
  {
    title: '操作',
    key: 'actions',
    width: 140,
    render: (m) =>
      h(NSpace, { size: 'small' }, () => [
        h(
          NButton,
          { size: 'tiny', quaternary: true, type: 'primary', onClick: () => openEdit(m) },
          () => '编辑',
        ),
        h(
          NPopconfirm,
          { onPositiveClick: () => removeMerchant(m.id) },
          {
            default: () => '确认删除该商户？历史交易仍会显示它的名字。',
            trigger: () =>
              h(
                NButton,
                { size: 'tiny', type: 'error', quaternary: true },
                () => '删除',
              ),
          },
        ),
      ]),
  },
]
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="新增商户" size="small">
      <NForm label-placement="left" :show-feedback="false" inline size="small">
        <NFormItem label="名称">
          <NInput v-model:value="name" placeholder="商户名称" style="width: 160px" />
        </NFormItem>
        <NFormItem label="图标">
          <NInput v-model:value="icon" placeholder="图标名" style="width: 120px" />
        </NFormItem>
        <NFormItem label="颜色">
          <NInput v-model:value="color" placeholder="颜色" style="width: 120px" />
        </NFormItem>
        <NButton type="primary" @click="addMerchant">添加</NButton>
      </NForm>
    </NCard>

    <NCard title="商户列表" size="small">
      <NDataTable
        :columns="columns"
        :data="reference.merchants"
        :bordered="false"
        size="small"
        :row-key="(m: Merchant) => m.id"
      />
    </NCard>

    <MerchantEditModal v-model:show="showEditModal" :merchant="editingMerchant" />
  </NSpace>
</template>
