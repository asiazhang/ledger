import { describe, it, expect } from 'vitest'
import {
  CUSTOM_VENDOR_ID,
  REGION_PLACEHOLDER,
  S3_VENDOR_PRESETS,
  VENDOR_UNVERIFIED_KEY,
  VENDOR_VERIFIED_KEY,
  findVendorPreset,
  matchVendorByEndpoint,
  vendorOptions,
  vendorPrefill,
  vendorTierKey,
} from '@/utils/s3-vendors'

/**
 * 厂商预设纯函数（issue #1220，父 spec #1214）。
 *
 * 本票验收把「预填与反查」的覆盖明确归到纯函数单测：下拉项构成、命中预填、端点
 * 命中与未命中都在这里钉死；组件侧只验证渲染与「预设不落库」。
 */
describe('vendorOptions 下拉项构成', () => {
  it('末尾固定「其他（自定义）」，其余全是预设厂商', () => {
    const options = vendorOptions()
    expect(options[options.length - 1]).toEqual({
      id: CUSTOM_VENDOR_ID,
      name: '',
      custom: true,
      verified: false,
    })
    expect(options.slice(0, -1).every((o) => !o.custom)).toBe(true)
    expect(options.filter((o) => o.custom)).toHaveLength(1)
  })

  it('预设项按声明序出现且带各自档位', () => {
    const options = vendorOptions().slice(0, -1)
    expect(options.map((o) => o.id)).toEqual(S3_VENDOR_PRESETS.map((v) => v.id))
    expect(options.map((o) => o.name)).toEqual(S3_VENDOR_PRESETS.map((v) => v.name))
    expect(options.map((o) => o.verified)).toEqual(S3_VENDOR_PRESETS.map((v) => v.verified))
  })
})

describe('vendorPrefill 选中厂商预填', () => {
  it('缺省地域取该厂商常用地域首项，端点按模板替换', () => {
    expect(vendorPrefill('aliyun-oss')).toEqual({
      endpoint: 'https://s3.oss-cn-hangzhou.aliyuncs.com',
      region: 'cn-hangzhou',
      pathStyle: false,
    })
  })

  it('指定常用地域：端点与地域同步替换', () => {
    expect(vendorPrefill('aliyun-oss', 'cn-beijing')).toEqual({
      endpoint: 'https://s3.oss-cn-beijing.aliyuncs.com',
      region: 'cn-beijing',
      pathStyle: false,
    })
  })

  it('未列出的地域回退首项（不把悬空地域写进端点）', () => {
    expect(vendorPrefill('tencent-cos', 'not-a-region')).toEqual(
      vendorPrefill('tencent-cos'),
    )
  })

  it('寻址方式随厂商给出（七牛 Kodo 为 path-style）', () => {
    expect(vendorPrefill('qiniu-kodo')?.pathStyle).toBe(true)
    expect(vendorPrefill('huawei-obs')?.pathStyle).toBe(false)
  })

  it('自定义与未知厂商不预填（字段保持用户已填内容）', () => {
    expect(vendorPrefill(CUSTOM_VENDOR_ID)).toBeNull()
    expect(vendorPrefill('no-such-vendor')).toBeNull()
  })

  it('预填端点与地域自洽：每个厂商的首项都落在自己的模板里', () => {
    for (const vendor of S3_VENDOR_PRESETS) {
      const prefill = vendorPrefill(vendor.id)
      expect(prefill, vendor.id).not.toBeNull()
      expect(prefill!.endpoint, vendor.id).toContain(prefill!.region)
      expect(prefill!.endpoint, vendor.id).toBe(
        vendor.endpointTemplate.replace(REGION_PLACEHOLDER, prefill!.region),
      )
    }
  })
})

describe('matchVendorByEndpoint 端点反查', () => {
  it.each([
    // 阿里云：S3 协议专属端点（预设模板）与原生端点都认。
    ['https://s3.oss-cn-hangzhou.aliyuncs.com', 'aliyun-oss'],
    ['https://bucket.s3.oss-cn-hangzhou.aliyuncs.com', 'aliyun-oss'],
    ['https://oss-cn-hangzhou.aliyuncs.com', 'aliyun-oss'],
    ['https://bucket.oss-cn-hangzhou.aliyuncs.com', 'aliyun-oss'],
    // 腾讯云：COS 的 S3 兼容端点即其原生端点（虚拟托管）。
    ['https://cos.ap-guangzhou.myqcloud.com', 'tencent-cos'],
    ['https://bucket-1250000000.cos.ap-guangzhou.myqcloud.com', 'tencent-cos'],
    ['https://obs.cn-north-4.myhuaweicloud.com', 'huawei-obs'],
    ['https://bucket.obs.cn-north-4.myhuaweicloud.com', 'huawei-obs'],
    // 火山引擎：S3 协议专属端点是 tos-s3-*，与原生 tos-* 不同名。
    ['https://tos-s3-cn-beijing.volces.com', 'volcengine-tos'],
    ['https://bucket.tos-s3-cn-beijing.volces.com', 'volcengine-tos'],
    ['https://tos-cn-beijing.volces.com', 'volcengine-tos'],
    ['https://s3.cn-east-1.qiniucs.com', 'qiniu-kodo'],
    ['https://my-bucket.s3.cn-east-1.qiniucs.com', 'qiniu-kodo'],
  ])('命中：%s → %s', (endpoint, expected) => {
    expect(matchVendorByEndpoint(endpoint)).toBe(expected)
  })

  it('省略 scheme 与大小写差异同样命中', () => {
    expect(matchVendorByEndpoint('s3.oss-cn-hangzhou.aliyuncs.com')).toBe('aliyun-oss')
    expect(matchVendorByEndpoint('HTTPS://S3.OSS-CN-HANGZHOU.ALIYUNCS.COM')).toBe('aliyun-oss')
  })

  it('未命中一律回「自定义」：自建域名、空串、非法地址、仿冒后缀', () => {
    expect(matchVendorByEndpoint('https://s3.example.com')).toBe(CUSTOM_VENDOR_ID)
    expect(matchVendorByEndpoint('https://minio.internal:9000')).toBe(CUSTOM_VENDOR_ID)
    expect(matchVendorByEndpoint('')).toBe(CUSTOM_VENDOR_ID)
    expect(matchVendorByEndpoint('   ')).toBe(CUSTOM_VENDOR_ID)
    expect(matchVendorByEndpoint('not a url at all')).toBe(CUSTOM_VENDOR_ID)
    // 后缀判据带前导点：仿冒域名不误判成厂商预设。
    expect(matchVendorByEndpoint('https://aliyuncs.com.evil.example')).toBe(CUSTOM_VENDOR_ID)
  })

  it('同品牌的其他服务不算命中（只看品牌后缀会把 ECS/CDN 误判成对象存储）', () => {
    // 服务段前缀 + 品牌后缀两端同时成立才算命中：华为云 ECS、腾讯云 CDN、
    // 火山引擎其他 volces.com 服务都不是对象存储端点。
    expect(matchVendorByEndpoint('https://ecs.cn-north-4.myhuaweicloud.com')).toBe(
      CUSTOM_VENDOR_ID,
    )
    expect(matchVendorByEndpoint('https://cdn.myqcloud.com')).toBe(CUSTOM_VENDOR_ID)
    expect(matchVendorByEndpoint('https://iam.volces.com')).toBe(CUSTOM_VENDOR_ID)
    // 品牌域名本身（无服务段）也不算命中。
    expect(matchVendorByEndpoint('https://myhuaweicloud.com')).toBe(CUSTOM_VENDOR_ID)
    expect(matchVendorByEndpoint('https://aliyuncs.com')).toBe(CUSTOM_VENDOR_ID)
  })
})

describe('vendorTierKey 档位标注映射', () => {
  it('已实测 / 未实测 两分支都有映射', () => {
    expect(vendorTierKey(true)).toBe(VENDOR_VERIFIED_KEY)
    expect(vendorTierKey(false)).toBe(VENDOR_UNVERIFIED_KEY)
  })
})

describe('预设表诚实性与完整性守门', () => {
  it('每条预设必备：专名、模板占位符、常用地域、主机前后缀、https 官方文档', () => {
    expect(S3_VENDOR_PRESETS.length).toBeGreaterThan(0)
    for (const vendor of S3_VENDOR_PRESETS) {
      expect(vendor.name, vendor.id).not.toBe('')
      expect(vendor.endpointTemplate, vendor.id).toContain(REGION_PLACEHOLDER)
      expect(vendor.endpointTemplate, vendor.id).toMatch(/^https:\/\//)
      expect(vendor.regions.length, vendor.id).toBeGreaterThan(0)
      expect(vendor.hostPrefixes.length, vendor.id).toBeGreaterThan(0)
      expect(vendor.hostSuffixes.length, vendor.id).toBeGreaterThan(0)
      expect(vendor.docsUrl, vendor.id).toMatch(/^https:\/\//)
    }
  })

  it('模板与反查判据自洽：每个厂商的首项预填端点都能反查回自己', () => {
    for (const vendor of S3_VENDOR_PRESETS) {
      const prefill = vendorPrefill(vendor.id)
      expect(prefill, vendor.id).not.toBeNull()
      expect(matchVendorByEndpoint(prefill!.endpoint), vendor.id).toBe(vendor.id)
    }
  })

  it('厂商 id 与主机后缀不重复（反查必须唯一命中）', () => {
    const ids = S3_VENDOR_PRESETS.map((v) => v.id)
    expect(new Set(ids).size).toBe(ids.length)
    const suffixes = S3_VENDOR_PRESETS.flatMap((v) => v.hostSuffixes)
    expect(new Set(suffixes).size).toBe(suffixes.length)
  })

  it('未实测前档位恒 false：没有任何厂商被凭空标成「已实测」', () => {
    // #1222 真实桶实测清单落地后，这条断言随之改为「已实测集合与清单一致」。
    expect(S3_VENDOR_PRESETS.every((v) => v.verified === false)).toBe(true)
  })

  it('findVendorPreset 命中与未命中', () => {
    expect(findVendorPreset('aliyun-oss')?.name).toBe('阿里云 OSS')
    expect(findVendorPreset(CUSTOM_VENDOR_ID)).toBeNull()
    expect(findVendorPreset('no-such-vendor')).toBeNull()
  })
})
