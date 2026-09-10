import { describe, it, expect } from 'vitest'
import { mockInvoke, wireInvokeSeam, lastInvokeArgs } from './helpers/invoke-mock'
import { messageCalls } from './helpers/message-mock'
import { findButtonByTestId, findInputByTestId } from './helpers/dom'
import { mount, flushPromises } from '@vue/test-utils'
import type { ParkedOpInfo, SyncChannelConfig, SyncRoundReport, SyncStatus } from '@/types'

import SyncSettings from '@/components/settings/SyncSettings.vue'

// 多端同步卡片组件测试（issue #862）：invoke 测试接缝（defaults/overrides 表，
// ADR-0085）布线命令替身；断言强度对准用户可观察回归（状态回显、明文警示、
// 动作调用与效果、配置提交参数），不对准实现形状。

const baseStatus: SyncStatus = {
  device_id: 'device-abcdef',
  channel_configured: true,
  last_sync_at: '2026-01-15T08:30:00Z',
  parked_count: 0,
  library_encrypted: false,
}

const baseConfig: SyncChannelConfig = {
  base_url: 'https://dav.example.com/dav/ledger/',
  username: 'alice',
  password: 'app-pass',
  space_id: 'family',
  configured: true,
}

const parkedOp: ParkedOpInfo = {
  op_id: 'op-1',
  device_id: 'device-abcdef',
  entity: 'transaction',
  entity_id: 'txn-1',
  code: 'sync-engine.schema-ahead',
  message: '该操作来自更新版本的应用，升级本端后将自动重试',
  parked_at: '2026-01-15T09:00:00Z',
}

const emptyReport: SyncRoundReport = {
  uploaded_segments: 1,
  uploaded_ops: 2,
  downloaded_segments: 0,
  applied: 0,
  deduped: 0,
  superseded: 0,
  skipped: 0,
  parked: 0,
  plaintext_mode: true,
}

describe('SyncSettings.vue', () => {
  it('挂载即拉取状态与通道配置：上次同步时间、挂起数量回显，明文库警示明文同步', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: baseStatus,
        get_sync_channel_config: baseConfig,
        sync_now: emptyReport,
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('get_sync_status')
    expect(mockInvoke).toHaveBeenCalledWith('get_sync_channel_config')
    const html = wrapper.html()
    expect(html).toContain('2026-01-15 08:30')
    expect(html).not.toContain('从未同步')
    // 明文库：明文警示在场（ADR-0091 决策 8 界面义务），密文提示不在场。
    expect(html).toContain('明文存放于网盘')
    expect(html).not.toContain('密文形态')
  })

  it('密文库：显示密文提示与主口令输入，不再警示明文', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: { ...baseStatus, library_encrypted: true },
        get_sync_channel_config: baseConfig,
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()
    const html = wrapper.html()
    expect(html).toContain('密文形态')
    expect(html).not.toContain('明文存放于网盘')
    expect(findInputByTestId(wrapper, 'sync-passphrase').exists()).toBe(true)
  })

  it('从未同步：回显「从未同步」占位而非空时间', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: { ...baseStatus, last_sync_at: null },
        get_sync_channel_config: baseConfig,
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()
    expect(wrapper.html()).toContain('从未同步')
  })

  it('立即同步：携带口令参数调用命令，成功提示轮次报告并刷新状态（双断言）', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: baseStatus,
        get_sync_channel_config: baseConfig,
        sync_now: { ...emptyReport, applied: 3 },
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()
    mockInvoke.mockClear()

    await findButtonByTestId(wrapper, 'sync-now').trigger('click')
    await flushPromises()

    expect(mockInvoke).toHaveBeenCalledWith('sync_now', { passphrase: null })
    expect(lastInvokeArgs('sync_now')).toEqual({ passphrase: null })
    expect(
      messageCalls().some(
        (m) => m.method === 'success' && m.text.includes('应用 3 条'),
      ),
    ).toBe(true)
    // 效果断言：同步后状态被重新拉取（上次同步时间刷新通道）。
    expect(mockInvoke).toHaveBeenCalledWith('get_sync_status')
  })

  it('立即同步（密文库 + 已输入口令）：口令随调用传递，不落任何本地存储', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: { ...baseStatus, library_encrypted: true },
        get_sync_channel_config: baseConfig,
        sync_now: emptyReport,
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()
    mockInvoke.mockClear()

    const pass = findInputByTestId(wrapper, 'sync-passphrase')
    await pass.setValue('master-pass')
    await findButtonByTestId(wrapper, 'sync-now').trigger('click')
    await flushPromises()

    expect(lastInvokeArgs('sync_now')).toEqual({ passphrase: 'master-pass' })
  })

  it('同步失败：码化错误经 errors.<code> 模板本地化呈现（非透传原文），状态不变', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: baseStatus,
        get_sync_channel_config: baseConfig,
      },
      overrides: {
        // 后端码化错误序列化形态：message 是后端原文，前端应按 errors.<code>
        // 模板（settings.json 真实码表）渲染而非透传。
        sync_now: () =>
          Promise.reject({ kind: 'Invalid', code: 'sync-channel.not-configured', message: 'RAW' }),
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()
    mockInvoke.mockClear()

    await findButtonByTestId(wrapper, 'sync-now').trigger('click')
    await flushPromises()

    expect(
      messageCalls().some(
        (m) => m.method === 'error' && m.text.includes('同步通道尚未配置，请先在设置中填写网盘信息'),
      ),
    ).toBe(true)
    expect(messageCalls().some((m) => m.text.includes('RAW'))).toBe(false)
    expect(mockInvoke).not.toHaveBeenCalledWith('get_sync_status')
  })

  it('保存通道配置：表单值作为参数整体提交，成功提示并刷新状态', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: baseStatus,
        get_sync_channel_config: baseConfig,
        set_sync_channel_config: null,
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()
    mockInvoke.mockClear()

    await findInputByTestId(wrapper, 'sync-url').setValue('https://new.example.com/dav/')
    await findButtonByTestId(wrapper, 'sync-save-channel').trigger('click')
    await flushPromises()

    expect(lastInvokeArgs('set_sync_channel_config')).toEqual({
      config: {
        base_url: 'https://new.example.com/dav/',
        username: 'alice',
        password: 'app-pass',
        space_id: 'family',
      },
    })
    expect(
      messageCalls().some((m) => m.method === 'success' && m.text.includes('已保存')),
    ).toBe(true)
    expect(mockInvoke).toHaveBeenCalledWith('get_sync_status')
  })

  it('保存通道配置失败（码化错误透出）：错误提示，配置不误报已保存', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: baseStatus,
        get_sync_channel_config: baseConfig,
      },
      overrides: {
        set_sync_channel_config: () =>
          Promise.reject({ kind: 'Invalid', code: 'sync-channel.base-url-missing', message: 'RAW' }),
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()

    await findInputByTestId(wrapper, 'sync-url').setValue('')
    await findButtonByTestId(wrapper, 'sync-save-channel').trigger('click')
    await flushPromises()

    expect(
      messageCalls().some((m) => m.method === 'error' && m.text.includes('同步通道地址不能为空')),
    ).toBe(true)
    expect(
      messageCalls().some((m) => m.method === 'success'),
    ).toBe(false)
  })

  // ---- 挂起通知（issue #863 验收项）----

  it('挂起数量 > 0：拉取明细并按码化原因本地化呈现（非透传原文）', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: { ...baseStatus, parked_count: 1 },
        get_sync_channel_config: baseConfig,
        get_parked_ops: [{ ...parkedOp, message: 'RAW' }],
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()

    expect(mockInvoke).toHaveBeenCalledWith('get_parked_ops')
    const list = wrapper.find('[data-testid="sync-parked-list"]')
    expect(list.exists()).toBe(true)
    // 码化原因经 errors.<code> 模板本地化，原文不出现（未知码才降级透传）。
    expect(list.text()).toContain('该操作来自更新版本的应用')
    expect(list.text()).not.toContain('RAW')
  })

  it('无挂起：不拉明细也不渲染挂起清单（零无谓 IPC）', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: { ...baseStatus, parked_count: 0 },
        get_sync_channel_config: baseConfig,
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()

    expect(mockInvoke).not.toHaveBeenCalledWith('get_parked_ops')
    expect(wrapper.find('[data-testid="sync-parked-list"]').exists()).toBe(false)
  })

  it('手动同步后出现挂起：轮次报告驱动挂起提示，并刷新状态与明细', async () => {
    // 同步前的状态无挂起、同步后回显 1 条挂起（真实后端语义：轮次把 op 挂起，
    // 随后的状态查询即反映新数量）——刷新明细的触发条件由此成立。
    let parkedCount = 0
    wireInvokeSeam({
      defaults: {
        get_sync_channel_config: baseConfig,
        get_parked_ops: [parkedOp],
        sync_now: { ...emptyReport, parked: 1 },
      },
      overrides: {
        get_sync_status: () =>
          Promise.resolve({ ...baseStatus, parked_count: parkedCount }),
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()
    mockInvoke.mockClear()
    parkedCount = 1

    await findButtonByTestId(wrapper, 'sync-now').trigger('click')
    await flushPromises()

    expect(
      messageCalls().some(
        (m) => m.method === 'warning' && m.text.includes('1 条操作无法在本机执行'),
      ),
    ).toBe(true)
    // 效果断言：状态与挂起明细均被重新拉取（挂起通知可见通道）。
    expect(mockInvoke).toHaveBeenCalledWith('get_sync_status')
    expect(mockInvoke).toHaveBeenCalledWith('get_parked_ops')
  })
})
