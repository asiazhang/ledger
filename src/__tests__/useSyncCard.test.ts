import { afterEach, describe, it, expect, vi } from 'vitest'
import { mockInvoke, wireInvokeSeam, lastInvokeArgs } from '@ledger/test-support/invoke-mock'
import { messageCalls } from '@ledger/test-support/message-mock'
import { mount } from '@vue/test-utils'
import { defineComponent } from 'vue'
import type { SyncChannelConfig } from '@ledger/types'

import {
  makeParkedOp,
  makeSyncChannelConfig,
  makeSyncRoundReport,
  makeSyncStatus,
} from './factories'
import { registerToastSink } from '@ledger/loadable'
import { makeFakeSink, resetToastSink } from './factories'
import { useSyncCard } from '@/settings/useSyncCard'
import { restartAppShortly } from '@/backup/restart'

// useSyncCard 模块测试（issue #1397，ADR-0041 决策 10 测试归属转移）：宿主组件
// 模式承载 composable 生命周期与 onMounted 首刷（useBackup.test.ts 先例），断言
// 只打模块返回面（状态终态、发出的调用参数、toast/重启编排），不断言内部标志位。
// toast 双通道：成功与 warning 经 useMessage（messageCalls 断言），失败经 Loadable
// 默认策略的模块级 sink（makeFakeSink 断言）。
//
// invoke 时序与重启内部编排归既有接缝测试（restart.test.ts）；组件渲染冒烟归
// SyncSettings.test.ts。

// 引导成功后的原位重引导走 composables/restart 单点（Restore 同型）；模块测试只断言
// 「成功即触发重启编排」。
vi.mock('@/backup/restart', () => ({ restartAppShortly: vi.fn() }))

// 「失败反馈走 Loadable 错误通道」：每测把模块级 sink 复位回 no-op，避免用例间串味。
afterEach(() => resetToastSink())

const checkpointInfo = {
  generation: 3,
  size: 2 * 1024 * 1024,
  created_at: '2026-01-15T08:00:00Z',
}

// 发布结果（明文库 → plaintext_mode 真）。
const publishResult = {
  generation: 3,
  size: 2 * 1024 * 1024,
  plaintext_mode: true,
}

const bootstrapOutcome = {
  generation: 3,
  size: 2 * 1024 * 1024,
  reencrypted: false,
}

/** 承载 composable 生命周期的宿主组件：setup 中捕获返回值供断言。 */
function mountHost() {
  let card!: ReturnType<typeof useSyncCard>
  const Host = defineComponent({
    setup() {
      card = useSyncCard()
      return () => null
    },
  })
  mount(Host)
  return card
}

describe('useSyncCard 状态回显与挂起明细（issue #862 / #863）', () => {
  it('挂载即并行拉取状态与通道配置：status 与 lastSyncText 回显', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.status.value).not.toBeNull())

    expect(mockInvoke).toHaveBeenCalledWith('get_sync_status')
    expect(mockInvoke).toHaveBeenCalledWith('get_sync_channel_config')
    expect(card.status.value).toEqual(makeSyncStatus())
    // 上次同步时刻 ISO → 本地可读截断；「从未同步」占位不在场。
    expect(card.lastSyncText.value).toContain('2026-01-15 08:30')
    expect(card.lastSyncText.value).not.toContain('从未同步')
  })

  it('从未同步：lastSyncText 回显「从未同步」占位而非空时间', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus({ last_sync_at: null }),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.status.value).not.toBeNull())
    expect(card.lastSyncText.value).toBe('从未同步')
  })

  it('状态读取失败：失败 toast 收口 Loadable 通道（裸码化错误，不再带「同步状态读取失败」前缀）', async () => {
    const sink = makeFakeSink()
    registerToastSink(sink)
    wireInvokeSeam({
      defaults: { get_sync_channel_config: makeSyncChannelConfig() },
      overrides: {
        get_sync_status: () =>
          Promise.reject({ kind: 'Invalid', code: 'sync-channel.not-configured', message: 'RAW' }),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(sink.error).toHaveBeenCalled())

    // 码化错误经 errors.<code> 模板本地化呈现（非透传原文）；收口后裸码化错误，
    // 前缀「同步状态读取失败：」退役（issue #1397 显式记录的等价例外）。
    expect(sink.error).toHaveBeenCalledWith(
      '同步通道尚未配置，请先在设置中填写同步通道信息',
    )
    expect(sink.error).not.toHaveBeenCalledWith(expect.stringContaining('RAW'))
    expect(sink.error).not.toHaveBeenCalledWith(expect.stringContaining('同步状态读取失败'))
    // 状态保持未就绪（不误显旧值）。
    expect(card.status.value).toBeNull()
  })

  it('挂起数量 > 0：按需拉取明细，原始 op 内化为模块状态（渲染侧经 errorMessage 单点插值）', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus({ parked_count: 1 }),
        get_sync_channel_config: makeSyncChannelConfig(),
        get_parked_ops: [makeParkedOp({ message: 'RAW' })],
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.parkedOps.value).toHaveLength(1))

    expect(mockInvoke).toHaveBeenCalledWith('get_parked_ops')
    // 模块交付原始 op（拉取原样内化）；码化插值断言打模块产出 × 单点插值表达式
    //（与渲染侧同源）：带参码插出动态值、原文不透传（issue #957 缺陷回归）。
    const op = card.parkedOps.value[0]
    expect(op.op_id).toBe('op-1')
  })

  it('挂起带参码：经码化模板插出动态值（issue #957 回归，模块面）', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus({ parked_count: 1 }),
        get_sync_channel_config: makeSyncChannelConfig(),
        // 真实主路径形态：对端 op 引用不存在账户，重放被账户存活守卫拒绝。
        // message 置为哨兵值：只有「params 插值走通模板」才能得到 acc-1；
        // 若 params 丢失，errorMessage 守卫会回退透传 message，断言即失败。
        get_parked_ops: [
          makeParkedOp({ code: 'account.not-found', params: ['acc-1'], message: 'RAW' }),
        ],
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.parkedOps.value).toHaveLength(1))

    const { errorMessage } = await import('@ledger/utils/errors')
    const rendered = errorMessage(card.parkedOps.value[0])
    expect(rendered).toContain('acc-1')
    expect(rendered).not.toContain('{0}')
    expect(rendered).not.toContain('RAW')
  })

  it('无挂起：不拉明细（零无谓 IPC），明细保持空', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus({ parked_count: 0 }),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.status.value).not.toBeNull())

    expect(mockInvoke).not.toHaveBeenCalledWith('get_parked_ops')
    expect(card.parkedOps.value).toEqual([])
  })

  it('明细拉取失败：console.warn 降级、明细置空，状态回显不受影响（刻意降级不收编）', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    wireInvokeSeam({
      defaults: { get_sync_channel_config: makeSyncChannelConfig() },
      overrides: {
        get_sync_status: makeSyncStatus({ parked_count: 1 }),
        get_parked_ops: () => Promise.reject(new Error('boom')),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.status.value).not.toBeNull())

    expect(card.parkedOps.value).toEqual([])
    expect(warn).toHaveBeenCalledWith('挂起明细拉取失败', expect.anything())
    warn.mockRestore()
  })
})

describe('useSyncCard 立即同步（issue #862）', () => {
  it('缺省口令：sync_now 不带口令（后端回退本机记住口令），成功提示轮次报告并刷新状态（双断言）', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
        sync_now: makeSyncRoundReport({ applied: 3 }),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.status.value).not.toBeNull())
    mockInvoke.mockClear()

    await card.syncNow()

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

  it('密文库已输入口令：口令随调用传递，不落任何本地存储', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus({ library_encrypted: true }),
        get_sync_channel_config: makeSyncChannelConfig(),
        sync_now: makeSyncRoundReport(),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.status.value).not.toBeNull())
    mockInvoke.mockClear()

    card.passphrase.value = 'master-pass'
    await card.syncNow()

    expect(lastInvokeArgs('sync_now')).toEqual({ passphrase: 'master-pass' })
  })

  it('同步失败：码化错误经 Loadable 通道本地化呈现（非透传原文），状态不重拉', async () => {
    const sink = makeFakeSink()
    registerToastSink(sink)
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
      overrides: {
        sync_now: () =>
          Promise.reject({ kind: 'Invalid', code: 'sync-channel.not-configured', message: 'RAW' }),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.status.value).not.toBeNull())
    mockInvoke.mockClear()

    await card.syncNow()

    expect(sink.error).toHaveBeenCalledWith(
      '同步通道尚未配置，请先在设置中填写同步通道信息',
    )
    expect(sink.error).not.toHaveBeenCalledWith(expect.stringContaining('RAW'))
    expect(mockInvoke).not.toHaveBeenCalledWith('get_sync_status')
  })

  it('手动同步后出现挂起：轮次报告驱动挂起警告，并刷新状态与明细（挂起通知可见通道）', async () => {
    // 同步前的状态无挂起、同步后回显 1 条挂起（真实后端语义：轮次把 op 挂起，
    // 随后的状态查询即反映新数量）——刷新明细的触发条件由此成立。
    let parkedCount = 0
    wireInvokeSeam({
      defaults: {
        get_sync_channel_config: makeSyncChannelConfig(),
        get_parked_ops: [makeParkedOp()],
        sync_now: makeSyncRoundReport({ parked: 1 }),
      },
      overrides: {
        get_sync_status: () =>
          Promise.resolve(makeSyncStatus({ parked_count: parkedCount })),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.status.value).not.toBeNull())
    mockInvoke.mockClear()
    parkedCount = 1

    await card.syncNow()

    expect(
      messageCalls().some(
        (m) => m.method === 'warning' && m.text.includes('1 条操作无法在本机执行'),
      ),
    ).toBe(true)
    expect(mockInvoke).toHaveBeenCalledWith('get_sync_status')
    expect(mockInvoke).toHaveBeenCalledWith('get_parked_ops')
  })
})

describe('useSyncCard 通道配置（issue #1218）', () => {
  it('保存通道配置：表单落到后端，回显落库值（删除该接线即表单只留用户敲的原串）', async () => {
    // 后端替身带状态：保存后回显「落库并归一化」的结果（端点去尾斜杠）。这样
    // 「表单显示的是后端存下来的值」成为可观察判据——接线断掉时表单只留
    // 用户敲的原始串，本用例即变红（负向条目，见 PR 正文）。
    let stored: SyncChannelConfig = makeSyncChannelConfig()
    wireInvokeSeam({
      defaults: { get_sync_status: makeSyncStatus() },
      overrides: {
        get_sync_channel_config: () => stored,
        set_sync_channel_config: (args) => {
          const input = (args as { config: Partial<SyncChannelConfig> }).config
          stored = {
            ...stored,
            ...input,
            endpoint: String(input.endpoint).replace(/\/+$/, ''),
            configured: true,
          }
          return null
        },
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.form.value.endpoint).not.toBe(''))

    card.form.value.endpoint = 'https://s3.example.org/'
    card.form.value.bucket = 'new-bucket'
    await card.saveChannel()

    expect(lastInvokeArgs('set_sync_channel_config')).toEqual({
      config: {
        space_id: 'family',
        endpoint: 'https://s3.example.org/',
        region: 'us-east-1',
        bucket: 'new-bucket',
        prefix: 'sync',
        access_key: 'AKIAEXAMPLE',
        secret_key: 'secret-value',
        path_style: true,
      },
    })
    // 效果断言（用户可观察）：表单显示后端存下来的归一化端点，而非用户敲的原串。
    expect(card.form.value.endpoint).toBe('https://s3.example.org')
    expect(card.form.value.bucket).toBe('new-bucket')
    expect(
      messageCalls().some((m) => m.method === 'success' && m.text.includes('已保存')),
    ).toBe(true)
  })

  it('保存通道配置失败（非 https 端点被拒）：码化错误本地化呈现，不误报已保存', async () => {
    const sink = makeFakeSink()
    registerToastSink(sink)
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
      overrides: {
        set_sync_channel_config: () =>
          Promise.reject({ kind: 'Invalid', code: 'sync-channel.endpoint-insecure', message: 'RAW' }),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.form.value.endpoint).not.toBe(''))

    card.form.value.endpoint = 'http://s3.example.com'
    await card.saveChannel()

    expect(sink.error).toHaveBeenCalledWith('同步通道端点必须使用 https 地址')
    expect(sink.error).not.toHaveBeenCalledWith(expect.stringContaining('RAW'))
    expect(
      messageCalls().some((m) => m.method === 'success'),
    ).toBe(false)
  })

  it('密钥留空保存：沿用已保存密钥，不逼迫用户重输（「加载时不回显完整密钥」前提下的机制）', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
        set_sync_channel_config: null,
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.form.value.endpoint).not.toBe(''))

    expect(card.secretKeyInput.value).toBe('')
    await card.saveChannel()

    const sent = lastInvokeArgs('set_sync_channel_config') as { config: { secret_key: string } }
    expect(sent.config.secret_key).toBe('secret-value')
    // 效果断言（双断言）：保存真的发生了——成功提示在场。
    expect(
      messageCalls().some((m) => m.method === 'success' && m.text.includes('已保存')),
    ).toBe(true)
  })

  it('输入新密钥保存：提交新值，保存回显后密钥缓冲清回空串（密钥永不上屏）', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
        set_sync_channel_config: null,
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.form.value.endpoint).not.toBe(''))

    card.secretKeyInput.value = 'rotated-secret'
    await card.saveChannel()

    const sent = lastInvokeArgs('set_sync_channel_config') as { config: { secret_key: string } }
    expect(sent.config.secret_key).toBe('rotated-secret')
    // 保存成功后的回显同样清回空白（密钥永不上屏）。
    expect(card.secretKeyInput.value).toBe('')
  })
})

describe('useSyncCard 测试连接（issue #1219，ADR-0040 默认策略）', () => {
  it('探测用当前表单快照（含未保存改动与新密钥），成功提示通道可读且不落库', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
        test_sync_channel_connection: null,
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.form.value.endpoint).not.toBe(''))

    // 探测针对的是表单现状，不是落库配置：改桶、改密钥后点按钮，发的就是新值。
    card.form.value.bucket = 'other-bucket'
    card.secretKeyInput.value = 'rotated-secret'
    await card.testConnection()

    expect(lastInvokeArgs('test_sync_channel_connection')).toEqual({
      config: {
        space_id: 'family',
        endpoint: 'https://s3.example.com',
        region: 'us-east-1',
        bucket: 'other-bucket',
        prefix: 'sync',
        access_key: 'AKIAEXAMPLE',
        secret_key: 'rotated-secret',
        path_style: true,
      },
    })
    // 双断言（效果面）：成功提示在场，且**没有**顺带保存——探测不落库。
    expect(
      messageCalls().some((m) => m.method === 'success' && m.text.includes('连接成功')),
    ).toBe(true)
    expect(mockInvoke).not.toHaveBeenCalledWith('set_sync_channel_config', expect.anything())
  })

  it('探测与厂商预填共存：探测读的是预填后的同一份表单，且不落库', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
        test_sync_channel_connection: null,
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.form.value.endpoint).not.toBe(''))

    // 先走 #1220 的预填路径（选中厂商 → 预填端点/地域/寻址方式），再探测。
    card.onVendorChange('aliyun-oss')
    await card.testConnection()

    const sent = lastInvokeArgs('test_sync_channel_connection') as {
      config: { endpoint: string; region: string; path_style: boolean }
    }
    expect(sent.config.endpoint).toBe('https://s3.oss-cn-hangzhou.aliyuncs.com')
    expect(sent.config.region).toBe('cn-hangzhou')
    expect(sent.config.path_style).toBe(false)
    // 共存语义：预填只写表单、探测只读表单，两者都不落库（无保存调用）。
    expect(mockInvoke).not.toHaveBeenCalledWith('set_sync_channel_config', expect.anything())
  })

  it('探测失败：按后端分层码本地化呈现可自救提示（fake sink / error 置位，不透传原文）', async () => {
    const sink = makeFakeSink()
    registerToastSink(sink)
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
      overrides: {
        // 403 + AccessDenied 的域侧分层码：与「凭据被拒」是两条不同的自救指引。
        test_sync_channel_connection: () =>
          Promise.reject({
            kind: 'Invalid',
            code: 'sync-channel.permission-denied',
            message: 'RAW',
          }),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.form.value.endpoint).not.toBe(''))

    await card.testConnection()

    expect(sink.error).toHaveBeenCalledWith(
      '同步通道权限不足，请检查当前凭据是否具备该桶的读取与写入权限',
    )
    expect(sink.error).not.toHaveBeenCalledWith('RAW')
    expect(messageCalls().some((m) => m.method === 'success')).toBe(false)
  })
})

describe('useSyncCard 厂商预设与端点反查（issue #1220）', () => {
  it('选中厂商：预填端点模板、默认地域与寻址方式；常用地域快捷项即换端点与地域', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.form.value.endpoint).not.toBe(''))

    card.onVendorChange('aliyun-oss')
    expect(card.form.value.endpoint).toBe('https://s3.oss-cn-hangzhou.aliyuncs.com')
    expect(card.form.value.region).toBe('cn-hangzhou')
    expect(card.form.value.path_style).toBe(false)
    expect(card.selectedVendor.value).toBe('aliyun-oss')
    expect(card.selectedVendorPreset.value?.name).toBe('阿里云 OSS')

    // 地域快捷项：点一下即换端点与地域（随后仍可手改）。
    card.applyVendorRegion('cn-beijing')
    expect(card.form.value.endpoint).toBe('https://s3.oss-cn-beijing.aliyuncs.com')
    expect(card.form.value.region).toBe('cn-beijing')
  })

  it('寻址方式随厂商预填（华为 OBS 虚拟托管 → false，七牛 Kodo → path-style true）', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.form.value.endpoint).not.toBe(''))

    // baseConfig.path_style = true，选一家虚拟托管厂商后应被预填覆盖为 false。
    card.onVendorChange('huawei-obs')
    expect(card.form.value.path_style).toBe(false)

    card.onVendorChange('qiniu-kodo')
    expect(card.form.value.path_style).toBe(true)
  })

  it('再次打开按端点反查回显厂商；未命中显示「其他（自定义）」', async () => {
    // 命中：端点属于腾讯云 COS（虚拟托管形态也命中）。
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig({
          endpoint: 'https://bucket-1250000000.cos.ap-guangzhou.myqcloud.com',
        }),
      },
    })
    const hit = mountHost()
    await vi.waitFor(() => expect(hit.form.value.endpoint).not.toBe(''))
    expect(hit.selectedVendor.value).toBe('tencent-cos')
    expect(hit.selectedVendorPreset.value?.name).toBe('腾讯云 COS')

    // 未命中：自建服务回「其他（自定义）」，无厂商预设可展示。
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig({ endpoint: 'https://minio.internal:9000' }),
      },
    })
    const miss = mountHost()
    await vi.waitFor(() => expect(miss.form.value.endpoint).not.toBe(''))
    expect(miss.selectedVendor.value).toBe('custom')
    expect(miss.selectedVendorPreset.value).toBeNull()
  })

  it('预设不落库：选中厂商只改表单，提交载荷不含任何厂商字段', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
        set_sync_channel_config: null,
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.form.value.endpoint).not.toBe(''))

    card.onVendorChange('aliyun-oss')
    await card.saveChannel()

    // toEqual 是整体形状断言：多出 vendor / preset 一类字段即变红。
    expect(lastInvokeArgs('set_sync_channel_config')).toEqual({
      config: {
        space_id: 'family',
        endpoint: 'https://s3.oss-cn-hangzhou.aliyuncs.com',
        region: 'cn-hangzhou',
        bucket: 'ledger-bucket',
        prefix: 'sync',
        access_key: 'AKIAEXAMPLE',
        secret_key: 'secret-value',
        path_style: false,
      },
    })
  })
})

describe('useSyncCard 检查点发布与新端引导（issue #864）', () => {
  it('发布检查点：携带口令参数（与立即同步共用口令框），成功提示代数与体积', async () => {
    wireInvokeSeam({
      defaults: {
        // 密文库：口令框与 sync_now 共用同一输入。
        get_sync_status: makeSyncStatus({ library_encrypted: true }),
        get_sync_channel_config: makeSyncChannelConfig(),
        publish_sync_checkpoint: publishResult,
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.status.value).not.toBeNull())

    card.passphrase.value = 'master-pass'
    await card.publishCheckpoint()

    expect(lastInvokeArgs('publish_sync_checkpoint')).toEqual({ passphrase: 'master-pass' })
    expect(
      messageCalls().some(
        (m) => m.method === 'success' && m.text.includes('第 3 代') && m.text.includes('2.0 MB'),
      ),
    ).toBe(true)
    // 明文库发布：明文显著提示（ADR-0091 决策 8）。
    expect(
      messageCalls().some(
        (m) => m.method === 'warning' && m.text.includes('明文存放于对象存储'),
      ),
    ).toBe(true)
  })

  it('引导预检发现检查点：checkpointInfo 落位（module 自持）、弹窗开、口令清空', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
        get_sync_channel_checkpoint: checkpointInfo,
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.status.value).not.toBeNull())
    card.bootstrapPassphrase.value = 'stale-pass'

    await card.openBootstrap()

    expect(mockInvoke).toHaveBeenCalledWith('get_sync_channel_checkpoint')
    expect(card.bootstrapShow.value).toBe(true)
    expect(card.bootstrapPassphrase.value).toBe('')
    expect(card.checkpointInfo.value).toEqual(checkpointInfo)
    expect(card.precheckError.value).toBeNull()
  })

  it('确认引导：携带口令整库换入，成功后关弹窗、提示并触发原位重引导（Restore 同型）', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
        get_sync_channel_checkpoint: checkpointInfo,
        bootstrap_sync_from_channel: bootstrapOutcome,
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.status.value).not.toBeNull())
    await card.openBootstrap()
    mockInvoke.mockClear()

    card.bootstrapPassphrase.value = 'master-pass'
    await card.confirmBootstrap()

    expect(lastInvokeArgs('bootstrap_sync_from_channel')).toEqual({ passphrase: 'master-pass' })
    expect(
      messageCalls().some((m) => m.method === 'success' && m.text.includes('引导完成')),
    ).toBe(true)
    expect(card.bootstrapShow.value).toBe(false)
    // 重启编排（Restore 同型）：引导成功即触发原位重引导。
    expect(restartAppShortly).toHaveBeenCalled()
  })

  it('引导失败：码化错误本地化呈现，弹窗保持打开可就地重试，不触发重启', async () => {
    const sink = makeFakeSink()
    registerToastSink(sink)
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
        get_sync_channel_checkpoint: checkpointInfo,
      },
      overrides: {
        bootstrap_sync_from_channel: () =>
          Promise.reject({
            kind: 'Invalid',
            code: 'sync-engine.bootstrap-not-fresh',
            params: ['sync_ops'],
            message: 'RAW',
          }),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.status.value).not.toBeNull())
    await card.openBootstrap()

    await card.confirmBootstrap()

    // 码化错误 + params 插值；收口后裸码化错误，「引导失败：」前缀退役（等价例外）。
    expect(sink.error).toHaveBeenCalledWith(expect.stringContaining('已参与同步'))
    expect(sink.error).not.toHaveBeenCalledWith(expect.stringContaining('RAW'))
    expect(sink.error).not.toHaveBeenCalledWith(expect.stringContaining('引导失败：'))
    // 弹窗保持打开（口令/形态问题就地重试），重启编排未触发。
    expect(card.bootstrapShow.value).toBe(true)
    expect(restartAppShortly).not.toHaveBeenCalled()
    // busy 收尾：bootstrapping 归零，确认按钮可再次发起。
    expect(card.bootstrapping.value).toBe(false)
  })

  it('通道上没有检查点：checkpointInfo 为空，确认不发引导', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
        get_sync_channel_checkpoint: null,
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.status.value).not.toBeNull())

    await card.openBootstrap()
    expect(card.checkpointInfo.value).toBeNull()
    expect(card.precheckError.value).toBeNull()

    mockInvoke.mockClear()
    await card.confirmBootstrap()

    expect(mockInvoke).not.toHaveBeenCalledWith('bootstrap_sync_from_channel', expect.anything())
  })

  it('预检失败：silent 实例不弹 toast，precheckError 置位供弹窗错误位渲染', async () => {
    const sink = makeFakeSink()
    registerToastSink(sink)
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
      overrides: {
        get_sync_channel_checkpoint: () =>
          Promise.reject({ kind: 'Invalid', code: 'sync-channel.not-configured', message: 'RAW' }),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.status.value).not.toBeNull())

    await card.openBootstrap()

    expect(card.precheckError.value).toBe(
      '同步通道尚未配置，请先在设置中填写同步通道信息',
    )
    expect(card.checkpointInfo.value).toBeNull()
    expect(sink.error).not.toHaveBeenCalled()
  })

  it('失败后再次打开：重新预检成功即清错误位并落位检查点（无陈态残留）', async () => {
    let failPrecheck = true
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
      overrides: {
        get_sync_channel_checkpoint: () =>
          failPrecheck
            ? Promise.reject({ kind: 'Invalid', code: 'sync-channel.not-configured', message: 'RAW' })
            : Promise.resolve(checkpointInfo),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.status.value).not.toBeNull())

    await card.openBootstrap()
    expect(card.precheckError.value).not.toBeNull()

    failPrecheck = false
    await card.openBootstrap()
    expect(card.precheckError.value).toBeNull()
    expect(card.checkpointInfo.value).toEqual(checkpointInfo)
  })

  it('引导在途重入：bootstrapping 期间重复确认（enter 键路径）不重复发起', async () => {
    let resolveBootstrap!: (v: typeof bootstrapOutcome) => void
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
        get_sync_channel_checkpoint: checkpointInfo,
      },
      overrides: {
        bootstrap_sync_from_channel: () =>
          new Promise<typeof bootstrapOutcome>((resolve) => {
            resolveBootstrap = resolve
          }),
      },
    })
    const card = mountHost()
    await vi.waitFor(() => expect(card.status.value).not.toBeNull())
    await card.openBootstrap()

    const first = card.confirmBootstrap()
    const second = card.confirmBootstrap()
    resolveBootstrap(bootstrapOutcome)
    await Promise.all([first, second])

    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'bootstrap_sync_from_channel')
    expect(calls).toHaveLength(1)
  })
})
