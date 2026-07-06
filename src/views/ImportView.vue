<script setup lang="ts">
import { h, onMounted, ref } from 'vue'
import {
  NCard,
  NButton,
  NDataTable,
  NSelect,
  NSpace,
  NText,
  NEmpty,
  NTag,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import { open } from '@tauri-apps/plugin-dialog'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import { formatAmount } from '@/types'
import type { Account, ImportedRow } from '@/types'

const store = useAppStore()
const message = useMessage()
const rows = ref<ImportedRow[]>([])
const accountId = ref<string | null>(null)
const importing = ref(false)

const accountOptions = () =>
  store.accounts.map((a) => ({ label: a.name, value: a.id }))

async function pickFile() {
  const selected = await open({
    multiple: false,
    filters: [
      { name: '账单文件', extensions: ['csv', 'xlsx', 'xls'] },
    ],
  })
  if (!selected || typeof selected !== 'string') return
  try {
    rows.value = await api.previewImport(selected)
    message.success(`已解析 ${rows.value.length} 条记录`)
  } catch (e) {
    message.error(`解析失败: ${e}`)
  }
}

async function doImport() {
  if (!accountId.value) {
    message.warning('请选择目标账户')
    return
  }
  if (rows.value.length === 0) {
    message.warning('没有可导入的数据')
    return
  }
  const account = store.accountMap.get(accountId.value) as Account | undefined
  if (!account) return
  importing.value = true
  let ok = 0
  try {
    for (const r of rows.value) {
      const kind = r.amount_cents >= 0 ? 'income' : 'expense'
      try {
        await api.createTransaction({
          kind,
          amount_cents: Math.abs(r.amount_cents),
          currency_code: account.currency_code,
          account_id: accountId.value,
          note: r.note || null,
          date: r.date,
        })
        ok++
      } catch {
        // 单条失败继续导入其余
      }
    }
    message.success(`成功导入 ${ok} 条`)
    rows.value = []
  } finally {
    importing.value = false
  }
}

const columns: DataTableColumns<ImportedRow> = [
  { title: '日期', key: 'date', width: 120 },
  {
    title: '金额',
    key: 'amount_cents',
    render: (row) =>
      row.amount_cents >= 0
        ? h(NTag, { type: 'success' }, () => formatAmount(row.amount_cents))
        : h(NTag, { type: 'error' }, () => formatAmount(row.amount_cents)),
  },
  { title: '备注', key: 'note' },
  { title: '分类', key: 'category_name', render: (row) => row.category_name ?? '-' },
]

onMounted(async () => {
  await store.loadAll()
})
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="导入账单" size="small">
      <NSpace vertical :size="12">
        <NText depth="3">
          支持 CSV / Excel 文件，表头需包含 日期、金额（正=收入，负=支出）、备注/描述（可选）、分类（可选）。
        </NText>
        <NSpace :size="12" align="center">
          <NButton @click="pickFile">选择文件</NButton>
          <NSelect
            v-model:value="accountId"
            :options="accountOptions()"
            placeholder="目标账户"
            style="width: 200px"
          />
          <NButton
            type="primary"
            :loading="importing"
            :disabled="rows.length === 0"
            @click="doImport"
          >
            导入 {{ rows.length }} 条
          </NButton>
        </NSpace>
      </NSpace>
    </NCard>

    <NCard title="预览" size="small">
      <NEmpty v-if="rows.length === 0" description="请先选择文件" />
      <NDataTable v-else :columns="columns" :data="rows" :bordered="false" size="small" :max-height="420" />
    </NCard>
  </NSpace>
</template>
