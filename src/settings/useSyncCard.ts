import { computed, onMounted, ref } from 'vue'
import { useMessage } from 'naive-ui'
import { api } from '@ledger/api'
import { t } from '@ledger/i18n'
import { formatIsoMinute } from '@ledger/utils/datetime'
import { restartAppShortly } from '@/backup/restart'
import { useLoadable } from '@ledger/loadable'
import {
  CUSTOM_VENDOR_ID,
  findVendorPreset,
  matchVendorByEndpoint,
  vendorOptions,
  vendorPrefill,
  vendorTierKey,
  type S3VendorPrefill,
} from '@ledger/utils/s3-vendors'
import type {
  ParkedOpInfo,
  SyncChannelConfig,
  SyncChannelConfigInput,
  SyncCheckpointInfo,
  SyncStatus,
} from '@ledger/types'

/**
 * useSyncCard——多端同步卡片的加载与动作编排深模块（issue #1397 / #1306 结论实施）：
 * 状态回显（上次同步时间、挂起数量与明细）、「立即同步」、通道配置表单（S3 七字段
 * + 同步空间，含厂商预设预填）、保存前「测试连接」、检查点发布与新端引导向导，
 * 全部编排内化于此；入口组件 SyncSettings.vue 是它的薄 adapter——警示 Alert、
 * 引导向导 AppModal 模板与文案留在组件，其余只做渲染接线（ADR-0118：多端同步
 * 前端面住 settings）。
 *
 * 显示值一律来自命令返回（AppSettings 权威），不走 localStorage；通道配置是本机
 * 设备配置（不同步）。自动触发（打开应用即同步 + 运行期低频轮询）由后端编排，
 * 前端零调用面；本模块只呈现「同步到什么状态」。
 *
 * toast 收口（ADR-0040 / #1008 收口延续，issue #1397）：全部异步动作的失败反馈走
 * 各自 Loadable 实例的错误通道（默认策略 = 裸 errorMessage + error 状态双通道），
 * 不再直弹 toast。唯一行为等价例外（显式记录）：失败文案从「前缀：码化错误」降级
 * 为裸码化错误（码化 + params 插值不变，只少前缀；与 #1008 收编批同型，ADR-0040
 * 不设策略注入故前缀无处安放）。成功与 warning 提示（syncOk/saveOk/testOk/
 * publishOk/bootstrapOk/parkedToast/publishPlaintextToast）不是错误反馈，保持动作
 * 内直调。例外之二：引导预检是 silent 实例（error 照常置位、toast 不弹）——失败
 * 经弹窗错误位就地呈现（原 precheckFailed = 裸 errorMessage 不弹 toast，逐字等价）。
 *
 * 密钥回显口径（#1218 验收「加载时不回显完整密钥」）：命令面照旧回显 `secret_key`
 * （「不改密钥直接保存」需要它），界面层不把该值渲染进输入框——密钥输入缓冲恒以
 * 空串起填，已保存值只留在内存表单里；用户未改动即沿用旧值、改动即提交新值。
 *
 * 厂商预设（issue #1220）只做界面预填与端点反查回显，不落库、不进后端契约；预填
 * 与保存/探测共享同一份 form（applyPrefill 是表单字段的单一落点），「测试连接」
 * 在点击那一刻取表单快照（channelPayload），预填与手改一视同仁。i18n key 由本
 * module 持有（useBookSwitcher 先例）。
 *
 * 引导是 ADR-0098 决策 5 钉死的显式向导动作（不挂自动轮次）：open 清口令 → 预检
 * 三态（prechecking / precheckError / checkpointInfo 有无）→ confirm 整库换入 →
 * 关弹窗 → success toast → restartAppShortly 原位重引导（Restore 同型）；失败弹窗
 * 保持打开可就地重试，bootstrapping 兼作重入守卫（enter 键路径）。
 */
export function useSyncCard() {
  const message = useMessage()

  // ---------------------------------------------------------------------------
  // 状态区：同步状态回显 + 挂起明细（issue #862 / #863）
  // ---------------------------------------------------------------------------

  const status = ref<SyncStatus | null>(null)
  // 口令输入：仅密文库需要（留空则后端回退本机已记住口令）；立即同步与发布检查点
  // 共用同一输入框；不落任何本地存储。
  const passphrase = ref('')
  // 挂起操作明细（issue #863 挂起通知）：数量 > 0 时按需拉取，展示码化原因。
  const parkedOps = ref<ParkedOpInfo[]>([])

  /** 状态读取收编 Loadable（ADR-0040）：失败 toast = 裸码化错误（收口例外，见头注）。 */
  const statusLoad = useLoadable(async () => {
    const next = await api.getSyncStatus()
    status.value = next
    await refreshParkedOps()
    return true
  })
  const statusLoading = statusLoad.loading

  async function refreshStatus(): Promise<void> {
    await statusLoad.run()
  }

  /**
   * 拉取挂起明细（issue #863）：仅当数量 > 0 时调用，避免无谓 IPC。失败不升级为
   * 卡片级错误：数量仍由状态回显，重试即下次刷新——console.warn 降级、明细置空，
   * 无 busy 语义，不收编 Loadable（刻意降级是合法形态，ADR-0040 决策 6）。
   */
  async function refreshParkedOps(): Promise<void> {
    if (!status.value || status.value.parked_count === 0) {
      parkedOps.value = []
      return
    }
    try {
      parkedOps.value = await api.getParkedOps()
    } catch (e) {
      console.warn('挂起明细拉取失败', e)
      parkedOps.value = []
    }
  }

  /** 上次同步时刻展示文本（ISO → 本地可读截断，单一格式化点 utils/datetime）。 */
  const lastSyncText = computed(() =>
    status.value?.last_sync_at
      ? formatIsoMinute(status.value.last_sync_at)
      : t('settings.data.sync.neverSynced'),
  )

  // ---------------------------------------------------------------------------
  // 立即同步（issue #862）：手动触发一轮同步，成功轻量提示轮次报告并刷新状态。
  // ---------------------------------------------------------------------------

  const syncLoad = useLoadable(async () => {
    const report = await api.syncNow(passphrase.value || undefined)
    message.success(
      t('settings.data.sync.syncOk', {
        uploaded: report.uploaded_ops,
        applied: report.applied,
        parked: report.parked,
      }),
    )
    await refreshStatus()
    if (report.parked > 0) {
      message.warning(t('settings.data.sync.parkedToast', { count: report.parked }))
    }
    return true
  })
  const syncing = syncLoad.loading

  async function syncNow(): Promise<void> {
    await syncLoad.run()
  }

  // ---------------------------------------------------------------------------
  // 通道表单（S3 七字段 + 同步空间，issue #1218 / #1219 / #1220 / #1221）：初值来自
  // 命令回显（未配置为空表单，空间字段填默认值）；v1 唯一后端是 S3 兼容对象存储，
  // 配置面没有后端判别字段（WebDAV 已随 #1221 整体退役）。
  // ---------------------------------------------------------------------------

  // 表单只装本界面拥有的字段：命令回显形态 `SyncChannelConfig` 的字段与输入面
  // 一一对应，回显时投影一次以隔开「契约对象」与「草稿对象」两类状态。
  type ChannelForm = Pick<
    SyncChannelConfig,
    | 'space_id'
    | 'endpoint'
    | 'region'
    | 'bucket'
    | 'prefix'
    | 'access_key'
    | 'secret_key'
    | 'path_style'
  >

  const form = ref<ChannelForm>({
    space_id: 'default',
    endpoint: '',
    region: '',
    bucket: '',
    prefix: '',
    access_key: '',
    secret_key: '',
    path_style: false,
  })

  // 密钥输入缓冲（#1218 验收「加载时不回显完整密钥」）：输入框只绑本 ref，加载与
  // 保存成功后一律清回空串——已保存密钥只活在 form.secret_key（内存），不上屏。
  const secretKeyInput = ref('')

  /** 密钥输入框占位：已保存过密钥时提示「留空则保持不变」，否则是普通字段名。 */
  const secretKeyPlaceholder = computed(() =>
    form.value.secret_key
      ? t('settings.data.sync.secretKeySavedPlaceholder')
      : t('settings.data.sync.secretKeyPlaceholder'),
  )

  // 厂商预设（issue #1220）：用户选中的厂商判别键。它只是界面态——不随表单保存，
  // 命令面 `SyncChannelConfig` 也没有厂商字段；「是谁」由端点反查决定（再次打开
  // 或保存回显时按端点重算），所以这条状态不可能是落库数据的第二事实源。
  const selectedVendor = ref<string>(CUSTOM_VENDOR_ID)

  /** 当前选中厂商的预设（「其他（自定义）」或未知 id 为 null）。 */
  const selectedVendorPreset = computed(() => findVendorPreset(selectedVendor.value))

  /**
   * 下拉项：预设按声明序 + 末尾固定「其他（自定义）」（issue #1220 验收判据）。
   * 选项标签 = 厂商专名 + 档位标注（options 里的 name 不进翻译，档位文案经 i18n）。
   */
  const vendorSelectOptions = computed(() =>
    vendorOptions().map((option) => ({
      value: option.id,
      label: option.custom
        ? t('settings.data.sync.vendorCustom')
        : t('settings.data.sync.vendorOption', {
            name: option.name,
            tier: t(vendorTierKey(option.verified)),
          }),
    })),
  )

  /**
   * 选中厂商：预填端点模板、默认地域与寻址方式（纯函数产出的值，只落表单）。
   * 「其他（自定义）」不预填——字段保持用户已填内容，等待用户自己写端点。
   */
  function onVendorChange(vendorId: string): void {
    selectedVendor.value = vendorId
    const prefill = vendorPrefill(vendorId)
    if (prefill) applyPrefill(prefill)
  }

  /** 常用地域快捷项：换地域即按当前厂商模板重写端点（字段随后仍可手改）。 */
  function applyVendorRegion(region: string): void {
    const prefill = vendorPrefill(selectedVendor.value, region)
    if (prefill) applyPrefill(prefill)
  }

  /** 预填值落进表单的单一落点（选中预填与地域快捷项共用，避免两处各写一遍字段）。 */
  function applyPrefill(prefill: S3VendorPrefill): void {
    form.value.endpoint = prefill.endpoint
    form.value.region = prefill.region
    form.value.path_style = prefill.pathStyle
  }

  /** 通道配置读取收编 Loadable（ADR-0040）：失败 toast = 裸码化错误（收口例外）。 */
  const channelLoad = useLoadable(async () => {
    const config = await api.getSyncChannelConfig()
    // 投影进表单（不持有回显对象本体）：表单的 v-model 会就地改写所绑对象，
    // 直接拿 IPC 契约快照当草稿纸用，等于把响应体当可变状态。
    form.value = {
      space_id: config.configured ? config.space_id : 'default',
      endpoint: config.endpoint,
      region: config.region,
      bucket: config.bucket,
      prefix: config.prefix,
      access_key: config.access_key,
      secret_key: config.secret_key,
      path_style: config.path_style,
    }
    // 密钥输入恒从空白起（不回显完整密钥）；已保存值留在 form 内供「留空沿用」。
    secretKeyInput.value = ''
    // 厂商回显按端点反查（issue #1220 验收判据）：命中厂商即回显该厂商，未命中
    //（自建服务、空表单、改过的端点）回「其他（自定义）」——不额外落库厂商字段。
    selectedVendor.value = matchVendorByEndpoint(form.value.endpoint)
    return true
  })

  async function refreshChannelConfig(): Promise<void> {
    await channelLoad.run()
  }

  /**
   * 表单 → 命令入参的单一转换点（保存与「测试连接」共用，issue #1218 / #1219）：
   * S3 七字段与同步空间（跨端共识的世界身份；空值交由后端回默认）。
   *
   * 密钥取值：输入框有内容（用户改过）用新值，为空则沿用内存里的已保存值——这是
   * 「加载时不回显完整密钥」前提下仍能「不改密钥直接保存」的机制。两个动作共用本
   * 转换点，「测通了就能存进去」才对同一份表单成立。
   */
  function channelPayload(): SyncChannelConfigInput {
    return {
      space_id: form.value.space_id.trim() || undefined,
      endpoint: form.value.endpoint,
      region: form.value.region,
      bucket: form.value.bucket,
      prefix: form.value.prefix,
      access_key: form.value.access_key,
      secret_key: secretKeyInput.value !== '' ? secretKeyInput.value : form.value.secret_key,
      path_style: form.value.path_style,
    }
  }

  /**
   * 保存通道配置（issue #1218）：表单经 [`channelPayload`] 落到后端；保存成功后重新
   * 回显，把落库结果（含后端归一化后的字段）呈现在表单上。
   */
  const saveLoad = useLoadable(async () => {
    await api.setSyncChannelConfig(channelPayload())
    message.success(t('settings.data.sync.saveOk'))
    await Promise.all([refreshStatus(), refreshChannelConfig()])
    return true
  })
  const saving = saveLoad.loading

  async function saveChannel(): Promise<void> {
    await saveLoad.run()
  }

  // ---------------------------------------------------------------------------
  // 保存前「测试连接」（issue #1219）：把当前表单（尚未落库）交给后端做一次对象
  // 读取探针，当场回答「这份配置能不能用」——成功即通道可读；失败按后端分层码
  //（凭据 / 目标 / 权限 / 网络 / 服务）经 Loadable 错误通道本地化（ADR-0040 /
  // #1008 收口，issue #1219 时的既有滩头），给出可自救的下一步。
  //
  // 探测与厂商预设共存的方式很直接：探测在点击那一刻取一次表单快照（
  // [`channelPayload`]），所以「下拉预填 / 地域快捷项 / 手改」的结果一视同仁，
  // 两者不共享任何写入状态；按钮态也各管各的（probe.loading vs saving），
  // 预填不因探测而禁用，探测不因预填而失效。
  //
  // 不写任何本地状态：探测不落库、不改本机已保存配置，用户改坏表单也不影响既有同步。
  // ---------------------------------------------------------------------------

  const probe = useLoadable(async () => {
    await api.testSyncChannelConnection(channelPayload())
    return true
  })
  const testing = probe.loading

  async function testConnection(): Promise<void> {
    if (await probe.run()) {
      message.success(t('settings.data.sync.testOk'))
    }
  }

  // ---------------------------------------------------------------------------
  // 检查点发布与新端引导（issue #864）：命令面 get_sync_channel_checkpoint /
  // publish_sync_checkpoint / bootstrap_sync_from_channel；引导是显式向导动作
  //（ADR-0098 决策 5），确认步展示整库替换与重启后果，成功即原位重引导。
  // ---------------------------------------------------------------------------

  /** 快照体大小展示文本（字节 → MB，一位小数；toast 插值用，module 私有）。 */
  function formatSizeMb(bytes: number): string {
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`
  }

  /** 发布检查点到通道：存量数据的设备把「新端可引导的来源」放上通道。 */
  const publishLoad = useLoadable(async () => {
    const result = await api.publishSyncCheckpoint(passphrase.value || undefined)
    message.success(
      t('settings.data.sync.publishOk', {
        generation: result.generation,
        size: formatSizeMb(result.size),
      }),
    )
    if (result.plaintext_mode) {
      // 明文显著提示（ADR-0091 决策 8）：快照整库明文上通道，与常驻卡片警示同义。
      message.warning(t('settings.data.sync.publishPlaintextToast'))
    }
    return true
  })
  const publishing = publishLoad.loading

  async function publishCheckpoint(): Promise<void> {
    await publishLoad.run()
  }

  // 引导向导状态：弹窗开合、口令与检查点（module 自持——Loadable 不持任务结果，
  // ADR-0040 决策 1，precheck run() 返回后存入）。
  const bootstrapShow = ref(false)
  const bootstrapPassphrase = ref('')
  const checkpointInfo = ref<SyncCheckpointInfo | null>(null)

  // 引导预检：silent 实例（ADR-0040 决策 3 修订注）——error 照常置位、toast 不弹，
  // 弹窗错误位渲染 precheckError（原 precheckFailed = 裸 errorMessage，逐字等价）。
  const precheck = useLoadable(() => api.getSyncChannelCheckpoint(), { silent: true })
  const prechecking = precheck.loading
  const precheckError = precheck.error

  /** 打开引导向导并预检通道（只读 manifest，不下载快照体）。 */
  async function openBootstrap(): Promise<void> {
    bootstrapShow.value = true
    bootstrapPassphrase.value = ''
    checkpointInfo.value = null
    checkpointInfo.value = await precheck.run()
  }

  // 引导确认收编 Loadable（ADR-0040）：失败 toast = 裸码化错误（收口例外）；loading
  // 兼作重入守卫（enter 键路径保留），bootstrapping 归零后弹窗仍开、可就地重试。
  const bootstrapLoad = useLoadable(
    async () => await api.bootstrapSyncFromChannel(bootstrapPassphrase.value || undefined),
  )
  const bootstrapping = bootstrapLoad.loading

  /** 确认引导：整库换入通道快照，成功后原位重引导（Restart 同型，重启载入数据）。 */
  async function confirmBootstrap(): Promise<void> {
    if (!checkpointInfo.value || bootstrapping.value) return
    const outcome = await bootstrapLoad.run()
    if (outcome === null) return
    bootstrapShow.value = false
    message.success(
      t('settings.data.sync.bootstrapOk', {
        generation: outcome.generation,
        size: formatSizeMb(outcome.size),
        reencrypted: outcome.reencrypted ? t('settings.data.sync.bootstrapReencrypted') : '',
      }),
    )
    restartAppShortly()
  }

  // 挂载首刷：状态与通道配置并行拉取（时序内化，adapter 零生命周期义务）。
  onMounted(async () => {
    await Promise.all([refreshStatus(), refreshChannelConfig()])
  })

  return {
    // 状态区
    status,
    statusLoading,
    lastSyncText,
    refreshStatus,
    parkedOps,
    passphrase,
    // 立即同步
    syncing,
    syncNow,
    // 通道表单（含厂商预填）
    form,
    secretKeyInput,
    secretKeyPlaceholder,
    selectedVendor,
    selectedVendorPreset,
    vendorSelectOptions,
    onVendorChange,
    applyVendorRegion,
    channelPayload,
    refreshChannelConfig,
    saving,
    saveChannel,
    // 测试连接
    testing,
    testConnection,
    // 检查点与引导
    publishing,
    publishCheckpoint,
    bootstrapShow,
    prechecking,
    precheckError,
    checkpointInfo,
    bootstrapPassphrase,
    bootstrapping,
    openBootstrap,
    confirmBootstrap,
  }
}

export type SyncCard = ReturnType<typeof useSyncCard>
