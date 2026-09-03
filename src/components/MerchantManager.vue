<script setup lang="ts">
import { h, ref, computed, watch } from 'vue'
import {
  NButton,
  NCard,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NSpace,
  useMessage,
  type DataTableColumn,
} from 'naive-ui'
import AppPopconfirm from '@/components/AppPopconfirm.vue'
import MerchantEditModal from '@/components/merchants/MerchantEditModal.vue'
import { api } from '@/api'
import { useRouter } from 'vue-router'
import { useReferenceStore } from '@/stores/reference'
import { t } from '@/i18n'
import { formatQuantity } from '@/utils/money'
import type { Merchant, MerchantInput } from '@/types'

// 商户管理（issue #189 / ADR-0028）：字典为扁平表（无层级、无 sort_order，按名称排序），
// 交互沿用分类管理先例——新增表单卡片 + 列表卡片 + 编辑弹窗；写入成功后参考数据
// 由 ledger:changed 信号自动重拉，交易列表/表单补全即时更新。
// 商户回归「名字字典」（issue #223）：只处理名称，无图标/颜色列与输入框。
// 关联交易条数列（issue #445，毛笔数口径）：条数为独立只读聚合
// （list_merchant_transaction_counts，实时推导不落库），不进参考 store 四表——
// 列表行仍消费参考数据单一来源，计数按 merchant_id 客户端拼接；
// 商户写入经既有失效信号触发 store 重拉，store version 变化即伴随重拉计数。
// 条数下钻（issue #446）：点击条数直达按该商户过滤的交易列表，与商户名链接
// （MerchantLink）、报表分类下钻同一 URL 下钻机制（?merchant=<id>）；落地后的
// 过滤行为由 TransactionFilter 既有机制承担（含与分类/账户/日期参数 AND 并存、
// 软删商户经 merchantMap 历史交易口径可解析）。本票只负责产生正确的跳转。

interface MerchantRow extends Merchant {
  /** 关联交易条数（毛笔数）：无引用商户为 0 */
  transactionCount: number
}

const reference = useReferenceStore()
const message = useMessage()
const router = useRouter()

/** 条数下钻（issue #446）：按行 id 产生跳转，不对条数/商户状态设门——
 * 条数为 0 点击只见空列表（诚实行为）；软删商户行（#447 引入展示后）同样可下钻。 */
function goMerchantTransactions(m: MerchantRow) {
  router.push({ name: 'transactions', query: { merchant: m.id } })
}

// —— 关联交易条数（独立读模型，非关键路径：失败保留旧值不阻塞字典管理）——
const transactionCounts = ref(new Map<string, number>())

async function loadTransactionCounts() {
  try {
    const rows = await api.listMerchantTransactionCounts()
    transactionCounts.value = new Map(rows.map((r) => [r.merchant_id, r.transaction_count]))
  } catch {
    /* 条数加载失败静默保留旧值（展示 0 优于阻塞字典管理） */
  }
}

// 拉取接缝单点：立即执行一次（初始拉取）+ 参考数据失效重拉（新建/改名/软删商户后）
// 时伴随重拉计数，两条路径收敛在同一个 watch 上
watch(
  () => reference.version,
  () => {
    void loadTransactionCounts()
  },
  { immediate: true },
)

/** 列表行视图模型：参考数据单一来源的商户行 + 客户端拼接的条数（缺失补 0）。 */
const rows = computed<MerchantRow[]>(() =>
  reference.merchants.map((m) => ({
    ...m,
    transactionCount: transactionCounts.value.get(m.id) ?? 0,
  })),
)

// —— 新增 ——
const name = ref('')

async function addMerchant() {
  const trimmed = name.value.trim()
  if (!trimmed) {
    message.warning(t('settings.merchants.msg.nameRequired'))
    return
  }
  const input: MerchantInput = { name: trimmed }
  try {
    await api.createMerchant(input)
    message.success(t('settings.merchants.msg.added'))
    name.value = ''
  } catch (e) {
    // 重名错误（「商户已存在: X」）原样上抛展示，表单不清空、用户可直接修正
    message.error(t('settings.merchants.msg.addFailed', { msg: e }))
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
    message.success(t('settings.merchants.msg.deleted'))
  } catch (e) {
    message.error(t('settings.merchants.msg.deleteFailed', { msg: e }))
  }
}

// —— 列表 ——
const columns: DataTableColumn<MerchantRow>[] = [
  { title: () => t('settings.merchants.columns.name'), key: 'name', width: 200, ellipsis: { tooltip: true } },
  {
    // 关联交易条数（issue #445）：毛笔数、可排序；展示走数字分组口径（数量列）。
    // 点击条数下钻（issue #446）：文字按钮跳转交易列表并携带商户过滤参数，
    // title 与 MerchantLink 同源（common.link.viewMerchant）。
    title: () => t('settings.merchants.columns.transactionCount'),
    key: 'transactionCount',
    width: 110,
    sorter: (a, b) => a.transactionCount - b.transactionCount,
    render: (m) =>
      h(
        NButton,
        {
          text: true,
          type: 'primary',
          title: t('common.link.viewMerchant'),
          onClick: () => goMerchantTransactions(m),
        },
        () => formatQuantity(m.transactionCount),
      ),
  },
  {
    title: () => t('settings.merchants.columns.actions'),
    key: 'actions',
    width: 140,
    render: (m) =>
      h(NSpace, { size: 'small' }, () => [
        h(
          NButton,
          { size: 'tiny', quaternary: true, type: 'primary', onClick: () => openEdit(m) },
          () => t('settings.merchants.rowActions.edit'),
        ),
        h(
          AppPopconfirm,
          { onPositiveClick: () => removeMerchant(m.id) },
          {
            default: () => t('settings.merchants.deleteConfirm'),
            trigger: () =>
              h(
                NButton,
                { size: 'tiny', type: 'error', quaternary: true },
                () => t('settings.merchants.rowActions.delete'),
              ),
          },
        ),
      ]),
  },
]
</script>

<template>
  <NSpace vertical :size="16">
    <NCard :title="t('settings.merchants.addTitle')" size="small">
      <NForm label-placement="left" :show-feedback="false" inline size="small">
        <NFormItem :label="t('settings.merchants.form.name')">
          <NInput v-model:value="name" :placeholder="t('settings.merchants.form.namePlaceholder')" style="width: 160px" />
        </NFormItem>
        <NButton type="primary" @click="addMerchant">{{ t('settings.merchants.form.add') }}</NButton>
      </NForm>
    </NCard>

    <NCard :title="t('settings.merchants.listTitle')" size="small">
      <NDataTable
        :columns="columns"
        :data="rows"
        :bordered="false"
        size="small"
        :row-key="(m: MerchantRow) => m.id"
      />
    </NCard>

    <MerchantEditModal v-model:show="showEditModal" :merchant="editingMerchant" />
  </NSpace>
</template>
