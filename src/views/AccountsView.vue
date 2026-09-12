<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { yuanToCents, formatAmount } from '@/utils/money'
import { computed, h, onMounted, ref } from 'vue'
import {
  NCard,
  NButton,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NSpace,
  NText,
  useMessage,
  useThemeVars,
  type DataTableColumns,
} from 'naive-ui'
import { api } from '@/api'
import { t } from '@ledger/i18n'
import { useReferenceStore } from '@/stores/reference'
import AppModal from '@/components/AppModal.vue'
import AppDropdown from '@/components/AppDropdown.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import AppSelect from '@/components/AppSelect.vue'
import { useAppDialog } from '@/composables/useAppDialog'
import { useModalIntent } from '@/composables/useModalIntent'
import { useRowContextMenu } from '@/composables/useRowContextMenu'
import { useWindowTier } from '@/composables/useWindowTier'
import AccountLink from '@/components/AccountLink.vue'
import { buildAccountRowMenuOptions } from '@/components/account-row-menu'
import { ACCOUNT_TYPES } from '@ledger/types'
import type { AccountBalance, AccountInput, AccountType } from '@ledger/types'

const reference = useReferenceStore()
const message = useMessage()
const dialog = useAppDialog()
const themeVars = useThemeVars()
const balances = ref<AccountBalance[]>([])

// 移动档适配（issue #847 / ADR-0088 决策 11 票⑦）：账户列表三分列 + 新增表单
// 纵向堆叠 + 「⋯」48px 触控目标。断点口径接窗口分级 composable 唯一事实源，
// 不自立断点；桌面档列结构与表单布局一字不动（回归红线）。
// composable 在 setup 顶层调用一次（监听注册与 onScopeDispose 注销归其内聚），
// 档位派生只读返回值。
const windowTier = useWindowTier()
const isMobileTier = computed(() => windowTier.value === 'mobile')

const name = ref('')
const type = ref<AccountType>('cash')
const currencyCode = ref('CNY')
const initial = ref<number | null>(0)

// computed：标签经 t() 随界面语言即时切换（ADR-0049）
const typeOptions = computed(() =>
  ACCOUNT_TYPES.map((k) => ({
    label: t(`accounts.type.${k}`),
    value: k,
  })),
)
const currencyOptions = () =>
  reference.currencies.map((c) => ({ label: `${c.name} (${c.code})`, value: c.code }))

async function refresh() {
  balances.value = await api.listAccountBalances()
}

async function create() {
  if (!name.value.trim()) {
    message.warning(t('accounts.message.nameRequired'))
    return
  }
  const input: AccountInput = {
    name: name.value,
    type: type.value,
    currency_code: currencyCode.value,
    initial_balance_cents: yuanToCents(initial.value ?? 0) ?? 0,
  }
  try {
    await api.createAccount(input)
    message.success(t('accounts.message.created'))
    name.value = ''
    initial.value = 0
    // 参考数据由 ledger:changed 信号自动重拉；此处仅刷新交易派生余额
    await refresh()
  } catch (e) {
    message.error(t('accounts.message.createFailed', { message: errorMessage(e) }))
  }
}

async function remove(id: string) {
  try {
    await api.deleteAccount(id)
    message.success(t('accounts.message.deleted'))
    // 参考数据由 ledger:changed 信号自动重拉；此处仅刷新交易派生余额
    await refresh()
  } catch (e) {
    message.error(t('accounts.message.deleteFailed', { message: errorMessage(e) }))
  }
}

/** 删除走 useAppDialog 二次确认（与交易行菜单同语义）：取消不删，确认后才删除。
 * 遮罩点击不构成关闭意图（issue #252 弹层关闭语义）：确认/取消须显式点击。 */
function confirmDelete(row: AccountBalance) {
  dialog.warning({
    title: t('accounts.deleteDialog.title'),
    content: t('accounts.deleteDialog.content', { name: row.account.name }),
    positiveText: t('accounts.deleteDialog.positive'),
    negativeText: t('accounts.deleteDialog.negative'),
    maskClosable: false,
    onPositiveClick: () => remove(row.account.id),
  })
}

// ---------------------------------------------------------------------------
// 编辑账户弹窗（name + currency_code；type 不可改——参与余额符号归属；
// initial_balance_cents 归「调整余额」管，两处不同改同一字段）
// 开启/目标/关闭编排归弹窗意图工厂 ModalIntent（ADR-0072）：意图闭集单成员
// （携带目标账户行），显示由「意图非空」派生、序号随开启递增驱动表单重建、
// 关闭清回 null 终态。现状已带序号守卫（序号驱动表单重建），迁移为纯方言
// 替换：行为完全等价，无缺陷修复。
// ---------------------------------------------------------------------------

/** 编辑账户弹窗意图（单成员闭集）：携带目标账户行。 */
interface AccountEditIntent {
  row: AccountBalance
}

const {
  intent: editIntent,
  seq: editSeq,
  open: openEditIntent,
  close: closeEdit,
} = useModalIntent<AccountEditIntent>()

const editName = ref('')
const editCurrency = ref('')

function openEdit(row: AccountBalance) {
  editName.value = row.account.name
  editCurrency.value = row.account.currency_code
  openEditIntent({ row })
}

async function submitEdit() {
  if (!editIntent.value) return
  if (!editName.value.trim()) {
    message.warning(t('accounts.message.nameRequired'))
    return
  }
  try {
    await api.updateAccount(editIntent.value.row.account.id, {
      name: editName.value,
      currency_code: editCurrency.value,
    })
    message.success(t('accounts.message.saved'))
    closeEdit()
    // 参考数据由 ledger:changed 信号自动重拉；此处仅刷新余额
    await refresh()
  } catch (e) {
    message.error(t('accounts.message.saveFailed', { message: errorMessage(e) }))
  }
}

// ---------------------------------------------------------------------------
// 调整余额弹窗（ADR-0026）：校准到目标值，后端生成一笔与黑洞账户的转账
// （Δ>0 从「无」转入、Δ<0 转出至「无」，删除该转账即撤销调整）
// 开启/目标/关闭编排归弹窗意图工厂 ModalIntent（ADR-0072）：意图闭集单成员
// （携带目标账户行），显示由「意图非空」派生、序号随开启递增驱动表单重建、
// 关闭清回 null 终态。现状已带序号守卫（序号驱动表单重建），迁移为纯方言
// 替换：行为完全等价，无缺陷修复。
// ---------------------------------------------------------------------------

/** 调整余额弹窗意图（单成员闭集）：携带目标账户行。 */
interface AccountAdjustIntent {
  row: AccountBalance
}

const {
  intent: adjustIntent,
  seq: adjustSeq,
  open: openAdjustIntent,
  close: closeAdjust,
} = useModalIntent<AccountAdjustIntent>()

const adjustTarget = ref<number | null>(null)
const adjustDate = ref<number | null>(Date.now())

function openAdjust(row: AccountBalance) {
  adjustTarget.value = null
  adjustDate.value = Date.now()
  openAdjustIntent({ row })
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

/** 目标余额（分）：输入以元为单位，经 yuanToCents 统一口径转整数分（非法输入 → null，禁用提交）。 */
const adjustTargetCents = computed(() =>
  adjustTarget.value === null ? null : yuanToCents(adjustTarget.value),
)

/** 差额 Δ = 目标 − 当前：>0 从黑洞转入，<0 转出至黑洞，=0 无需调整。 */
const adjustDelta = computed(() => {
  if (adjustIntent.value === null || adjustTargetCents.value === null) return null
  return adjustTargetCents.value - adjustIntent.value.row.balance_cents
})

const adjustCurrency = computed(() =>
  adjustIntent.value
    ? reference.getCurrency(adjustIntent.value.row.account.currency_code)
    : undefined,
)

const adjustDeltaText = computed(() => {
  if (adjustDelta.value === null || adjustDelta.value === 0) return ''
  const abs = formatAmount(Math.abs(adjustDelta.value), adjustCurrency.value)
  return adjustDelta.value > 0
    ? t('accounts.adjust.deltaIn', { amount: abs })
    : t('accounts.adjust.deltaOut', { amount: abs })
})

async function submitAdjust() {
  if (!adjustIntent.value) return
  if (adjustTargetCents.value === null || adjustDelta.value === 0) return
  try {
    await api.adjustAccountBalance(adjustIntent.value.row.account.id, {
      target_balance_cents: adjustTargetCents.value,
      date: adjustDate.value ? formatLocalDate(new Date(adjustDate.value)) : todayIso(),
    })
    message.success(t('accounts.message.adjusted'))
    closeAdjust()
    // 参考数据由 ledger:changed 信号自动重拉（若按需新建了黑洞账户）；此处仅刷新余额
    await refresh()
  } catch (e) {
    message.error(t('accounts.message.adjustFailed', { message: errorMessage(e) }))
  }
}

// ---------------------------------------------------------------------------
// 行菜单（编辑 / 调整余额 / 删除）：操作列「⋯」按钮 + 行右键两入口共用同一
// options（buildAccountRowMenuOptions 纯函数，删除项着主题 error 色）与同一
// open 入口。打开、重定位、关闭、选中的全部时序收进行菜单编排工厂
// RowContextMenu（issue #550 / #551，ADR-0077）：业务动作分派留视图（工厂
// 入参回调，选中即收起并交付收起瞬间的目标行）。
// ---------------------------------------------------------------------------

const menuOptions = computed(() =>
  buildAccountRowMenuOptions({ errorColor: themeVars.value.errorColor }),
)

const rowMenu = useRowContextMenu<AccountBalance>((key, row) => {
  if (key === 'edit') openEdit(row)
  else if (key === 'adjust-balance') openAdjust(row)
  else if (key === 'delete') confirmDelete(row)
})

// 可见性由单判别状态派生（非空即显示）；定位坐标取工厂保留值（open 同步更新、
// close 不清零）：naive-ui 离场动画期间仍按 x/y 重定位弹层，视图侧清零会让
// 淡出中的菜单跳到视口左上角闪现一次（issue #798）。
const menuShow = computed(() => rowMenu.state.value !== null)
const menuX = computed(() => rowMenu.position.value.x)
const menuY = computed(() => rowMenu.position.value.y)

/** 表格行属性：绑定行右键菜单（open 内化「收起 → 下一帧重开」重定位舞步；
 * 原生菜单拦截单点归窗口行为守卫，视图不再 preventDefault）。 */
const rowProps = (row: AccountBalance) => ({
  onContextmenu: (e: MouseEvent) => rowMenu.open(e, row),
})

/** 余额单元格渲染（移动/桌面两分支共用，格式化接缝 formatAmount 单点含隐私掩码）。 */
const renderBalanceCell = (row: AccountBalance) =>
  formatAmount(row.balance_cents, reference.getCurrency(row.account.currency_code))

/** 移动档名称单元格布局：名称与「类型 · 币种」副行纵排；副行弱化小字（内联样式收口
 * 在列配置单点，同 transaction-columns 渲染函数先例）。 */
const MOBILE_NAME_CELL_STYLE = 'display: flex; flex-direction: column; gap: 2px; min-width: 0;'
const MOBILE_NAME_SUB_STYLE = 'font-size: 12px; opacity: 0.65;'
/** 移动档名称链接：换行不截断（悬停替代原则「空间够则常驻」，触屏无悬停全文）、
 * 文本左对齐（button 拉满单元格宽后默认居中会与桌面行错位，交易列先例）。 */
const MOBILE_NAME_LINK_STYLE = 'white-space: normal; text-align: left;'
/** 移动档「⋯」按钮：显式 48×48 触控目标（ADR-0088 全局验收基线；按钮自身达标，
 * 不用伪元素外扩——操作列内相邻行的热区互不侵入）。桌面档不挂，尺寸零变化。 */
const MOBILE_MORE_BUTTON_STYLE = { width: '48px', height: '48px', fontSize: '18px' }

const columns = computed<DataTableColumns<AccountBalance>>(() => {
  /** 操作列：「⋯」与行右键共用同一工厂 open 入口（以点击坐标弹出）；两轴同一
   * 列配置（入口全平台常显），仅移动档加大触控目标。 */
  const actionsColumn: DataTableColumns<AccountBalance>[number] = {
    title: t('accounts.list.colActions'),
    key: 'actions',
    width: 64,
    render: (row) =>
      h(
        NButton,
        {
          size: 'tiny',
          quaternary: true,
          'aria-label': t('accounts.list.moreActions'),
          style: isMobileTier.value ? MOBILE_MORE_BUTTON_STYLE : undefined,
          onClick: (e: MouseEvent) => rowMenu.open(e, row),
        },
        () => '⋯',
      ),
  }

  // 移动档三分列（issue #847）：名称（类型/币种并入副行）、余额、操作——328px
  // 内容宽内无横向滚动、逐行可读；桌面档五列一字不动（回归红线）。
  if (isMobileTier.value) {
    return [
      {
        title: t('accounts.list.colName'),
        key: 'account.name',
        // 名称下钻：点击跳转交易页并按涉及账户过滤（issue #97）；副行携带类型与币种
        render: (row) =>
          h('div', { style: MOBILE_NAME_CELL_STYLE }, [
            h(AccountLink, {
              accountId: row.account.id,
              style: MOBILE_NAME_LINK_STYLE,
            }),
            h(
              'div',
              { style: MOBILE_NAME_SUB_STYLE },
              `${t(`accounts.type.${row.account.type}`)} · ${row.account.currency_code}`,
            ),
          ]),
      },
      {
        title: t('accounts.list.colBalance'),
        key: 'balance_cents',
        width: 110,
        render: renderBalanceCell,
      },
      actionsColumn,
    ]
  }

  return [
    {
      title: t('accounts.list.colName'),
      key: 'account.name',
      // 账户名下钻：点击跳转交易页并按涉及账户过滤（issue #97）
      render: (row) => h(AccountLink, { accountId: row.account.id }),
    },
    {
      title: t('accounts.list.colType'),
      key: 'account.type',
      render: (row) => t(`accounts.type.${row.account.type}`),
    },
    { title: t('accounts.list.colCurrency'), key: 'account.currency_code' },
    {
      title: t('accounts.list.colBalance'),
      key: 'balance_cents',
      render: renderBalanceCell,
    },
    actionsColumn,
  ]
})

onMounted(() => {
  // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll
  void refresh()
})
</script>

<template>
  <NSpace vertical :size="16">
    <!-- 新增账户表单按窗口分级分档（issue #847）：桌面档行内横排（既有布局一字
         不动）；移动档纵排堆叠（标签上置 + 控件满宽，内联横排 ≈700px 在 360dp
         屏必横向溢出），行距取 ADR-0079 决策 4 的 12px 节奏值（show-feedback 关
         闭后表单项零间距，容器 gap 单点补齐；页面级表单不在弹窗节奏守门范围）。
         「添加」主操作热区扩至 ≥48px（视觉不变）。 -->
    <NCard :title="t('accounts.create.title')" size="small">
      <NForm
        :inline="!isMobileTier"
        :label-placement="isMobileTier ? 'top' : 'left'"
        :show-feedback="false"
        size="small"
        :style="
          isMobileTier
            ? { display: 'flex', flexDirection: 'column', gap: '12px' }
            : undefined
        "
      >
        <NFormItem :label="t('accounts.create.name')">
          <NInput
            v-model:value="name"
            :placeholder="t('accounts.create.namePlaceholder')"
            :style="isMobileTier ? { width: '100%' } : { width: '160px' }"
          />
        </NFormItem>
        <NFormItem :label="t('accounts.create.type')">
          <AppSelect
            v-model:value="type"
            :options="typeOptions"
            :style="isMobileTier ? { width: '100%' } : { width: '120px' }"
          />
        </NFormItem>
        <NFormItem :label="t('accounts.create.currency')">
          <AppSelect
            v-model:value="currencyCode"
            :options="currencyOptions()"
            :style="isMobileTier ? { width: '100%' } : { width: '140px' }"
          />
        </NFormItem>
        <NFormItem :label="t('accounts.create.initialBalance')">
          <NInputNumber
            v-model:value="initial"
            :precision="2"
            :style="isMobileTier ? { width: '100%' } : { width: '140px' }"
          />
        </NFormItem>
        <NButton
          type="primary"
          :class="isMobileTier ? 'touch-hit-area' : undefined"
          :style="isMobileTier ? { '--touch-hit-inset': '-10px -14px' } : undefined"
          @click="create"
        >
          {{ t('accounts.create.add') }}
        </NButton>
      </NForm>
    </NCard>

    <NCard :title="t('accounts.list.title')" size="small">
      <NDataTable
        :columns="columns"
        :data="balances"
        :bordered="false"
        size="small"
        :row-props="rowProps"
      />
    </NCard>

    <!-- 编辑账户弹窗：type 不可改（参与余额符号归属），币种仅无交易账户可改（后端校验）。
         显示由「意图非空」派生（无独立 show 布尔），关闭（✕ / ESC / 取消 / 提交成功）
         统一经工厂清回 null 终态；序号作表单 key 强制重建（ADR-0072）。 -->
    <AppModal
      :show="editIntent !== null"
      :title="t('accounts.edit.title')"
      preset="card"
      display-directive="if"
      card-size="sm"
      @update:show="(show: boolean) => { if (!show) closeEdit() }"
    >
      <NForm
        v-if="editIntent"
        :key="editSeq"
        label-placement="left"
        :show-feedback="false"
        size="small"
      >
        <!-- 行距节奏容器：NFormItem 默认零行距，表单项与按钮行同包（ADR-0079 决策 4 / issue #804） -->
        <NSpace vertical :size="12">
          <NFormItem :label="t('accounts.create.name')">
            <NInput
              v-model:value="editName"
              :placeholder="t('accounts.create.namePlaceholder')"
            />
          </NFormItem>
          <NFormItem :label="t('accounts.create.type')">
            <NInput :value="t(`accounts.type.${editIntent.row.account.type}`)" disabled />
          </NFormItem>
          <NFormItem :label="t('accounts.create.currency')">
            <AppSelect v-model:value="editCurrency" :options="currencyOptions()" style="width: 100%" />
          </NFormItem>
          <NSpace justify="end" :size="8">
            <NButton @click="closeEdit">{{ t('accounts.edit.cancel') }}</NButton>
            <NButton type="primary" @click="submitEdit">{{ t('accounts.edit.save') }}</NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 调整余额弹窗：输入目标余额，实时显示差额与去向；日期默认今天可改（对账常补记）。
         显示由「意图非空」派生（无独立 show 布尔），关闭（✕ / ESC / 取消 / 提交成功）
         统一经工厂清回 null 终态；序号作表单 key 强制重建（ADR-0072）。 -->
    <AppModal
      :show="adjustIntent !== null"
      :title="t('accounts.adjust.title')"
      preset="card"
      display-directive="if"
      card-size="sm"
      @update:show="(show: boolean) => { if (!show) closeAdjust() }"
    >
      <NForm
        v-if="adjustIntent"
        :key="adjustSeq"
        label-placement="left"
        :show-feedback="false"
        size="small"
      >
        <!-- 行距节奏容器：NFormItem 默认零行距，表单项与按钮行同包（ADR-0079 决策 4 / issue #804） -->
        <NSpace vertical :size="12">
          <NFormItem :label="t('accounts.adjust.currentBalance')">
            <NText>{{
              formatAmount(adjustIntent.row.balance_cents, adjustCurrency)
            }}</NText>
          </NFormItem>
          <NFormItem :label="t('accounts.adjust.targetBalance')">
            <NInputNumber
              v-model:value="adjustTarget"
              :precision="2"
              :placeholder="t('accounts.adjust.targetPlaceholder')"
              style="width: 100%"
            />
          </NFormItem>
          <NFormItem :label="t('accounts.adjust.date')">
            <AppDatePicker v-model:value="adjustDate" type="date" style="width: 100%" />
          </NFormItem>
          <NFormItem :label="t('accounts.adjust.delta')" :show-label="adjustDeltaText === ''">
            <NText v-if="adjustDelta === 0">{{ t('accounts.adjust.deltaZero') }}</NText>
            <NText v-else-if="adjustDeltaText" :type="adjustDelta! > 0 ? 'success' : 'warning'">
              {{ adjustDeltaText }}{{ t('accounts.adjust.hint') }}
            </NText>
          </NFormItem>
          <NSpace justify="end" :size="8">
            <NButton @click="closeAdjust">{{ t('accounts.edit.cancel') }}</NButton>
            <NButton
              type="primary"
              :disabled="adjustTargetCents === null || adjustDelta === 0"
              @click="submitAdjust"
            >
              {{ t('accounts.adjust.confirm') }}
            </NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 行菜单（操作列「⋯」与行右键共用）：手动定位弹出；开合上报经薄封装
         attrs watch 自动生效（`:show` 绑定照旧） -->
    <AppDropdown
      trigger="manual"
      placement="bottom-start"
      :show="menuShow"
      :x="menuX"
      :y="menuY"
      :options="menuOptions"
      style="max-width: 140px"
      @select="rowMenu.select"
      @clickoutside="rowMenu.close"
    />
  </NSpace>
</template>
