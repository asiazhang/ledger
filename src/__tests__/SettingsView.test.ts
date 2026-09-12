import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { findButton, findBodyButtonByTestId } from '@ledger/test-support/dom'
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { flushPromises } from '@vue/test-utils'

import { useAppStore } from '@/stores/app'
import { applyLocale } from '@/i18n'
import SettingsView from '@/views/SettingsView.vue'
import CategoryManager from '@/components/CategoryManager.vue'
import { captureLastListener, mockListen } from '@ledger/test-support/listen-mock'

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
  save: vi.fn(),
}))

import { open, save } from '@tauri-apps/plugin-dialog'

const mockOpen = vi.mocked(open)
const mockSave = vi.mocked(save)

/** 数据存储位置信息桩：统一形状，各用例只覆写差异字段。 */
function dataLocationInfo(
  overrides: Partial<{ active_dir: string; configured_dir: string | null; pending_restart: boolean; fallback_reason: string | null }> = {},
) {
  return {
    active_dir: '/Users/me/Library/Application Support/ledger',
    configured_dir: null,
    pending_restart: false,
    fallback_reason: null,
    ...overrides,
  }
}

/**
 * 场景级接缝布线表（issue #747）：defaults 只留本场景非参考命令的静态契约快照，
 * 五个参考 list 命令由接缝内建规范夹具兜底，不再枚举。「数据」pane 用
 * display-directive='show:lazy'，首次激活挂载；pane 内子页签（issue #568）默认
 * 「备份」，DataLocationSettings 随「存储位置」子页签首切才挂载（show:lazy），
 * 故 get_data_location_info 在场景级 overrides（子页签切换测试会触发）。
 */
const SCENE_DEFAULTS = {
  list_backups: [],
  get_log_level: { level: 'info' },
  create_backup: {
    path: '/tmp/ledger-backup.db.zip',
    size_bytes: 1024,
    schema_version: 4,
    created_at: '2026-01-01T00:00:00Z',
  },
  restore_backup: { schema_version: 4, restored_at: '2026-01-01T00:00:00Z' },
  restart_app: null,
  prune_backups: { kept: 0, deleted: [], failed: [] },
  get_auto_backup_state: { enabled: true, last_backup_at: null },
  set_auto_backup_enabled: null,
}

/** 场景级函数型应答（overrides 表）。 */
const SCENE_OVERRIDES = {
  get_data_location_info: () => dataLocationInfo(),
}

/** 定位标题为指定文本的卡片（Naive UI 卡片头主标题元素）。 */
function findCardByTitle(wrapper: ReturnType<typeof mount>, title: string) {
  return wrapper.findAll('.n-card').find((c) => c.find('.n-card-header__main').text() === title)
}

/** 按标签文本点击设置页 Tab（避免依赖不稳定的位置下标）；「数据」内子页签同名不冲突，同一助手通吃。 */
async function openTab(wrapper: ReturnType<typeof mount>, label: string) {
  const tab = wrapper.findAll('.n-tabs-tab').find((t) => t.text() === label)
  expect(tab, `设置页应存在「${label}」Tab`).toBeTruthy()
  await tab!.trigger('click')
  await nextTick()
}

beforeEach(async () => {
  mockOpen.mockReset()
  mockSave.mockReset()
  // 参考 store 预载走接缝 opt-in 参数（清理四件套由全局壳层每测执行）。
  const base = wireInvokeSeam({
    defaults: SCENE_DEFAULTS,
    overrides: SCENE_OVERRIDES,
    refreshReferenceStores: true,
  })
  await base.ready
})

describe('SettingsView.vue Tab 分域（issue #157 ADR-0022 立项；现役格局 5 页签）', () => {
  it('Tab 格局为 通用 → 分类 → 数据 → 定时 → 关于，共 5 个，关于在末位（#308 定时；#444 商户 Tab 移除——商户管理迁入「更多」聚合页，入口唯一）', () => {
    const wrapper = mount(SettingsView)
    const labels = wrapper.findAll('.n-tabs-tab').map((t) => t.text())
    expect(labels).toEqual(['通用', '分类', '数据', '定时', '关于'])
  })

  it('英文界面：Tab 页签以英文渲染，切回中文后恢复（issue #352）', async () => {
    await applyLocale('en-US')
    let enLabels: string[] = []
    try {
      const wrapper = mount(SettingsView)
      enLabels = wrapper.findAll('.n-tabs-tab').map((tab) => tab.text())
    } finally {
      await applyLocale('zh-CN')
      await nextTick()
    }
    expect(enLabels).toEqual(['General', 'Categories', 'Data', 'Scheduled', 'About'])
    // 切回中文后新挂载的组件恢复中文页签
    const wrapper = mount(SettingsView)
    expect(wrapper.findAll('.n-tabs-tab').map((tab) => tab.text())).toEqual([
      '通用',
      '分类',
      '数据',
      '定时',
      '关于',
    ])
  })

  it('旧 Tab（备份与恢复 / 外观 / 存储位置）全部消失', () => {
    // 「分类」「币种」不再列入：ADR-0034 后「分类」是现役 Tab 名（原「分类与币种」更名），
    // 币种只读展示已移除，不再有独立币种 Tab。
    const wrapper = mount(SettingsView)
    const labels = wrapper.findAll('.n-tabs-tab').map((t) => t.text())
    expect(labels).not.toContain('币种')
    expect(labels).not.toContain('备份与恢复')
    expect(labels).not.toContain('外观')
    expect(labels).not.toContain('存储位置')
    // #444：商户管理迁入「更多」聚合页，设置页不再承载商户入口
    expect(labels).not.toContain('商户')
  })

  it('「通用」默认激活，含深色模式开关、展示币种下拉与日志卡片，不含账本级本位币基准（issue #858 币种设置拆分；issue #930 日志卡片迁入）', async () => {
    const wrapper = mount(SettingsView)
    // 通用是首个 Tab，无需点击即挂载（show:lazy 语义）。
    const html = wrapper.html()
    expect(html).toContain('深色模式')
    expect(html).toContain('展示币种')
    // 日志卡片（issue #930 / ADR-0022 修订）：等级下拉与「打开日志目录」同卡在末位。
    expect(html).toContain('日志等级')
    expect(html).toContain('打开日志目录')
    // 本位币基准是账本级设置，落「分类」页签。
    expect(html).not.toContain('本位币基准')
    // 深色模式开关反映当前主题（默认暗色）。
    expect(wrapper.find('.n-switch').attributes('aria-checked')).toBe('true')
  })

  it('「通用」内切换深色模式开关更新 app store 主题', async () => {
    const store = useAppStore()
    const wrapper = mount(SettingsView)
    await wrapper.find('.n-switch').trigger('click')
    expect(store.theme).toBe('light')
  })

  it('「分类」含分类管理器与本位币基准卡片（issue #858，账本级设置按领域归属落本页签）', async () => {
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '分类')
    expect(wrapper.findComponent(CategoryManager).exists()).toBe(true)
    const html = wrapper.html()
    expect(html).toContain('本位币基准')
    // 币种字典本体仍无维护界面（ADR-0034，只读表格不回归）。
    expect(html).not.toContain('支持币种')
    expect(html).not.toContain('默认币种')
  })

  it('「数据」页签内部子页签为 备份 / 存储位置 / 数据修复，默认「备份」（issue #568）', async () => {
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await flushPromises()
    const labels = wrapper.findAll('.n-tabs-tab').map((t) => t.text())
    expect(labels).toContain('备份')
    expect(labels).toContain('存储位置')
    expect(labels).toContain('数据修复')
    // 默认「备份」：备份组件已挂载，存储位置与数据修复组件未挂载（子 pane show:lazy）。
    const html = wrapper.html()
    expect(html).toContain('一键备份')
    expect(html).not.toContain('数据存储位置')
    expect(html).not.toContain('拼音搜索数据')
  })

  it('子页签来回切换备份列表不卸载重拉（子 pane show:lazy + 显式 key，issue #568）', async () => {
    useAppStore().setBackupDir('/Users/me/backups')
    let listBackupsCalls = 0
    wireInvokeSeam({
      defaults: SCENE_DEFAULTS,
      overrides: {
        ...SCENE_OVERRIDES,
        list_backups: () => {
          listBackupsCalls++
          return Promise.resolve([])
        },
      },
    })
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await flushPromises()
    expect(listBackupsCalls).toBe(1)
    await openTab(wrapper, '存储位置')
    await openTab(wrapper, '数据修复')
    await openTab(wrapper, '备份')
    await flushPromises()
    expect(listBackupsCalls).toBe(1)
    expect(wrapper.html()).toContain('当前共 0 个备份，上限 30 个')
  })

  it('子页签选中态不持久化：离开设置页再回来默认回「备份」（issue #568）', async () => {
    wireInvokeSeam({
      defaults: SCENE_DEFAULTS,
      overrides: {
        ...SCENE_OVERRIDES,
        get_data_location_info: () => Promise.resolve(dataLocationInfo()),
      },
    })
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await flushPromises()
    await openTab(wrapper, '存储位置')
    await flushPromises()
    expect(wrapper.html()).toContain('数据存储位置')
    wrapper.unmount()

    const wrapper2 = mount(SettingsView)
    await openTab(wrapper2, '数据')
    await flushPromises()
    const html = wrapper2.html()
    expect(html).toContain('一键备份')
    expect(html).not.toContain('数据存储位置')
  })

  it('设置页内容列限宽约 720px、左对齐不居中（issue #651）', () => {
    const wrapper = mount(SettingsView)
    const column = wrapper.find('[data-testid="settings-column"]')
    expect(column.exists()).toBe(true)
    const style = column.attributes('style') ?? ''
    expect(style).toContain('max-width: 720px')
    // 左对齐：无居中 margin（margin auto 居中与否在此由 margin 属性是否出现表达）。
    expect(style).not.toContain('margin')
  })

  it('备份列表在 Tab 切换间保留缓存，不随切换重拉', async () => {
    useAppStore().setBackupDir('/Users/me/backups')
    let listBackupsCalls = 0
    wireInvokeSeam({
      defaults: SCENE_DEFAULTS,
      overrides: {
        ...SCENE_OVERRIDES,
        list_backups: () => {
          listBackupsCalls++
          return Promise.resolve([])
        },
      },
    })
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await flushPromises()
    expect(listBackupsCalls).toBe(1)
    expect(wrapper.html()).toContain('当前共 0 个备份，上限 30 个')

    // 切走再切回：数据 pane 保持挂载（display-directive='show:lazy'），不重拉。
    await openTab(wrapper, '通用')
    await openTab(wrapper, '数据')
    await flushPromises()
    expect(listBackupsCalls).toBe(1)
    expect(wrapper.html()).toContain('当前共 0 个备份，上限 30 个')
  })

  it('备份与恢复：目录选择持久化到 localStorage', async () => {
    mockOpen.mockResolvedValue('/Users/me/ledger-backups')
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await nextTick()
    // 按文本定位目录按钮（卡片重排后首个按钮不再固定是它，issue #651）。
    const dirBtn = findButton(wrapper, '选择目录')!
    await dirBtn.trigger('click')
    await nextTick()
    expect(mockOpen).toHaveBeenCalledWith({ directory: true, multiple: false, title: '选择备份目录' })
    expect(localStorage.getItem('backup_dir')).toBe('"/Users/me/ledger-backups"')
  })

  it('一键备份调用 create_backup 命令', async () => {
    const store = useAppStore()
    store.setBackupDir('/Users/me/backups')
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await nextTick()
    const backupBtn = findButton(wrapper, '一键备份')!
    await backupBtn.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith(
      'create_backup',
      expect.objectContaining({ targetPath: expect.stringMatching(/ledger-backup-\d{8}-\d{6}\.db\.zip$/) }),
    )
    expect(wrapper.html()).toContain('最近备份')
  })

  it('一键备份写入受管目录后自动滚动清理', async () => {
    const store = useAppStore()
    store.setBackupDir('/Users/me/backups')
    wireInvokeSeam({
      defaults: SCENE_DEFAULTS,
      overrides: {
        ...SCENE_OVERRIDES,
        create_backup: () => ({
          path: '/Users/me/backups/ledger-backup-20260101-010101.db.zip',
          size_bytes: 1024,
          schema_version: 4,
          created_at: '2026-01-01T01:01:01Z',
        }),
      },
    })
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await nextTick()
    const backupBtn = findButton(wrapper, '一键备份')!
    await backupBtn.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('prune_backups', { dir: '/Users/me/backups', keep: 30 })
  })

  it('备份文件列表展示数量与上限，手动清理需确认', async () => {
    const store = useAppStore()
    store.setBackupDir('/Users/me/backups')
    store.setBackupMaxCount(1)
    wireInvokeSeam({
      defaults: SCENE_DEFAULTS,
      overrides: {
        ...SCENE_OVERRIDES,
        list_backups: () => [
          {
            file_name: 'ledger-backup-20260102-010101.db.zip',
            path: '/Users/me/backups/ledger-backup-20260102-010101.db.zip',
            size_bytes: 2048,
            created_at: '2026-01-02T01:01:01Z',
          },
          {
            file_name: 'ledger-backup-20260101-010101.db.zip',
            path: '/Users/me/backups/ledger-backup-20260101-010101.db.zip',
            size_bytes: 1024,
            created_at: '2026-01-01T01:01:01Z',
          },
        ],
        prune_backups: () => ({ kept: 1, deleted: ['ledger-backup-20260101-010101.db.zip'], failed: [] }),
      },
    })
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await flushPromises()
    expect(wrapper.html()).toContain('当前共 2 个备份，上限 1 个')
    const pruneBtn = findButton(wrapper, '立即清理')!
    await pruneBtn.trigger('click')
    await flushPromises()
    // 手动清理确认弹窗（issue #652 / ADR-0078）：应用内 warning 级确认后续接删除
    const confirmPruneBtn = findBodyButtonByTestId('danger-confirm')
    expect(confirmPruneBtn, '清理确认弹窗应弹出').toBeTruthy()
    await confirmPruneBtn!.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('prune_backups', { dir: '/Users/me/backups', keep: 1 })
  })

  it('备份保留上限可配置并持久化', async () => {
    const store = useAppStore()
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await nextTick()
    const input = wrapper.find('.n-input-number input')
    await input.setValue('10')
    await input.trigger('blur')
    expect(store.backupMaxCount).toBe(10)
    expect(localStorage.getItem('backup_max_count')).toBe('10')
  })

  it('自动备份卡片展示开关与上次自动备份时间', async () => {
    wireInvokeSeam({
      defaults: SCENE_DEFAULTS,
      overrides: {
        ...SCENE_OVERRIDES,
        get_auto_backup_state: () => ({ enabled: false, last_backup_at: '2026-02-17T09:30:00Z' }),
      },
    })
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await flushPromises()
    const html = wrapper.html()
    expect(html).toContain('自动备份')
    expect(html).toContain('上次自动备份：2026-02-17 09:30')
    // 用语义属性 aria-checked 断言开关状态，不依赖内部样式类。
    const backupSwitch = findCardByTitle(wrapper, '自动备份')!.find('.n-switch')
    expect(backupSwitch.attributes('aria-checked')).toBe('false')
  })

  it('切换自动备份开关调用 set_auto_backup_enabled 并刷新展示', async () => {
    let enabledState = true
    wireInvokeSeam({
      defaults: SCENE_DEFAULTS,
      overrides: {
        ...SCENE_OVERRIDES,
        get_auto_backup_state: () => ({ enabled: enabledState, last_backup_at: null }),
        set_auto_backup_enabled: (args?: Record<string, unknown>) => {
          enabledState = args?.enabled === true
          return Promise.resolve()
        },
      },
    })
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await flushPromises()
    const backupSwitch = findCardByTitle(wrapper, '自动备份')!.find('.n-switch')
    await backupSwitch.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('set_auto_backup_enabled', { enabled: false })
    expect(backupSwitch.attributes('aria-checked')).toBe('false')
  })

  it('未配置备份目录时提示引导，配置后提示消失', async () => {
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await flushPromises()
    expect(wrapper.html()).toContain('设置备份目录后自动备份生效')

    useAppStore().setBackupDir('/Users/me/backups')
    await nextTick()
    expect(wrapper.html()).not.toContain('设置备份目录后自动备份生效')
  })

  it('恢复前经应用内弹窗确认（issue #572）：读取备份元数据，确认后带 passphrase 调用 restore_backup', async () => {
    mockOpen.mockResolvedValueOnce('/Users/me/backups/ledger-backup.db.zip')
    wireInvokeSeam({
      defaults: SCENE_DEFAULTS,
      overrides: {
        ...SCENE_OVERRIDES,
        get_backup_meta: () => ({ kind: 'manual', encrypted: false }),
        get_encryption_status: () => ({ locked: false, file_encrypted: false }),
      },
    })
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await nextTick()
    const restoreBtn = findButton(wrapper, '从备份恢复')!
    await restoreBtn.trigger('click')
    await flushPromises()
    // 元数据先行：确认弹窗已开（teleport 到 body），恢复尚未执行。
    expect(mockInvoke).toHaveBeenCalledWith('get_backup_meta', {
      path: '/Users/me/backups/ledger-backup.db.zip',
    })
    expect(mockInvoke).not.toHaveBeenCalledWith('restore_backup', expect.anything())
    const confirmBtn = findBodyButtonByTestId('restore-confirm')
    expect(confirmBtn, '恢复确认弹窗应弹出').toBeTruthy()
    await confirmBtn!.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('restore_backup', {
      backupPath: '/Users/me/backups/ledger-backup.db.zip',
      passphrase: null,
    })
  })

  it('备份文件列表展示来源列，区分自动与手动（issue #129）', async () => {
    const store = useAppStore()
    store.setBackupDir('/Users/me/backups')
    wireInvokeSeam({
      defaults: SCENE_DEFAULTS,
      overrides: {
        ...SCENE_OVERRIDES,
        list_backups: () => [
          {
            file_name: 'ledger-auto-20260217-093000.db.zip',
            path: '/Users/me/backups/ledger-auto-20260217-093000.db.zip',
            size_bytes: 4096,
            created_at: '2026-02-17T09:30:00Z',
            kind: 'auto',
          },
          {
            file_name: 'ledger-backup-20260101-010101.db.zip',
            path: '/Users/me/backups/ledger-backup-20260101-010101.db.zip',
            size_bytes: 1024,
            created_at: '2026-01-01T01:01:01Z',
            kind: 'manual',
          },
        ],
      },
    })
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await flushPromises()

    const headers = wrapper.findAll('th').map((t) => t.text())
    expect(headers).toContain('来源')
    const cellTexts = wrapper.findAll('tbody td').map((t) => t.text())
    expect(cellTexts).toContain('自动')
    expect(cellTexts).toContain('手动')
  })

  it('ledger:backups-changed 到达后自动刷新备份列表（issue #129）', async () => {
    useAppStore().setBackupDir('/Users/me/backups')
    mockListen.mockReset()
    const readBackupsChanged = captureLastListener()
    let backupList: unknown[] = []
    wireInvokeSeam({
      defaults: SCENE_DEFAULTS,
      overrides: {
        ...SCENE_OVERRIDES,
        list_backups: () => Promise.resolve(backupList),
        get_auto_backup_state: () => ({ enabled: true, last_backup_at: null }),
      },
    })

    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await flushPromises()
    expect(wrapper.html()).toContain('当前共 0 个备份，上限 30 个')

    // 后端自动备份完成 → 发出无 payload 信号 → 列表自动重拉。
    backupList = [
      {
        file_name: 'ledger-auto-20260217-093000.db.zip',
        path: '/Users/me/backups/ledger-auto-20260217-093000.db.zip',
        size_bytes: 4096,
        created_at: '2026-02-17T09:30:00Z',
        kind: 'auto',
      },
    ]
    readBackupsChanged()?.()
    await flushPromises()

    expect(wrapper.html()).toContain('当前共 1 个备份，上限 30 个')
    const cellTexts = wrapper.findAll('tbody td').map((t) => t.text())
    expect(cellTexts).toContain('ledger-auto-20260217-093000.db.zip')
    expect(cellTexts).toContain('自动')
  })

  it('存储位置异常态文案不变：待重启提示与回退告警照常展示', async () => {
    wireInvokeSeam({
      defaults: SCENE_DEFAULTS,
      overrides: {
        ...SCENE_OVERRIDES,
        get_data_location_info: () =>
          Promise.resolve(dataLocationInfo({ pending_restart: true, configured_dir: '/Users/me/ledger-data' })),
      },
    })
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '数据')
    await openTab(wrapper, '存储位置')
    await flushPromises()
    let html = wrapper.html()
    expect(html).toContain('数据存储位置')
    expect(html).toContain('/Users/me/ledger-data')
    expect(html).toContain('下次启动')

    // 回退告警：fallback_reason 非空时展示回退提示，原路径仍可见。
    wireInvokeSeam({
      defaults: SCENE_DEFAULTS,
      overrides: {
        ...SCENE_OVERRIDES,
        get_data_location_info: () =>
          Promise.resolve(dataLocationInfo({ fallback_reason: '配置的位置不可用：权限不足' })),
      },
    })
    const wrapper2 = mount(SettingsView)
    await openTab(wrapper2, '数据')
    await openTab(wrapper2, '存储位置')
    await flushPromises()
    html = wrapper2.html()
    expect(html).toContain('已回退到默认位置')
    expect(html).toContain('权限不足')
  })
})

describe('SettingsView.vue 页签内容存在性矩阵（issue #769：切页签断言文案在场的用例收行）', () => {
  // 行 =（导航动作，期望文案数组）；删掉对应页签/入口即红（生杀线内，见 CONTEXT-testing「存在性断言」）。
  // 行 ↔ 原用例对应：
  //   通用 → 原「页含界面语言选择器，默认跟随系统」（#342 / ADR-0049）；
  //   关于 → 原「在末位，显示版本号」（末位次序由 Tab 格局用例的全等断言守护）；
  //   数据 → 原「从未自动备份时显示从未占位」；
  //   数据 → 备份 / 存储位置 / 数据修复 三行 ↔ 原「三组件随子页签挂载」（#568）按子页签拆行；
  //     其中 get_data_location_info 调用断言不另保留——目录文案即该调用应答的渲染结果，
  //     渲染断言已覆盖其失败面。
  it.each([
    { nav: '进入「通用」页签', path: ['通用'], texts: ['界面语言', '跟随系统'] },
    { nav: '进入「关于」页签', path: ['关于'], texts: ['版本号'] },
    { nav: '进入「数据」页签', path: ['数据'], texts: ['上次自动备份：从未'] },
    { nav: '进入「数据 → 备份」子页签', path: ['数据', '备份'], texts: ['一键备份', '从备份恢复'] },
    {
      nav: '进入「数据 → 存储位置」子页签',
      path: ['数据', '存储位置'],
      texts: ['数据存储位置', '/Users/me/Library/Application Support/ledger'],
    },
    { nav: '进入「数据 → 数据修复」子页签', path: ['数据', '数据修复'], texts: ['拼音搜索数据'] },
  ])('$nav：期望文案在场 $texts', async ({ path, texts }) => {
    const wrapper = mount(SettingsView)
    for (const tab of path) {
      await openTab(wrapper, tab)
      await flushPromises()
    }
    const html = wrapper.html()
    for (const text of texts) expect(html).toContain(text)
  })
})

describe('SettingsView.vue 定时 Tab：设备级自动执行开关（issue #308 / ADR-0042）', () => {
  it('「定时」内含自动执行卡片：默认关，附「只应在一台机器开启」与设备偏好说明', async () => {
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '定时')
    const card = findCardByTitle(wrapper, '自动执行')
    expect(card, '「定时」Tab 应存在「自动执行」卡片').toBeTruthy()
    const text = card!.text()
    expect(text).toContain('自动执行只应在一台机器开启')
    expect(text).toContain('不随备份')
    // 默认关（ADR-0042：设备级开关默认关，换新机器或恢复备份后保持本机值）。
    expect(card!.find('.n-switch').attributes('aria-checked')).toBe('false')
  })

  it('切换开关更新 store 并持久化 localStorage（设备偏好落点，不经后端持久化）', async () => {
    const store = useAppStore()
    const wrapper = mount(SettingsView)
    await openTab(wrapper, '定时')
    const sw = findCardByTitle(wrapper, '自动执行')!.find('.n-switch')
    await sw.trigger('click')
    expect(store.autoExecutionEnabled).toBe(true)
    expect(localStorage.getItem('auto_execution_enabled')).toBe('true')
    expect(findCardByTitle(wrapper, '自动执行')!.find('.n-switch').attributes('aria-checked')).toBe('true')
    await sw.trigger('click')
    expect(store.autoExecutionEnabled).toBe(false)
    expect(localStorage.getItem('auto_execution_enabled')).toBe('false')
  })
})
