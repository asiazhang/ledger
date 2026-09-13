import { describe, it, expect, vi, beforeAll } from 'vitest'
import { mockInvoke, wireInvokeSeam, lastInvokeArgs } from '@ledger/test-support/invoke-mock'
import { messageCalls } from '@ledger/test-support/message-mock'
import {
  findButtonByTestId,
  findInputByTestId,
  findBodyButtonByTestId,
} from '@ledger/test-support/dom'
import { DOMWrapper, mount, flushPromises } from '@vue/test-utils'
import type { ParkedOpInfo, SyncChannelConfig, SyncRoundReport, SyncStatus } from '@ledger/types'

import SyncSettings from '@/components/settings/SyncSettings.vue'

// 引导成功后的原位重引导走 utils/restart 单点（Restore 同型）；组件测试只断言
// 「成功即触发重启编排」，重启内部编排归 restart.test.ts。
vi.mock('@/utils/restart', () => ({ restartAppShortly: vi.fn() }))
import { restartAppShortly } from '@/utils/restart'

// jsdom 未实现元素滚动（naive-ui 下拉菜单打开时会 scrollTo），补空实现避免打断
// Vue 调度队列（仅影响本文件的厂商下拉交互用例，QuickTimeRange.test.ts 先例）。
beforeAll(() => {
  Element.prototype.scrollTo = () => {}
})

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
  backend: 's3',
  base_url: '',
  username: '',
  password: '',
  space_id: 'family',
  endpoint: 'https://s3.example.com',
  region: 'us-east-1',
  bucket: 'ledger-bucket',
  prefix: 'sync',
  access_key: 'AKIAEXAMPLE',
  secret_key: 'secret-value',
  path_style: true,
  configured: true,
}

const parkedOp: ParkedOpInfo = {
  op_id: 'op-1',
  device_id: 'device-abcdef',
  entity: 'transaction',
  entity_id: 'txn-1',
  code: 'sync-engine.schema-ahead',
  params: [],
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

  it('保存通道配置：S3 表单落到后端，表单回显落库值（删除该接线即同步跑不起来）', async () => {
    // 后端替身带状态：保存后回显「落库并归一化」的结果（端点去尾斜杠）。这样
    // 「表单显示的是后端存下来的值」成为用户可观察判据——接线断掉时表单只留
    // 用户敲的原始串，本用例即变红（负向条目，见 PR 正文）。
    // 初态 = 已保存过配置（表单加载出 family 空间等既有值），随后用户改端点与桶再保存。
    let stored: SyncChannelConfig = { ...baseConfig }
    wireInvokeSeam({
      defaults: { get_sync_status: baseStatus },
      overrides: {
        get_sync_channel_config: () => stored,
        set_sync_channel_config: (args) => {
          const input = (args as { config: Partial<SyncChannelConfig> }).config
          stored = {
            ...baseConfig,
            ...input,
            endpoint: String(input.endpoint).replace(/\/+$/, ''),
            configured: true,
          }
          return null
        },
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()

    await findInputByTestId(wrapper, 'sync-endpoint').setValue('https://s3.example.org/')
    await findInputByTestId(wrapper, 'sync-bucket').setValue('new-bucket')
    await findButtonByTestId(wrapper, 'sync-save-channel').trigger('click')
    await flushPromises()

    expect(lastInvokeArgs('set_sync_channel_config')).toEqual({
      config: {
        backend: 's3',
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
    expect(
      (findInputByTestId(wrapper, 'sync-endpoint').element as HTMLInputElement).value,
    ).toBe('https://s3.example.org')
    expect((findInputByTestId(wrapper, 'sync-bucket').element as HTMLInputElement).value).toBe(
      'new-bucket',
    )
    expect(
      messageCalls().some((m) => m.method === 'success' && m.text.includes('已保存')),
    ).toBe(true)
  })

  it('保存通道配置失败（非 https 端点被拒）：码化错误本地化呈现，不误报已保存', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: baseStatus,
        get_sync_channel_config: baseConfig,
      },
      overrides: {
        set_sync_channel_config: () =>
          Promise.reject({ kind: 'Invalid', code: 'sync-channel.endpoint-insecure', message: 'RAW' }),
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()

    await findInputByTestId(wrapper, 'sync-endpoint').setValue('http://s3.example.com')
    await findButtonByTestId(wrapper, 'sync-save-channel').trigger('click')
    await flushPromises()

    expect(
      messageCalls().some(
        (m) => m.method === 'error' && m.text.includes('同步通道端点必须使用 https 地址'),
      ),
    ).toBe(true)
    expect(messageCalls().some((m) => m.text.includes('RAW'))).toBe(false)
    expect(
      messageCalls().some((m) => m.method === 'success'),
    ).toBe(false)
  })

  // ---- S3 表单与密钥回显口径（issue #1218 验收项）----

  it('加载已保存配置：完整密钥不回显进输入框，占位提示留空即保持', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: baseStatus,
        get_sync_channel_config: baseConfig,
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()

    // 密钥是唯一不回显的字段：输入框为空，已保存值不出现在渲染结果里。
    const secret = findInputByTestId(wrapper, 'sync-secret-key')
    expect(secret.exists()).toBe(true)
    expect((secret.element as HTMLInputElement).value).toBe('')
    expect(wrapper.html()).not.toContain('secret-value')
    expect(secret.attributes('placeholder')).toContain('留空则保持不变')
    // 其余 S3 字段照常回显（端点/区域/桶/前缀/Access Key）。
    expect((findInputByTestId(wrapper, 'sync-endpoint').element as HTMLInputElement).value).toBe(
      'https://s3.example.com',
    )
    expect((findInputByTestId(wrapper, 'sync-region').element as HTMLInputElement).value).toBe(
      'us-east-1',
    )
    expect((findInputByTestId(wrapper, 'sync-bucket').element as HTMLInputElement).value).toBe(
      'ledger-bucket',
    )
    expect((findInputByTestId(wrapper, 'sync-prefix').element as HTMLInputElement).value).toBe('sync')
    expect((findInputByTestId(wrapper, 'sync-access-key').element as HTMLInputElement).value).toBe(
      'AKIAEXAMPLE',
    )
  })

  it('密钥留空保存：沿用已保存密钥，不逼迫用户重输', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: baseStatus,
        get_sync_channel_config: baseConfig,
        set_sync_channel_config: null,
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()

    await findButtonByTestId(wrapper, 'sync-save-channel').trigger('click')
    await flushPromises()

    const sent = lastInvokeArgs('set_sync_channel_config') as { config: { secret_key: string } }
    expect(sent.config.secret_key).toBe('secret-value')
    // 效果断言（双断言）：保存真的发生了——成功提示在场。
    expect(
      messageCalls().some((m) => m.method === 'success' && m.text.includes('已保存')),
    ).toBe(true)
  })

  it('输入新密钥保存：提交新值，同时不把旧值渲染上屏', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: baseStatus,
        get_sync_channel_config: baseConfig,
        set_sync_channel_config: null,
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()

    await findInputByTestId(wrapper, 'sync-secret-key').setValue('rotated-secret')
    await findButtonByTestId(wrapper, 'sync-save-channel').trigger('click')
    await flushPromises()

    const sent = lastInvokeArgs('set_sync_channel_config') as { config: { secret_key: string } }
    expect(sent.config.secret_key).toBe('rotated-secret')
    // 保存成功后的回显同样为空白输入框（密钥永不上屏）。
    expect(
      (findInputByTestId(wrapper, 'sync-secret-key').element as HTMLInputElement).value,
    ).toBe('')
  })

  it('未配置通道：S3 空表单起填，密钥占位为普通字段名', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: { ...baseStatus, channel_configured: false },
        get_sync_channel_config: {
          ...baseConfig,
          endpoint: '',
          region: '',
          bucket: '',
          prefix: '',
          access_key: '',
          secret_key: '',
          path_style: false,
          configured: false,
        },
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()

    expect((findInputByTestId(wrapper, 'sync-endpoint').element as HTMLInputElement).value).toBe('')
    // 未保存过密钥时不提示「留空则保持不变」（那会让用户以为已有密钥在位）。
    expect(findInputByTestId(wrapper, 'sync-secret-key').attributes('placeholder')).toBe(
      'Secret Access Key',
    )
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

  it('挂起原因带参码：按 errors.<code> 模板插出动态值（issue #957 缺陷回归）', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: { ...baseStatus, parked_count: 1 },
        get_sync_channel_config: baseConfig,
        // 真实主路径形态：对端 op 引用不存在账户，重放被账户存活守卫拒绝。
        // message 置为哨兵值：只有「params 插值走通模板」才能得到 acc-1；
        // 若 params 丢失，守卫会回退透传 message，断言即失败。
        get_parked_ops: [
          {
            ...parkedOp,
            code: 'account.not-found',
            params: ['acc-1'],
            message: 'RAW',
          },
        ],
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()

    const list = wrapper.find('[data-testid="sync-parked-list"]')
    // 缺陷回归：params 丢失时渲染成「账户不存在: 」（空悬占位符）。
    // 修复后必须插出动态值——挂起明细是用户唯一的裁决依据。
    expect(list.text()).toContain('acc-1')
    expect(list.text()).not.toContain('{0}')
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

// ---------------------------------------------------------------------------
// 检查点发布与新端引导向导（issue #864）
// ---------------------------------------------------------------------------

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

it('未配置通道：发布与引导入口均禁用（引导依赖通道在位）', async () => {
  wireInvokeSeam({
    defaults: {
      get_sync_status: { ...baseStatus, channel_configured: false },
      get_sync_channel_config: { ...baseConfig, configured: false },
    },
  })
  const wrapper = mount(SyncSettings)
  await flushPromises()
  expect(findButtonByTestId(wrapper, 'sync-publish-checkpoint').attributes('disabled')).toBeDefined()
  expect(findButtonByTestId(wrapper, 'sync-bootstrap').attributes('disabled')).toBeDefined()
})

it('发布检查点：携带口令参数调用命令，成功提示代数与体积', async () => {
  wireInvokeSeam({
    defaults: {
      // 密文库：主口令输入框在场（与 sync_now 共用同一输入）。
      get_sync_status: { ...baseStatus, library_encrypted: true },
      get_sync_channel_config: baseConfig,
      publish_sync_checkpoint: publishResult,
    },
  })
  const wrapper = mount(SyncSettings)
  await flushPromises()
  mockInvoke.mockClear()

  await findInputByTestId(wrapper, 'sync-passphrase').setValue('master-pass')
  await findButtonByTestId(wrapper, 'sync-publish-checkpoint').trigger('click')
  await flushPromises()

  expect(lastInvokeArgs('publish_sync_checkpoint')).toEqual({ passphrase: 'master-pass' })
  expect(
    messageCalls().some(
      (m) => m.method === 'success' && m.text.includes('第 3 代') && m.text.includes('2.0 MB'),
    ),
  ).toBe(true)
  // 明文库发布：明文显著提示（ADR-0091 决策 8）。
  expect(
    messageCalls().some(
      (m) => m.method === 'warning' && m.text.includes('明文存放于网盘'),
    ),
  ).toBe(true)
})

it('引导向导：预检发现检查点后确认，携带口令调用引导并原位重引导', async () => {
  wireInvokeSeam({
    defaults: {
      get_sync_status: baseStatus,
      get_sync_channel_config: baseConfig,
      get_sync_channel_checkpoint: checkpointInfo,
      bootstrap_sync_from_channel: bootstrapOutcome,
    },
  })
  const wrapper = mount(SyncSettings)
  await flushPromises()
  mockInvoke.mockClear()

  await findButtonByTestId(wrapper, 'sync-bootstrap').trigger('click')
  await flushPromises()

  // 预检回显（弹窗内容 teleport 到 body）：代数 + 体积 + 后果警示。
  expect(mockInvoke).toHaveBeenCalledWith('get_sync_channel_checkpoint')
  const modalHtml = document.body.querySelector('.n-modal')?.innerHTML ?? ''
  expect(modalHtml).toContain('第 3 代')
  expect(modalHtml).toContain('2.0 MB')
  expect(modalHtml).toContain('整库替换')

  const pass = document.body.querySelector('.n-modal input[type="password"]')!
  await new DOMWrapper<HTMLInputElement>(pass as HTMLInputElement).setValue('master-pass')
  await flushPromises()
  await findBodyButtonByTestId('sync-bootstrap-confirm')!.trigger('click')
  await flushPromises()

  expect(lastInvokeArgs('bootstrap_sync_from_channel')).toEqual({ passphrase: 'master-pass' })
  expect(
    messageCalls().some((m) => m.method === 'success' && m.text.includes('引导完成')),
  ).toBe(true)
  // 重启编排（Restore 同型）：引导成功即触发原位重引导。
  expect(restartAppShortly).toHaveBeenCalled()
})

it('引导向导：通道上没有检查点时展示指引且确认禁用（不发起引导）', async () => {
  wireInvokeSeam({
    defaults: {
      get_sync_status: baseStatus,
      get_sync_channel_config: baseConfig,
      get_sync_channel_checkpoint: null,
    },
  })
  const wrapper = mount(SyncSettings)
  await flushPromises()
  mockInvoke.mockClear()

  await findButtonByTestId(wrapper, 'sync-bootstrap').trigger('click')
  await flushPromises()

  expect(document.body.querySelector('.n-modal')?.innerHTML).toContain('通道上还没有检查点')
  expect(findBodyButtonByTestId('sync-bootstrap-confirm')!.attributes('disabled')).toBeDefined()
  expect(mockInvoke).not.toHaveBeenCalledWith('bootstrap_sync_from_channel')
})

it('引导失败：码化错误本地化呈现且弹窗保持打开（可就地重试）', async () => {
  wireInvokeSeam({
    defaults: {
      get_sync_status: baseStatus,
      get_sync_channel_config: baseConfig,
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
  const wrapper = mount(SyncSettings)
  await flushPromises()
  mockInvoke.mockClear()

  await findButtonByTestId(wrapper, 'sync-bootstrap').trigger('click')
  await flushPromises()
  await findBodyButtonByTestId('sync-bootstrap-confirm')!.trigger('click')
  await flushPromises()

  expect(
    messageCalls().some(
      (m) => m.method === 'error' && m.text.includes('已参与同步'),
    ),
  ).toBe(true)
  expect(messageCalls().some((m) => m.text.includes('RAW'))).toBe(false)
  expect(restartAppShortly).not.toHaveBeenCalled()
})

// ---- 厂商预设下拉与端点反查（issue #1220 验收项）----
//
// 本票把「预填与反查」的命中/未命中覆盖明确归到纯函数单测（见 s3-vendors.test.ts）；
// 这里只验证组件侧的用户可观察行为：下拉项构成（末尾固定自定义）、选中即预填且字段
// 仍可编辑、档位标注与官方文档外链、再次打开按端点反查回显、预设不落库。

describe('SyncSettings.vue 厂商预设（issue #1220）', () => {
  /** 下拉里当前渲染出的选项文本（真实交互：点开选择框，菜单 teleport 到 body）。 */
  async function openVendorMenu(wrapper: ReturnType<typeof mount>): Promise<string[]> {
    await wrapper.find('[data-testid="sync-vendor"] .n-base-selection').trigger('click')
    await flushPromises()
    return Array.from(document.body.querySelectorAll('.n-base-select-option')).map(
      (el) => el.textContent?.trim() ?? '',
    )
  }

  /** 模拟用户从下拉里点选一项（按可见选项文本匹配），点完菜单关闭。 */
  async function pickVendor(wrapper: ReturnType<typeof mount>, labelPart: string) {
    const option = (await openVendorMenu(wrapper)).findIndex((text) => text.includes(labelPart))
    expect(option, `下拉里应能看到「${labelPart}」选项`).toBeGreaterThanOrEqual(0)
    const optionEl = document.body.querySelectorAll('.n-base-select-option')[option]
    await new DOMWrapper(optionEl).trigger('click')
    await flushPromises()
  }

  /** 下拉上显示出的当前选中文本（用户可观察的回显面）。 */
  function selectedVendorText(wrapper: ReturnType<typeof mount>): string {
    return wrapper.find('[data-testid="sync-vendor"]').text()
  }

  it('下拉包含国内主流厂商预设，末尾固定「其他（自定义）」', async () => {
    wireInvokeSeam({
      defaults: { get_sync_status: baseStatus, get_sync_channel_config: baseConfig },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()

    // 入口在位（用户可观察）：通道配置表单里能看到厂商下拉。
    expect(wrapper.find('[data-testid="sync-vendor"]').exists()).toBe(true)

    const options = await openVendorMenu(wrapper)
    expect(options.length).toBeGreaterThan(1)
    // 末尾固定自定义项（验收判据），且它只能出现一次。
    expect(options.at(-1)).toBe('其他（自定义）')
    expect(options.filter((o) => o === '其他（自定义）')).toHaveLength(1)
    // 国内主流厂商在列，选项文本用厂商专名原文 + 档位标注。
    for (const name of ['阿里云 OSS', '腾讯云 COS', '华为云 OBS', '火山引擎 TOS', '七牛云 Kodo']) {
      expect(options.some((o) => o.includes(name) && o.includes('未实测')), name).toBe(true)
    }
  })

  it('选中厂商：预填端点模板、默认地域与寻址方式，字段全部保持可编辑', async () => {
    wireInvokeSeam({
      defaults: { get_sync_status: baseStatus, get_sync_channel_config: baseConfig },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()

    await pickVendor(wrapper, '阿里云 OSS')

    const endpoint = findInputByTestId(wrapper, 'sync-endpoint')
    expect((endpoint.element as HTMLInputElement).value).toBe(
      'https://oss-cn-hangzhou.aliyuncs.com',
    )
    expect((findInputByTestId(wrapper, 'sync-region').element as HTMLInputElement).value).toBe(
      'cn-hangzhou',
    )
    // 预填只写默认值：字段没有被锁死，用户可改成自建或其他兼容服务。
    expect((endpoint.element as HTMLInputElement).disabled).toBe(false)
    expect((findInputByTestId(wrapper, 'sync-region').element as HTMLInputElement).disabled).toBe(
      false,
    )

    // 常用地域快捷项：点一下即换端点与地域（随后仍可手改）。
    const beijing = wrapper
      .findAll('[data-testid="sync-vendor-region"]')
      .find((b) => b.text() === 'cn-beijing')
    expect(beijing).toBeTruthy()
    await beijing?.trigger('click')
    await flushPromises()
    expect((findInputByTestId(wrapper, 'sync-endpoint').element as HTMLInputElement).value).toBe(
      'https://oss-cn-beijing.aliyuncs.com',
    )
    expect((findInputByTestId(wrapper, 'sync-region').element as HTMLInputElement).value).toBe(
      'cn-beijing',
    )
  })

  it('选中厂商：预填寻址方式（七牛 Kodo 为 path-style）', async () => {
    wireInvokeSeam({
      defaults: { get_sync_status: baseStatus, get_sync_channel_config: baseConfig },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()

    // baseConfig.path_style = true，选一家虚拟托管厂商后应被预填覆盖为 false。
    await pickVendor(wrapper, '华为云 OBS')
    expect(wrapper.find('[data-testid="sync-path-style"]').attributes('aria-checked')).toBe('false')

    await pickVendor(wrapper, '七牛云 Kodo')
    expect(wrapper.find('[data-testid="sync-path-style"]').attributes('aria-checked')).toBe('true')
  })

  it('预设在界面上区分「已实测 / 未实测」并给出官方文档外链', async () => {
    wireInvokeSeam({
      defaults: { get_sync_status: baseStatus, get_sync_channel_config: baseConfig },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()

    await pickVendor(wrapper, '阿里云 OSS')

    // 档位标注：目前无任何厂商跑过真实桶，界面如实标「未实测」。
    expect(wrapper.find('[data-testid="sync-vendor-tier"]').text()).toBe('未实测')
    // 官方文档外链：_blank 才能被系统浏览器打开（桌面壳 opener 的既有约定）。
    const docs = wrapper.find('[data-testid="sync-vendor-docs"]')
    expect(docs.attributes('href')).toBe('https://help.aliyun.com/zh/oss/')
    expect(docs.attributes('target')).toBe('_blank')
  })

  it('再次打开按端点反查回显厂商；未命中显示「其他（自定义）」', async () => {
    // 命中：端点属于腾讯云 COS（虚拟托管形态也命中）。
    wireInvokeSeam({
      defaults: {
        get_sync_status: baseStatus,
        get_sync_channel_config: {
          ...baseConfig,
          endpoint: 'https://bucket-1250000000.cos.ap-guangzhou.myqcloud.com',
        },
      },
    })
    const hit = mount(SyncSettings)
    await flushPromises()
    expect(selectedVendorText(hit)).toContain('腾讯云 COS')
    expect(hit.find('[data-testid="sync-vendor-tier"]').text()).toBe('未实测')

    // 未命中：自建服务回「其他（自定义）」，档位与外链区不渲染（无厂商可展示）。
    wireInvokeSeam({
      defaults: {
        get_sync_status: baseStatus,
        get_sync_channel_config: { ...baseConfig, endpoint: 'https://minio.internal:9000' },
      },
    })
    const miss = mount(SyncSettings)
    await flushPromises()
    expect(selectedVendorText(miss)).toContain('其他（自定义）')
    expect(miss.find('[data-testid="sync-vendor-meta"]').exists()).toBe(false)
  })

  it('预设不落库：选中厂商只改表单，提交载荷不含任何厂商字段', async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: baseStatus,
        get_sync_channel_config: baseConfig,
        set_sync_channel_config: null,
      },
    })
    const wrapper = mount(SyncSettings)
    await flushPromises()

    await pickVendor(wrapper, '阿里云 OSS')
    await findButtonByTestId(wrapper, 'sync-save-channel').trigger('click')
    await flushPromises()

    // toEqual 是整体形状断言：多出 vendor / preset 一类字段即变红。
    expect(lastInvokeArgs('set_sync_channel_config')).toEqual({
      config: {
        backend: 's3',
        space_id: 'family',
        endpoint: 'https://oss-cn-hangzhou.aliyuncs.com',
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
