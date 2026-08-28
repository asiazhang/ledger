<script setup lang="ts">
import { computed, h, nextTick, onMounted, ref } from 'vue'
import {
  NCard,
  NButton,
  NDataTable,
  NDatePicker,
  NDropdown,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NModal,
  NSelect,
  NSpace,
  NText,
  useDialog,
  useMessage,
  useThemeVars,
  type DataTableColumns,
} from 'naive-ui'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import AccountLink from '@/components/AccountLink.vue'
import { buildAccountRowMenuOptions } from '@/components/account-row-menu'
import { ACCOUNT_TYPE_LABELS, formatAmount } from '@/types'
import type { AccountBalance, AccountInput, AccountType } from '@/types'

const reference = useReferenceStore()
const message = useMessage()
const dialog = useDialog()
const themeVars = useThemeVars()
const balances = ref<AccountBalance[]>([])

const name = ref('')
const type = ref<AccountType>('cash')
const currencyCode = ref('CNY')
const initial = ref<number | null>(0)

const typeOptions = (Object.keys(ACCOUNT_TYPE_LABELS) as AccountType[]).map((k) => ({
  label: ACCOUNT_TYPE_LABELS[k],
  value: k,
}))
const currencyOptions = () =>
  reference.currencies.map((c) => ({ label: `${c.name} (${c.code})`, value: c.code }))

async function refresh() {
  balances.value = await api.listAccountBalances()
}

async function create() {
  if (!name.value.trim()) {
    message.warning('请输入账户名称')
    return
  }
  const input: AccountInput = {
    name: name.value,
    type: type.value,
    currency_code: currencyCode.value,
    initial_balance_cents: Math.round((initial.value ?? 0) * 100),
  }
  try {
    await api.createAccount(input)
    message.success('已创建账户')
    name.value = ''
    initial.value = 0
    // 参考数据由 ledger:changed 信号自动重拉；此处仅刷新交易派生余额
    await refresh()
  } catch (e) {
    message.error(`创建失败: ${e}`)
  }
}

async function remove(id: string) {
  try {
    await api.deleteAccount(id)
    message.success('已删除')
    // 参考数据由 ledger:changed 信号自动重拉；此处仅刷新交易派生余额
    await refresh()
  } catch (e) {
    message.error(`删除失败: ${e}`)
  }
}

/** 删除走 useDialog 二次确认（与交易行菜单同语义）：取消不删，确认后才删除。 */
function confirmDelete(row: AccountBalance) {
  dialog.warning({
    title: '删除账户',
    content: `确认删除账户「${row.account.name}」？删除后相关交易仍保留。`,
    positiveText: '删除',
    negativeText: '取消',
    onPositiveClick: () => remove(row.account.id),
  })
}

// ---------------------------------------------------------------------------
// 编辑账户弹窗（name + currency_code；type 不可改——参与余额符号归属；
// initial_balance_cents 归「调整余额」管，两处不同改同一字段）
// ---------------------------------------------------------------------------

const showEdit = ref(false)
const editSeq = ref(0)
const editRow = ref<AccountBalance | null>(null)
const editName = ref('')
const editCurrency = ref('')

function openEdit(row: AccountBalance) {
  editRow.value = row
  editName.value = row.account.name
  editCurrency.value = row.account.currency_code
  editSeq.value += 1
  showEdit.value = true
}

async function submitEdit() {
  if (!editRow.value) return
  if (!editName.value.trim()) {
    message.warning('请输入账户名称')
    return
  }
  try {
    await api.updateAccount(editRow.value.account.id, {
      name: editName.value,
      currency_code: editCurrency.value,
    })
    message.success('已保存')
    showEdit.value = false
    // 参考数据由 ledger:changed 信号自动重拉；此处仅刷新余额
    await refresh()
  } catch (e) {
    message.error(`保存失败: ${e}`)
  }
}

// ---------------------------------------------------------------------------
// 调整余额弹窗（ADR-0026）：校准到目标值，后端生成一笔与黑洞账户的转账
// （Δ>0 从「无」转入、Δ<0 转出至「无」，删除该转账即撤销调整）
// ---------------------------------------------------------------------------

const showAdjust = ref(false)
const adjustSeq = ref(0)
const adjustRow = ref<AccountBalance | null>(null)
const adjustTarget = ref<number | null>(null)
const adjustDate = ref<number | null>(Date.now())

function openAdjust(row: AccountBalance) {
  adjustRow.value = row
  adjustTarget.value = null
  adjustDate.value = Date.now()
  adjustSeq.value += 1
  showAdjust.value = true
}

function todayIso(): string {
  return formatLocalDate(new Date())
}

/** 本地时区日期 → YYYY-MM-DD（不用 toISOString：避免时区偏移使日期漂移一天）。 */
function formatLocalDate(d: Date): string {
  const m = `${d.getMonth() + 1}`.padStart(2, '0')
  const day = `${d.getDate()}`.padStart(2, '0')
  return `${d.getFullYear()}-${m}-${day}`
}

/** 目标余额（分）：输入以元为单位，存储为整数分。 */
const adjustTargetCents = computed(() =>
  adjustTarget.value === null ? null : Math.round(adjustTarget.value * 100),
)

/** 差额 Δ = 目标 − 当前：>0 从黑洞转入，<0 转出至黑洞，=0 无需调整。 */
const adjustDelta = computed(() => {
  if (adjustRow.value === null || adjustTargetCents.value === null) return null
  return adjustTargetCents.value - adjustRow.value.balance_cents
})

const adjustCurrency = computed(() =>
  adjustRow.value
    ? reference.getCurrency(adjustRow.value.account.currency_code)
    : undefined,
)

const adjustDeltaText = computed(() => {
  if (adjustDelta.value === null || adjustDelta.value === 0) return ''
  const abs = formatAmount(Math.abs(adjustDelta.value), adjustCurrency.value)
  return adjustDelta.value > 0 ? `将从「无」转入 ${abs}` : `将转出 ${abs} 至「无」`
})

async function submitAdjust() {
  if (!adjustRow.value) return
  if (adjustTargetCents.value === null || adjustDelta.value === 0) return
  try {
    await api.adjustAccountBalance(adjustRow.value.account.id, {
      target_balance_cents: adjustTargetCents.value,
      date: adjustDate.value ? formatLocalDate(new Date(adjustDate.value)) : todayIso(),
    })
    message.success('余额已调整')
    showAdjust.value = false
    // 参考数据由 ledger:changed 信号自动重拉（若按需新建了黑洞账户）；此处仅刷新余额
    await refresh()
  } catch (e) {
    message.error(`调整失败: ${e}`)
  }
}

// ---------------------------------------------------------------------------
// 行菜单（编辑 / 调整余额 / 删除）：操作列「⋯」按钮 + 行右键共用同一 options
// （buildAccountRowMenuOptions 纯函数），删除项着主题 error 色
// ---------------------------------------------------------------------------

const menuOptions = computed(() =>
  buildAccountRowMenuOptions({ errorColor: themeVars.value.errorColor }),
)

const menuShow = ref(false)
const menuX = ref(0)
const menuY = ref(0)
const menuRow = ref<AccountBalance | null>(null)

/** 行右键弹出菜单：先收起再 nextTick 展开，保证换行弹出时位置刷新。 */
function showRowMenu(e: MouseEvent, row: AccountBalance) {
  e.preventDefault()
  menuRow.value = row
  menuX.value = e.clientX
  menuY.value = e.clientY
  menuShow.value = false
  void nextTick(() => {
    menuShow.value = true
  })
}

function onMenuSelect(key: string) {
  menuShow.value = false
  const row = menuRow.value
  if (!row) return
  if (key === 'edit') openEdit(row)
  else if (key === 'adjust-balance') openAdjust(row)
  else if (key === 'delete') confirmDelete(row)
}

/** 表格行属性：绑定行右键菜单。 */
const rowProps = (row: AccountBalance) => ({
  onContextmenu: (e: MouseEvent) => showRowMenu(e, row),
})

const columns: DataTableColumns<AccountBalance> = [
  {
    title: '名称',
    key: 'account.name',
    // 账户名下钻：点击跳转交易页并按涉及账户过滤（issue #97）
    render: (row) => h(AccountLink, { accountId: row.account.id }),
  },
  {
    title: '类型',
    key: 'account.type',
    render: (row) => ACCOUNT_TYPE_LABELS[row.account.type],
  },
  { title: '币种', key: 'account.currency_code' },
  {
    title: '余额',
    key: 'balance_cents',
    render: (row) => formatAmount(row.balance_cents, reference.getCurrency(row.account.currency_code)),
  },
  {
    title: '操作',
    key: 'actions',
    width: 64,
    // 「⋯」按钮与行右键共用同一手动定位菜单（showRowMenu 以点击坐标弹出）
    render: (row) =>
      h(
        NButton,
        {
          size: 'tiny',
          quaternary: true,
          'aria-label': '更多操作',
          onClick: (e: MouseEvent) => showRowMenu(e, row),
        },
        () => '⋯',
      ),
  },
]

onMounted(() => {
  // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll
  void refresh()
})
</script>

<template>
  <NSpace vertical :size="16">
    <NCard title="新增账户" size="small">
      <NForm label-placement="left" :show-feedback="false" inline size="small">
        <NFormItem label="名称">
          <NInput v-model:value="name" placeholder="账户名称" style="width: 160px" />
        </NFormItem>
        <NFormItem label="类型">
          <NSelect v-model:value="type" :options="typeOptions" style="width: 120px" />
        </NFormItem>
        <NFormItem label="币种">
          <NSelect v-model:value="currencyCode" :options="currencyOptions()" style="width: 140px" />
        </NFormItem>
        <NFormItem label="初始余额">
          <NInputNumber v-model:value="initial" :precision="2" style="width: 140px" />
        </NFormItem>
        <NButton type="primary" @click="create">添加</NButton>
      </NForm>
    </NCard>

    <NCard title="账户列表" size="small">
      <NDataTable
        :columns="columns"
        :data="balances"
        :bordered="false"
        size="small"
        :row-props="rowProps"
      />
    </NCard>

    <!-- 编辑账户弹窗：type 不可改（参与余额符号归属），币种仅无交易账户可改（后端校验） -->
    <NModal
      v-model:show="showEdit"
      title="编辑账户"
      preset="card"
      display-directive="if"
      style="width: 420px"
      :bordered="false"
    >
      <NForm
        v-if="editRow"
        :key="editSeq"
        label-placement="left"
        :show-feedback="false"
        size="small"
      >
        <NFormItem label="名称">
          <NInput v-model:value="editName" placeholder="账户名称" />
        </NFormItem>
        <NFormItem label="类型">
          <NInput :value="ACCOUNT_TYPE_LABELS[editRow.account.type]" disabled />
        </NFormItem>
        <NFormItem label="币种">
          <NSelect v-model:value="editCurrency" :options="currencyOptions()" style="width: 100%" />
        </NFormItem>
        <NSpace justify="end" :size="8">
          <NButton size="small" @click="showEdit = false">取消</NButton>
          <NButton size="small" type="primary" @click="submitEdit">保存</NButton>
        </NSpace>
      </NForm>
    </NModal>

    <!-- 调整余额弹窗：输入目标余额，实时显示差额与去向；日期默认今天可改（对账常补记） -->
    <NModal
      v-model:show="showAdjust"
      title="调整余额"
      preset="card"
      display-directive="if"
      style="width: 420px"
      :bordered="false"
    >
      <NForm
        v-if="adjustRow"
        :key="adjustSeq"
        label-placement="left"
        :show-feedback="false"
        size="small"
      >
        <NFormItem label="当前余额">
          <NText>{{
            formatAmount(adjustRow.balance_cents, adjustCurrency)
          }}</NText>
        </NFormItem>
        <NFormItem label="目标余额">
          <NInputNumber
            v-model:value="adjustTarget"
            :precision="2"
            placeholder="对账后应有的余额"
            style="width: 100%"
          />
        </NFormItem>
        <NFormItem label="调整日期">
          <NDatePicker v-model:value="adjustDate" type="date" style="width: 100%" />
        </NFormItem>
        <NFormItem label="差额" :show-label="adjustDeltaText === ''">
          <NText v-if="adjustDelta === 0">余额已等于目标值，无需调整</NText>
          <NText v-else-if="adjustDeltaText" :type="adjustDelta! > 0 ? 'success' : 'warning'">
            {{ adjustDeltaText }}（生成一笔与「无」的转账，删除该转账即撤销调整）
          </NText>
        </NFormItem>
        <NSpace justify="end" :size="8">
          <NButton size="small" @click="showAdjust = false">取消</NButton>
          <NButton
            size="small"
            type="primary"
            :disabled="adjustTargetCents === null || adjustDelta === 0"
            @click="submitAdjust"
          >
            确认调整
          </NButton>
        </NSpace>
      </NForm>
    </NModal>

    <!-- 行菜单（操作列「⋯」与行右键共用）：手动定位弹出 -->
    <NDropdown
      trigger="manual"
      placement="bottom-start"
      :show="menuShow"
      :x="menuX"
      :y="menuY"
      :options="menuOptions"
      :min-width="140"
      @select="onMenuSelect"
      @clickoutside="menuShow = false"
    />
  </NSpace>
</template>
