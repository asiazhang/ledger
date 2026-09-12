import { computed, onMounted, ref } from 'vue'
import { useMessage } from 'naive-ui'
import { api } from '@ledger/api'
import { t } from '@ledger/i18n'
import { errorMessage } from '@/utils/errors'
import { useLoadable } from '@/composables/useLoadable'
import { restartAppShortly } from '@/utils/restart'
import { useAppDialog } from '@/composables/useAppDialog'
import { useModalIntent } from '@/composables/useModalIntent'
import type { Book } from '@ledger/types'

/**
 * useBookSwitcher——侧栏账本入口的弹层逻辑深模块（issue #834 / ADR-0089）：
 * 内化清单加载与刷新、当前本判定、新建/改名/移除动作、切换的轻量确认意图
 * （非破坏级，提示应用将重载，ADR-0078 分级语义）与清单弹层的开合定位，
 * 产出可观察状态与动作；入口组件（BookSidebarEntry）是它的薄适配器，展开态
 * 常驻入口与折叠态浮标共用同一弹层实例。
 *
 * 弹层纪律：清单弹层为非模态面板（下拉菜单族）——点外部/再点入口关闭是其
 * 交互本体（开/关时序归弹层组件库 trigger=click 协调），开/关经 AppPopover
 * 上报弹层注册表（ADR-0035）；切换确认经 useAppDialog 应用内弹窗（遮罩点击
 * 不构成关闭意图，issue #252 语义）；新建/改名弹窗的开启/目标/关闭编排归
 * 弹窗意图工厂 ModalIntent（ADR-0072），意图闭集二：create（无载荷）/
 * rename（携带目标账本行快照）。
 *
 * 动作失败诚实呈现：错误经码化模板本地化后 toast（errorMessage，ADR-0050），
 * 状态不变——改名/新建弹窗保持打开可改后重试；切换失败留在当前账本。
 */

/** 新建/改名弹窗的意图闭集（判别联合；rename 携带开启时快照的目标账本）。 */
export type BookNameIntent = {
  mode: 'create'
} | {
  mode: 'rename'
  book: Book
}

export function useBookSwitcher() {
  const message = useMessage()
  const dialog = useAppDialog()

  // —— 清单状态（list_books 聚合读注册表最新落盘态，#833 契约） ——
  const books = ref<Book[]>([])
  const activeId = ref<string | null>(null)
  const mutable = ref(true)
  const fallbackReason = ref<string | null>(null)
  /** 切换在途（写指针→重载窗口）：期间拒绝再次发起切换。 */
  const switching = ref(false)

  // 清单加载收编 Loadable（issue #1008 / ADR-0040）：loading 置收、竞态裁决与错误
  // toast（默认策略 = 裸 errorMessage）内化；loadFailed 由 error 状态派生——弹层
  // 错误行 + 重试承载动作上下文，清单半边不再手搓 loading/失败位。
  const { loading, error, run: runList } = useLoadable(() => api.listBooks())
  const loadFailed = computed(() => error.value !== null)

  /** 当前活动账本（入口按钮展示名；清单未就绪或损坏时为 null）。 */
  const activeBook = computed(() => books.value.find((b) => b.id === activeId.value) ?? null)

  /** 刷新清单（挂载首刷 + 每次登记变更成功后重拉，弹层列表即时更新）。 */
  async function refresh(): Promise<void> {
    const info = await runList()
    if (info === null) return
    books.value = info.books
    activeId.value = info.active_id
    mutable.value = info.mutable
    fallbackReason.value = info.fallback_reason
  }

  onMounted(() => {
    void refresh()
  })

  // —— 登记变更动作（新建/改名/移除）：成功回 true 并刷新清单，失败 toast 后回 false ——
  async function createBook(name: string): Promise<boolean> {
    try {
      const book = await api.createBook(name)
      message.success(t('books.toasts.created', { name: book.name }))
      await refresh()
      return true
    } catch (e) {
      message.error(errorMessage(e))
      return false
    }
  }

  async function renameBook(id: string, name: string): Promise<boolean> {
    try {
      const book = await api.renameBook(id, name)
      message.success(t('books.toasts.renamed', { name: book.name }))
      await refresh()
      return true
    } catch (e) {
      message.error(errorMessage(e))
      return false
    }
  }

  async function removeBook(book: Book): Promise<boolean> {
    try {
      await api.removeBook(book.id)
      message.success(t('books.toasts.removed', { name: book.name }))
      await refresh()
      return true
    } catch (e) {
      message.error(errorMessage(e))
      return false
    }
  }

  // —— 切换意图：轻量确认（非破坏级）→ 写活动指针 → 原位重引导重载进目标账本 ——
  function requestSwitch(book: Book): void {
    // 当前本不重复切换；登记不可变时后端必拒，入口直接不触发
    if (book.id === activeId.value || !mutable.value || switching.value) return
    dialog.info({
      title: t('books.switchDialog.title'),
      content: t('books.switchDialog.content', { name: book.name }),
      positiveText: t('books.switchDialog.confirm'),
      negativeText: t('books.switchDialog.cancel'),
      maskClosable: false,
      onPositiveClick: () => {
        void performSwitch(book)
      },
    })
  }

  async function performSwitch(book: Book): Promise<void> {
    switching.value = true
    try {
      await api.switchBook(book.id)
      message.success(t('books.toasts.switching', { name: book.name }))
      closePanel()
      restartAppShortly()
    } catch (e) {
      // 切换失败留在当前账本（重载只在命令成功后发生，ADR-0080 语义）
      message.error(errorMessage(e))
    } finally {
      switching.value = false
    }
  }

  // —— 清单弹层开合：show 归弹层组件库协调（trigger=click 的开/关/点外/再点入口
  //  皆由其内部处理，无手写事件序），开关经 update:show 回流；命令式关闭仅两处：
  //  侧栏折叠形态互换时收起、确认切换后随重载收起 ——
  const panelShow = ref(false)

  function closePanel(): void {
    panelShow.value = false
  }

  // —— 新建/改名弹窗（ModalIntent 编排）：草稿在开启时落定，失败保持打开可重试 ——
  const nameIntent = useModalIntent<BookNameIntent>()
  const nameDraft = ref('')
  const nameBusy = ref(false)

  const nameModalTitle = computed(() =>
    nameIntent.intent.value?.mode === 'rename' ? t('books.renameModal.title') : t('books.createModal.title'),
  )
  const nameModalConfirmText = computed(() =>
    nameIntent.intent.value?.mode === 'rename' ? t('books.renameModal.confirm') : t('books.createModal.confirm'),
  )

  function openCreate(): void {
    nameDraft.value = ''
    nameIntent.open({ mode: 'create' })
  }

  function openRename(book: Book): void {
    nameDraft.value = book.name
    nameIntent.open({ mode: 'rename', book })
  }

  /** 提交命名：空名不提交（主操作禁用兜底）；成功关弹窗，失败保持打开可改。 */
  async function submitName(): Promise<void> {
    const intent = nameIntent.intent.value
    const name = nameDraft.value.trim()
    if (!intent || !name || nameBusy.value) return
    nameBusy.value = true
    const ok =
      intent.mode === 'create'
        ? await createBook(name)
        : await renameBook(intent.book.id, name)
    nameBusy.value = false
    if (ok) nameIntent.close()
  }

  return {
    // 清单状态
    books,
    activeId,
    activeBook,
    mutable,
    fallbackReason,
    loading,
    loadFailed,
    refresh,
    // 切换意图
    requestSwitch,
    // 弹层开合
    panelShow,
    closePanel,
    // 新建/改名弹窗
    nameIntent,
    nameDraft,
    nameBusy,
    nameModalTitle,
    nameModalConfirmText,
    openCreate,
    openRename,
    submitName,
    // 移除（行级气泡确认的确认回调）
    removeBook,
  }
}

export type BookSwitcher = ReturnType<typeof useBookSwitcher>
