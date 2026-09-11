// 交易列配置共享模块（Issue 39 Prefactor）：
// 交易列表与搜索视图复用同一列配置（日期/类型/分类/账户/备注/金额）。
// 渲染函数在运行时读取 store 的响应式数据，构建一次即可，无需 computed 包裹。

import { h, type VNode } from 'vue'
import { NEllipsis, NButton, NTag, type DataTableColumn } from 'naive-ui'
import { formatAmount } from '@/types'
import type { Transaction, TransactionKind } from '@/types'
import type { useReferenceStore } from '@/stores/reference'
import { useAppStore } from '@/stores/app'
import { kindSemanticColor } from '@/theme/semantic-colors'
import { t } from '@/i18n'
import AccountLink from '@/components/AccountLink.vue'
import MerchantLink from '@/components/MerchantLink.vue'
import SourceLink from '@/components/SourceLink.vue'
import NoteCopyButton from '@/components/NoteCopyButton.vue'
import AmountCell from '@/components/AmountCell.vue'
import { lendingLabelKey, resolveLendingDirection } from '@/domain/lending'

export type ReferenceStore = ReturnType<typeof useReferenceStore>

export const KIND_TAG_TYPE: Record<TransactionKind, 'success' | 'warning' | 'info' | 'default'> = {
  income: 'success',
  expense: 'warning',
  refund: 'info',
  transfer: 'default',
  buy: 'default',
  sell: 'default',
  // 基金转换（ADR-0099）取 info 蓝色标注：与退款同色型但标签文案不同，
  // 一眼区分于买入/卖出的中性标签（投资类默认色）。
  convert: 'info',
  // 份额调整（ADR-0106 / #1049）：与转换同为「无现金腿」kind，同取 info 蓝色标注，
  // 标签文案区分二者。
  split: 'info',
}

/**
 * 列表/卡片金额展示口径单点：基金转换行展示**转出金额**（确认单权威，ADR-0099），
 * 份额调整行无现金腿、无金额（ADR-0106）返回 `null`（空值口径，渲染 '-'），
 * 其余行展示行金额锚点（本位币口径）。
 *
 * 转换行的行金额锚点是服务端按 FIFO 消耗算出的**结转成本**（不是用户看到的转出金额），
 * 故展示必须走扩展字段；扩展缺失（旧数据/直读快照）时回退行金额，不抛错、不显空。
 * 表格金额列与移动卡片共用 `displayAmountText`，两处各自分支即口径漂移。
 */
export function displayAmountCents(row: Transaction): number | null {
  if (row.kind === 'convert' && row.convert) return row.convert.out_amount_cents
  // 份额调整（ADR-0106 决策 1）：无现金腿、无金额——按空值语义返回 null，
  // 不以 0 伪装「已知为零」（同持仓缺价行的 '-' 口径）。
  if (row.kind === 'split') return null
  return row.amount_native_cents
}

/** 金额展示文案单点（表格金额列与移动卡片共用）：空值口径（无现金腿的 split 无金额）
 * 渲染 '-'，其余经 `formatAmount`（含金额隐私模式与数字分组）。 */
export function displayAmountText(reference: ReferenceStore, row: Transaction): string {
  const cents = displayAmountCents(row)
  return cents === null ? '-' : formatAmount(cents, reference.getCurrency(row.currency_code))
}

/** 交易基础列：日期/类型/分类/账户/备注/金额（搜索结果与交易列表共用，只读）。
 * 列名经 t() 取当前语言：使用方以 computed 构造列数组（TransactionsView/SearchView），
 * 语言切换时重建列，表头即时更新。
 *
 * 列宽约定（Naive UI DataTable，headless Chrome 实测验证）：
 * - `ellipsis` 令 table-layout 强制为 fixed；fixed 布局下**未指定 `width` 的列均分剩余空间**，
 *   `minWidth`/`maxWidth` 均无效（maxWidth 仅 `resizable` 时生效）。
 * - 策略：除备注外所有列显式 `width`（贴合实际内容，不随窗口漂移）；**备注列不设 `width`，
 *   作为唯一弹性列吸收剩余空间**——窗口更宽则备注更宽、更窄则备注收缩，表格始终铺满容器，
 *   其余列不被挤压也不被拉伸。备注超长时由单元格内 NEllipsis 省略 + 悬停全文
 *   （复制按钮并排，见 renderNoteCell）。
 * - 不要覆盖 table 的 `width`（改 `auto` 会让带 `ellipsis` 的列被长文本撑宽，实测分类
 *   150→286px、备注 240→398px）。
 * - 使用方以「所有固定列（有 `width` 的列，含金额列；备注不计入）宽度总和」作为 `scroll-x`，
 *   作为窄窗口下的横向滚动下限。备注为弹性列，窗口变窄时先由备注收缩吸收，各固定列宽保持恒定——
 *   只有当内容区窄于固定列宽总和时才出现横向滚动（固定列总和 965，含来源列 140，窄窗口可能触发，
 *   由 scroll-x 提供横向滚动底线）。
 * - 宽度按实际内容估算：日期 105 / 类型 65 / 分类 150（最长路径 ≈149px）/ 商户 120 / 账户 180（转账行需容纳「转出 → 转入」两个账户名 + 箭头，长名由链接自身省略号兜底）/ 来源 140（图标 + 实体名 + 状态标注，spec #704）/ 金额 125。固定列总和 965。 */
/** 类型标签（issue #374）：借贷是 transfer 的派生视角——两端账户类型构成借贷
 * （receivable/debt）的转账显示借出/收回/借入/还款专属文案，普通转账仍显示「转账」；
 * 非 transfer kind 不参与派生、按自身 kind 标签。历史数据实时派生、无数据迁移。
 * 方向识别收口 domain 层借贷模块（与表单分派/回填共用同一函数），
 * 标签随账户映射响应式更新（同 categoryPath 的响应式纪律）。
 * 导出面（issue #846）：移动档卡片列表消费同一派生，与表格类型列单源同文案。 */
export function kindLabel(reference: ReferenceStore, row: Transaction): string {
  if (row.kind !== 'transfer') return t(`transactions.kind.${row.kind}`)
  const direction = resolveLendingDirection(row, (id) => reference.accountMap.get(id)?.type)
  return t(lendingLabelKey(direction ?? 'none'))
}

/** 备注单元格布局：文本占满剩余宽度（自省略），复制按钮固定宽度靠右。 */
const NOTE_CELL_STYLE =
  'display: flex; align-items: center; gap: 2px; width: 100%; max-width: 100%;'

/** 备注单元格渲染（显式复制通道，见 CONTEXT-ui-interaction「界面文本不可选」）：
 * - 无备注渲染 '-'，不渲染复制按钮（空备注无可复制）；
 * - 有备注：单元格内 flex——NEllipsis 承载文本（自省略 + 悬停全文，同账户/来源列的
 *   单元格内省略模式），NoteCopyButton 复制完整备注（clipboard API + toast），
 *   按钮悬停行显现（显隐样式收口 global.css）。 */
function renderNoteCell(row: Transaction): VNode | string {
  const { note } = row
  if (!note) return '-'
  return h('div', { style: NOTE_CELL_STYLE }, [
    h(NEllipsis, { style: 'flex: 1 1 auto; min-width: 0;' }, { default: () => note }),
    h(NoteCopyButton, { note, style: 'flex: none;' }),
  ])
}

/** buildTransactionColumns 可选装配面：调用方按需声明，缺省即纯只读列（搜索结果同款）。 */
export interface BuildTransactionColumnsOptions {
  /** 交易行「⋯」常显按钮的打开回调（ADR-0088 决策 6，issue #843）：传入即
   * 追加常显操作列，与行右键共用 RowContextMenu 同一 open 入口（账户行先例）；
   * 不传则不渲染该列（搜索结果无行菜单，保持只读）。 */
  onRowMenuOpen?: (event: MouseEvent, row: Transaction) => void
}

export function buildTransactionColumns(
  reference: ReferenceStore,
  options: BuildTransactionColumnsOptions = {},
): DataTableColumn<Transaction>[] {
  const columns: DataTableColumn<Transaction>[] = [
    { title: t('transactions.columns.date'), key: 'date', width: 105 },
    {
      title: t('transactions.columns.kind'),
      key: 'kind',
      width: 65,
      render: (row) =>
        h(NTag, { type: KIND_TAG_TYPE[row.kind] }, () => kindLabel(reference, row)),
    },
    {
      title: t('transactions.columns.category'),
      key: 'category_id',
      width: 150,
      ellipsis: { tooltip: true },
      render: (row) => (row.category_id ? reference.categoryPath(row.category_id) || '-' : '-'),
    },
    {
      title: t('transactions.columns.merchant'),
      key: 'merchant_id',
      width: 120,
      ellipsis: { tooltip: true },
      // 商户名经 merchantMap（含软删）解析并可点击下钻（issue #191）；未知/无商户回退 '-'
      render: (row) =>
        row.merchant_id ? h(MerchantLink, { merchantId: row.merchant_id }) : '-',
    },
    {
      title: t('transactions.columns.account'),
      key: 'account_id',
      width: 180,
      render: (row) => renderAccountCell(row),
    },
    {
      title: t('transactions.columns.source'),
      key: 'source',
      width: 140,
      // 来源列（spec #704 / issue #706）：图标 + 实体名 + 状态标注，点击经来源
      // 跳转深模块落地（SourceLink 内部收口）；无来源留空（手动/AI 导入口径）。
      // 不设列级 ellipsis（账户列同款理由：NEllipsis 会把图标/名称/标注包装成
      // 整体省略，破坏链接自身省略与标注并排语义），超长由链接自身省略号兜底。
      render: (row) => (row.source ? h(SourceLink, { source: row.source }) : '-'),
    },
    {
      title: t('transactions.columns.note'),
      key: 'note',
      // 弹性列：不设 width，由 fixed 布局均分剩余空间（超长时省略号 + 悬停显示全文）；
      // 不设列级 ellipsis（账户/来源列同款理由：会把复制按钮一起包进省略容器），
      // 省略与悬停全文由单元格内 NEllipsis 承担（fixed 布局由分类/商户列维持）
      render: renderNoteCell,
    },
    {
      title: t('transactions.columns.amount'),
      key: 'amount_native_cents',
      width: 125,
      // 金额按交易类型语义色着色（issue #435）：色值单一来源在
      // @/theme/semantic-colors（六类型亮/暗两套）。主题在渲染时读取 app store
      // 响应式取值：切换外观主题即时换色，无需重建列；借出/借入/收回/还款是
      // transfer 的派生视角（ADR-0053），随 transfer 同紫，不做派生级区分。
      // 单元格交互归 AmountCell（issue #843）：指针轴纯 span 零变化，触控轴
      // 点按弹出全文（悬停一击可达；文案与色在此单点计算后传入）。
      render: (row) =>
        h(AmountCell, {
          text: displayAmountText(reference, row),
          color: kindSemanticColor(row.kind, useAppStore().theme),
        }),
    },
  ]
  // 交易行「⋯」常显列（ADR-0088 决策 6，issue #843）：与右键共用同一行菜单编排
  // open 入口、以点击坐标弹出，全平台常显（账户行先例，桌面可见变化已裁决）；
  // 仅声明了回调的调用方（交易列表）渲染，搜索结果不追加。
  if (options.onRowMenuOpen) {
    columns.push({
      title: t('transactions.columns.actions'),
      key: 'actions',
      width: 64,
      render: (row) =>
        h(
          NButton,
          {
            size: 'tiny',
            quaternary: true,
            class: 'row-actions-btn touch-hit-area',
            'aria-label': t('transactions.menu.actions'),
            onClick: (e: MouseEvent) => options.onRowMenuOpen!(e, row),
          },
          () => '⋯',
        ),
    })
  }
  return columns
}

/** 转账/出资账户行单元格内账户链接的布局样式：内容宽度 + 允许收缩省略 + 文本左对齐。
 * 经 attrs 透传到 AccountLink 根按钮，与组件内部强调色样式合并。
 * 用内容宽度（flex-grow:0）而非均分剩余宽度：单账户行「花呗」是内容宽度、自然靠左，
 * 双账户行首账户名若也均分半宽会因 <button> 默认 text-align:center 被水平居中、顶不到列左缘
 * （与上方单账户行错位）。内容宽度让首名紧贴列左缘、与单账户行对齐；
 * 收缩项仍由 min-width:0 允许收缩（长名省略号兜底、不溢出）。 */
const ACCOUNT_CELL_LINK_STYLE = 'flex: 0 1 auto; min-width: 0; text-align: left;'

/** 双账户单元格（转账「转出 → 转入」、出资 buy/sell「出资账户 → 投资账户」）公共渲染：
 * inline-flex 容器，两个链接内容宽度、箭头固定宽度，整组 justify-content:flex-start
 * 靠左；长账户名由链接自身 ellipsis（见 AccountLink）省略号兜底、不溢出（列宽 180 时收缩省略）。
 * 首账户名因此与单账户行（如「花呗」）左侧对齐；不设列级 ellipsis（fixed 布局由备注列的
 * ellipsis 维持），否则 NEllipsis 会把两个按钮包装成整体省略，破坏各自可点击语义。
 * 导出面（issue #846）：移动档卡片列表消费同一渲染，账户呈现两形态单源。 */
function renderTwoAccountCell(fromAccountId: string, toAccountId: string): VNode {
  return h(
    'div',
    {
      style:
        'display: inline-flex; align-items: center; justify-content: flex-start; gap: 4px; width: 100%; max-width: 100%;',
    },
    [
      h(AccountLink, { accountId: fromAccountId, style: ACCOUNT_CELL_LINK_STYLE }),
      h('span', { style: 'flex: none; opacity: 0.5;' }, '→'),
      h(AccountLink, { accountId: toAccountId, style: ACCOUNT_CELL_LINK_STYLE }),
    ],
  )
}

/** 账户单元格渲染（issue #99 / #937，方向修正 issue #1030）：
 * - 转账行显示「转出 → 转入」双向账户名（to_account_id 存在时），两个名字各自可点击、
 *   各自下钻到对应账户的过滤视图；
 * - 带出资账户的 buy/sell 行按「资金流出方在前」显示双向账户名（ADR-0096 决策 6：
 *   buy「出资账户 → 投资账户」、sell「投资账户 → 出资账户」，与出资账户的流入/流出
 *   契约语义及转账行阅读顺序一致），两端各自可点击下钻；
 * - 其余交易类型（含不带出资账户的 buy/sell）仍显示主账户名（可点击下钻，issue #97）。
 *
 * 出资账户为空投资账户照常；出资账户命中时资金实际流出方在前（buy：出资账户，
 * sell：投资账户），与转账「资金流出方在前」的阅读顺序一致（issue #1030）。 */
export function renderAccountCell(row: Transaction): VNode {
  if (row.kind === 'transfer' && row.to_account_id) {
    return renderTwoAccountCell(row.account_id, row.to_account_id)
  }
  if (row.kind === 'buy' && row.funding_account_id) {
    return renderTwoAccountCell(row.funding_account_id, row.account_id)
  }
  if (row.kind === 'sell' && row.funding_account_id) {
    return renderTwoAccountCell(row.account_id, row.funding_account_id)
  }
  return h(AccountLink, { accountId: row.account_id })
}
