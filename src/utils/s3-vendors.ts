/**
 * S3 兼容厂商预设（issue #1220，父 spec #1214）。
 *
 * 定位：**纯前端展示层数据与纯函数**。预设只服务两件事——选中厂商时预填端点
 * 模板、寻址方式与常用地域，再次打开时按端点反查回显厂商；它不落库、不进后端
 * 契约（`SyncChannelConfig` 不新增字段），预填后的每个字段都保持用户可编辑。
 *
 * 专名与翻译的边界（ADR-0049）：厂商名是专有名词，直接以字面量住在本表、不进
 * 翻译资源；「其他（自定义）」「已实测 / 未实测」「官方文档」等说明性文案与选项
 * 模板文案，由组件经 i18n key 取（本模块只产出 key / 数据，不依赖 i18n）。
 *
 * 档位标注（`verified`）默认全部 false——目前没有任何厂商跑过真实桶，唯一诚实
 * 来源是 #1222 产出的「已实测厂商清单」。**本表 `verified` 字段是唯一的翻转点**：
 * #1222 结论落地后逐家改写该字段，界面标注随之改变，不在别处复制一份档位表。
 */

/** 端点模板里的地域占位符（预填时按所选地域替换）。 */
export const REGION_PLACEHOLDER = '{region}'

/** 「其他（自定义）」哨兵 id：不在预设表内，恒为下拉末项。 */
export const CUSTOM_VENDOR_ID = 'custom'

/** 厂商档位标注的 i18n key（已实测 / 未实测）。 */
export const VENDOR_VERIFIED_KEY = 'settings.data.sync.vendorVerified'
export const VENDOR_UNVERIFIED_KEY = 'settings.data.sync.vendorUnverified'

/** 单个厂商预设。字段全部只读：预设表是数据不是状态，改写只走源码修订。 */
export interface S3VendorPreset {
  /** 稳定判别键（反查与回显的锚点，非用户可见文案）。 */
  readonly id: string
  /** 厂商专名（专有名词，不进翻译资源）。 */
  readonly name: string
  /** 端点模板，含 `{region}` 占位符。 */
  readonly endpointTemplate: string
  /** 默认寻址方式：true = path-style，false = 虚拟托管（bucket 前缀进主机名）。 */
  readonly pathStyle: boolean
  /** 常用地域快捷项（地域代码同样是专名，不翻译）；首项为该厂商预填默认地域。 */
  readonly regions: readonly string[]
  /** 端点主机名的服务段前缀（反查判据之一：`obs.` / `s3.oss-` 一类服务标识，避免同品牌
   *  其他服务误判）。虚拟托管形态下 bucket 是更左的一段，比对时两级都试（见反查函数）。 */
  readonly hostPrefixes: readonly string[]
  /** 端点主机名后缀（反查判据；含前导点，避免匹配到同名后缀的仿冒域名）。 */
  readonly hostSuffixes: readonly string[]
  /** 官方文档外链。 */
  readonly docsUrl: string
  /** 是否已用真实桶实测（#1222 结论的唯一翻转点；未实测前恒 false）。 */
  readonly verified: boolean
}

/**
 * 国内主流对象存储厂商预设表。
 *
 * 端点模板一律取各家公开文档里的 **S3 协议专属端点**（不是各家的原生 REST 端点：
 * 阿里云是 `s3.oss-{region}.aliyuncs.com`、火山引擎是 `tos-s3-{region}.volces.com`，
 * 二者都与原生端点不同名）；地域段由用户在界面上选择或改成自建值。寻址方式填各家
 * 文档的默认（火山引擎 TOS 明确只支持虚拟托管），全部可在界面上改成另一种。**这里
 * 只给常用地域快捷项，不是该厂商的全部地域**——用户可自行填写未列出的地域代码。
 *
 * 「是否接受 AWS SigV4」一类兼容性事实不在本表妄断：未实测厂商一律 `verified:
 * false`，界面上只如实标注「未实测」并给官方文档外链，兼容性结论留给 #1222 的
 * 真实桶实测（spec #1214 已注明阿里云 OSS 是否接受 SigV4 尚待核查）。
 */
export const S3_VENDOR_PRESETS: readonly S3VendorPreset[] = [
  {
    id: 'aliyun-oss',
    name: '阿里云 OSS',
    endpointTemplate: `https://s3.oss-${REGION_PLACEHOLDER}.aliyuncs.com`,
    pathStyle: false,
    regions: ['cn-hangzhou', 'cn-shanghai', 'cn-beijing', 'cn-shenzhen', 'cn-guangzhou', 'cn-chengdu'],
    hostPrefixes: ['s3.oss-', 'oss-'],
    hostSuffixes: ['.aliyuncs.com'],
    docsUrl: 'https://help.aliyun.com/zh/oss/developer-reference/use-amazon-s3-sdks-to-access-oss',
    verified: false,
  },
  {
    id: 'tencent-cos',
    name: '腾讯云 COS',
    endpointTemplate: `https://cos.${REGION_PLACEHOLDER}.myqcloud.com`,
    pathStyle: false,
    regions: ['ap-guangzhou', 'ap-beijing', 'ap-shanghai', 'ap-chengdu', 'ap-hongkong'],
    hostPrefixes: ['cos.'],
    hostSuffixes: ['.myqcloud.com'],
    docsUrl: 'https://cloud.tencent.com/document/product/436',
    verified: false,
  },
  {
    id: 'huawei-obs',
    name: '华为云 OBS',
    endpointTemplate: `https://obs.${REGION_PLACEHOLDER}.myhuaweicloud.com`,
    pathStyle: false,
    regions: ['cn-north-4', 'cn-east-3', 'cn-south-1', 'cn-southwest-2', 'cn-north-1'],
    hostPrefixes: ['obs.'],
    hostSuffixes: ['.myhuaweicloud.com'],
    docsUrl: 'https://support.huaweicloud.com/obs/',
    verified: false,
  },
  {
    id: 'volcengine-tos',
    name: '火山引擎 TOS',
    endpointTemplate: `https://tos-s3-${REGION_PLACEHOLDER}.volces.com`,
    pathStyle: false,
    regions: ['cn-beijing', 'cn-shanghai', 'cn-guangzhou'],
    hostPrefixes: ['tos-s3-', 'tos-'],
    hostSuffixes: ['.volces.com'],
    docsUrl: 'https://www.volcengine.com/docs/6349/147050',
    verified: false,
  },
  {
    id: 'qiniu-kodo',
    name: '七牛云 Kodo',
    endpointTemplate: `https://s3.${REGION_PLACEHOLDER}.qiniucs.com`,
    pathStyle: true,
    regions: ['cn-east-1', 'cn-east-2', 'cn-north-1', 'cn-south-1'],
    hostPrefixes: ['s3.'],
    hostSuffixes: ['.qiniucs.com'],
    docsUrl: 'https://developer.qiniu.com/kodo/4088/s3-access-domainname',
    verified: false,
  },
]

/** 下拉项：预设厂商 + 末尾固定的自定义项。 */
export interface S3VendorOption {
  /** 下拉值 = 判别键（自定义项为 `CUSTOM_VENDOR_ID`）。 */
  readonly id: string
  /** 厂商专名；自定义项为空串（其标签由调用方按 i18n 渲染）。 */
  readonly name: string
  /** true = 末尾固定的「其他（自定义）」。 */
  readonly custom: boolean
  /** 是否已实测（自定义项恒 false）。 */
  readonly verified: boolean
}

/** 选中厂商后的字段预填。 */
export interface S3VendorPrefill {
  /** 端点（模板按地域替换后的完整 URL）。 */
  readonly endpoint: string
  /** 地域（所选地域，或该厂商的默认地域）。 */
  readonly region: string
  /** 寻址方式（该厂商的默认值）。 */
  readonly pathStyle: boolean
}

/** 按 id 取厂商预设（自定义或未知 id 返回 null）。 */
export function findVendorPreset(vendorId: string): S3VendorPreset | null {
  return S3_VENDOR_PRESETS.find((vendor) => vendor.id === vendorId) ?? null
}

/** 下拉项全集：全部预设按声明序 + 末尾固定「其他（自定义）」（验收判据）。 */
export function vendorOptions(): S3VendorOption[] {
  return [
    ...S3_VENDOR_PRESETS.map((vendor) => ({
      id: vendor.id,
      name: vendor.name,
      custom: false,
      verified: vendor.verified,
    })),
    { id: CUSTOM_VENDOR_ID, name: '', custom: true, verified: false },
  ]
}

/**
 * 选中厂商时的字段预填。
 *
 * `region` 缺省或用该厂商常用地域之外的值时回退首项——函数对任意输入都有确定
 * 输出，界面上的地域快捷项因此不会把未列出的地域写成端点里的悬空占位符。
 * 自定义（或未知）厂商不预填，返回 null：字段保持用户已填内容，全部可编辑。
 */
export function vendorPrefill(vendorId: string, region?: string): S3VendorPrefill | null {
  const vendor = findVendorPreset(vendorId)
  if (!vendor) return null
  const chosen =
    region !== undefined && vendor.regions.includes(region) ? region : vendor.regions[0]
  return {
    endpoint: vendor.endpointTemplate.replace(REGION_PLACEHOLDER, chosen),
    region: chosen,
    pathStyle: vendor.pathStyle,
  }
}

/** 档位标注的 i18n key（已实测 / 未实测）——单一映射点，两分支都有单测。 */
export function vendorTierKey(verified: boolean): string {
  return verified ? VENDOR_VERIFIED_KEY : VENDOR_UNVERIFIED_KEY
}

/** 从端点里取主机名（容忍省略 scheme 的写法；取不到返回空串）。 */
function hostnameOf(endpoint: string): string {
  const raw = endpoint.trim()
  if (raw === '') return ''
  for (const candidate of [raw, `https://${raw}`]) {
    try {
      return new URL(candidate).hostname.toLowerCase()
    } catch {
      // 换下一种写法继续试；两种都失败则按未命中处理。
    }
  }
  return ''
}

/** 去掉主机名最左一段（虚拟托管形态的 bucket 前缀）；无剩余段返回空串。 */
function withoutLeadingLabel(host: string): string {
  const dot = host.indexOf('.')
  return dot === -1 ? '' : host.slice(dot + 1)
}

/**
 * 按端点反查厂商（验收判据：命中回该厂商，未命中回 `CUSTOM_VENDOR_ID`）。
 *
 * 判据是「服务段前缀 + 品牌后缀」两端同时成立：只看品牌后缀会把同品牌的其他服务
 * 误判成对象存储（如华为云 ECS 的 `ecs.cn-north-4.myhuaweicloud.com`），只看后缀
 * 就是「未命中却回显厂商」，违反验收判据。虚拟托管形态下 bucket 是主机名最左一段
 *（`bucket.obs.cn-north-4.myhuaweicloud.com`），故原样与「去掉最左一段」两级都试。
 * 空串、无法解析的地址与自建域名一律回「其他（自定义）」——不猜、不按相似度兜底。
 */
export function matchVendorByEndpoint(endpoint: string): string {
  const host = hostnameOf(endpoint)
  if (host === '') return CUSTOM_VENDOR_ID
  const candidates = [host, withoutLeadingLabel(host)]
  const hit = S3_VENDOR_PRESETS.find((vendor) =>
    candidates.some(
      (candidate) =>
        vendor.hostSuffixes.some((suffix) => candidate.endsWith(suffix)) &&
        vendor.hostPrefixes.some((prefix) => candidate.startsWith(prefix)),
    ),
  )
  return hit ? hit.id : CUSTOM_VENDOR_ID
}
