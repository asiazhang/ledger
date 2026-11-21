export const REGION_PLACEHOLDER = "{region}";

export const CUSTOM_VENDOR_ID = "custom";

export const VENDOR_VERIFIED_KEY = "settings.data.sync.vendorVerified";
export const VENDOR_UNVERIFIED_KEY = "settings.data.sync.vendorUnverified";

export interface S3VendorPreset {
  readonly id: string;
  readonly name: string;
  readonly endpointTemplate: string;
  readonly pathStyle: boolean;
  readonly regions: readonly string[];
  readonly hostPrefixes: readonly string[];
  readonly hostSuffixes: readonly string[];
  readonly docsUrl: string;
  readonly verified: boolean;
}

export const S3_VENDOR_PRESETS: readonly S3VendorPreset[] = [
  {
    id: "aliyun-oss",
    name: "阿里云 OSS",
    endpointTemplate: `https://s3.oss-${REGION_PLACEHOLDER}.aliyuncs.com`,
    pathStyle: false,
    regions: [
      "cn-hangzhou",
      "cn-shanghai",
      "cn-beijing",
      "cn-shenzhen",
      "cn-guangzhou",
      "cn-chengdu",
    ],
    hostPrefixes: ["s3.oss-", "oss-"],
    hostSuffixes: [".aliyuncs.com"],
    docsUrl: "https://help.aliyun.com/zh/oss/developer-reference/use-amazon-s3-sdks-to-access-oss",
    verified: false,
  },
  {
    id: "tencent-cos",
    name: "腾讯云 COS",
    endpointTemplate: `https://cos.${REGION_PLACEHOLDER}.myqcloud.com`,
    pathStyle: false,
    regions: ["ap-guangzhou", "ap-beijing", "ap-shanghai", "ap-chengdu", "ap-hongkong"],
    hostPrefixes: ["cos."],
    hostSuffixes: [".myqcloud.com"],
    docsUrl: "https://cloud.tencent.com/document/product/436",
    verified: false,
  },
  {
    id: "huawei-obs",
    name: "华为云 OBS",
    endpointTemplate: `https://obs.${REGION_PLACEHOLDER}.myhuaweicloud.com`,
    pathStyle: false,
    regions: ["cn-north-4", "cn-east-3", "cn-south-1", "cn-southwest-2", "cn-north-1"],
    hostPrefixes: ["obs."],
    hostSuffixes: [".myhuaweicloud.com"],
    docsUrl: "https://support.huaweicloud.com/obs/",
    verified: false,
  },
  {
    id: "volcengine-tos",
    name: "火山引擎 TOS",
    endpointTemplate: `https://tos-s3-${REGION_PLACEHOLDER}.volces.com`,
    pathStyle: false,
    regions: ["cn-beijing", "cn-shanghai", "cn-guangzhou"],
    hostPrefixes: ["tos-s3-", "tos-"],
    hostSuffixes: [".volces.com"],
    docsUrl: "https://www.volcengine.com/docs/6349/147050",
    verified: false,
  },
  {
    id: "qiniu-kodo",
    name: "七牛云 Kodo",
    endpointTemplate: `https://s3.${REGION_PLACEHOLDER}.qiniucs.com`,
    pathStyle: true,
    regions: ["cn-east-1", "cn-east-2", "cn-north-1", "cn-south-1"],
    hostPrefixes: ["s3."],
    hostSuffixes: [".qiniucs.com"],
    docsUrl: "https://developer.qiniu.com/kodo/4088/s3-access-domainname",
    verified: false,
  },
];

export interface S3VendorOption {
  readonly id: string;
  readonly name: string;
  readonly custom: boolean;
  readonly verified: boolean;
}

export interface S3VendorPrefill {
  readonly endpoint: string;
  readonly region: string;
  readonly pathStyle: boolean;
}

export function findVendorPreset(vendorId: string): S3VendorPreset | null {
  return S3_VENDOR_PRESETS.find((vendor) => vendor.id === vendorId) ?? null;
}

export function vendorOptions(): S3VendorOption[] {
  return [
    ...S3_VENDOR_PRESETS.map((vendor) => ({
      id: vendor.id,
      name: vendor.name,
      custom: false,
      verified: vendor.verified,
    })),
    { id: CUSTOM_VENDOR_ID, name: "", custom: true, verified: false },
  ];
}

export function vendorPrefill(vendorId: string, region?: string): S3VendorPrefill | null {
  const vendor = findVendorPreset(vendorId);
  if (!vendor) return null;
  const chosen =
    region !== undefined && vendor.regions.includes(region) ? region : vendor.regions[0];
  return {
    endpoint: vendor.endpointTemplate.replace(REGION_PLACEHOLDER, chosen),
    region: chosen,
    pathStyle: vendor.pathStyle,
  };
}

export function vendorTierKey(verified: boolean): string {
  return verified ? VENDOR_VERIFIED_KEY : VENDOR_UNVERIFIED_KEY;
}

function hostnameOf(endpoint: string): string {
  const raw = endpoint.trim();
  if (raw === "") return "";
  for (const candidate of [raw, `https://${raw}`]) {
    try {
      return new URL(candidate).hostname.toLowerCase();
    } catch {}
  }
  return "";
}

function withoutLeadingLabel(host: string): string {
  const dot = host.indexOf(".");
  return dot === -1 ? "" : host.slice(dot + 1);
}

export function matchVendorByEndpoint(endpoint: string): string {
  const host = hostnameOf(endpoint);
  if (host === "") return CUSTOM_VENDOR_ID;
  const candidates = [host, withoutLeadingLabel(host)];
  const hit = S3_VENDOR_PRESETS.find((vendor) =>
    candidates.some(
      (candidate) =>
        vendor.hostSuffixes.some((suffix) => candidate.endsWith(suffix)) &&
        vendor.hostPrefixes.some((prefix) => candidate.startsWith(prefix)),
    ),
  );
  return hit ? hit.id : CUSTOM_VENDOR_ID;
}
