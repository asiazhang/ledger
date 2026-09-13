# 真桶验收清单：S3 兼容厂商（issue #1222 / 父 spec #1214）

> 验收节点：真桶实测是**运行期真实系统行为**（用户自己的云账号与桶），无法也不应
> 在 CI 中自动化——本清单即 issue #1222「验收脚本或步骤可重复执行」的落位。自动化
> 脚手架见 `src-tauri/tests/real_bucket_acceptance.rs`（全部 `#[ignore]`）。
>
> 「已实测 / 未实测」的**人读结论**在本文件第 3 节；界面档位标注（下拉里的「已实测 /
> 未实测」）的**唯一翻转点**是预设表 `S3_VENDOR_PRESETS[].verified`。两者由前端守门
> 测试逐行对齐（预设 id 集合全等、结论与档位一致、寻址方式与预设默认一致、官方文档
> 外链在册），漂移即红。

## 0. 诚实边界（未跑过的一律「未实测」）

- 本仓库不持有任何云账号凭据，验收对象是**用户自己的桶**。用户按下文跑完之前，第 3 节
  清单一律「未实测」——**不凭官方文档的兼容性声明、也不凭第三方经验推断「已实测」**。
- 「已实测」的唯一含义：在**同一家**厂商上按第 2 节跑完第 1 节全部四类用例且全部通过，
  并在第 5 节记录表留下日期与结论。
- 官方文档给出「S3 兼容端点」≠ 实测通过。尤其**「是否接受 AWS SigV4 签名」**这件事，
  spec #1214 已明示尚待事实核查（阿里云 OSS 是其中之一），未实测前不得写成结论；
  文档外链只作填写入口，不构成档位依据。
- 预设默认的寻址方式来自各家公开文档（#1220 落地时核对），仍属**未实测**结论：真桶上
  允许两种寻址都试（第 2 节），跑完把实际可用的那种记进记录表。

## 1. 一次验收要跑什么（四类用例）

| 用例 | 断言（用户可观察结果） | 脚手架用例 |
| --- | --- | --- |
| 上传 → 下载 | 往返逐字节一致 | `upload_then_download_roundtrips_byte_identical` |
| 缺对象 | 读不存在的对象回「无此对象」（不是错误） | `missing_object_reads_as_none` |
| 大文件分片 | 载荷越过分片阈值后往返仍逐字节一致 | `large_object_roundtrips_through_multipart_path` |
| 测试连接 | 保存前探针连通，且跑完通道上仍无探针对象 | `connectivity_probe_passes_and_leaves_the_channel_untouched` |

大文件用例的载荷是 17 MiB + 1234 字节，越过 8 MiB 分片阈值：写入端因此经
`CreateMultipartUpload → UploadPart×3 → CompleteMultipartUpload` 而非单次 PUT。
分片请求数不在断言面内（脚手架没有中间代理可观测线上形态），断言落在「往返逐字节
一致」这一用户可观察结果上。

## 2. 怎么跑（凭据由用户自己填写）

### 2.1 前置

- 该厂商的一个桶。建议**专用桶**，或至少给验收一个专用顶层前缀（见 `LEDGER_S3_TEST_PREFIX`）。
- 一份**最小权限**凭据：`GetObject`、`PutObject`、`CreateMultipartUpload`、`UploadPart`、
  `CompleteMultipartUpload`、`AbortMultipartUpload`（后两项为分片用例所需）。
  **不需要** `ListBucket`——「测试连接」探针只读单个保留键、不列桶，这也是它能在最小
  权限子账号上通过的原因。
- 该桶的端点、区域与寻址方式（可以先用设置页的「测试连接」验证一遍再填下面的变量）。

### 2.2 环境变量

凭据只经环境变量进入进程：不落文件、不进仓库、不进断言消息（失败信息只含桶名与对象键）。

| 变量 | 必填 | 说明 | 例 |
| --- | --- | --- | --- |
| `LEDGER_S3_TEST_ENDPOINT` | 是 | S3 兼容端点，含 scheme | `https://s3.oss-cn-hangzhou.aliyuncs.com` |
| `LEDGER_S3_TEST_REGION` | 是 | SigV4 签名区域 | `cn-hangzhou` |
| `LEDGER_S3_TEST_BUCKET` | 是 | 桶名 | `my-ledger-sync` |
| `LEDGER_S3_TEST_ACCESS_KEY` | 是 | Access Key ID（用户自己的） | （用户自己的） |
| `LEDGER_S3_TEST_SECRET_KEY` | 是 | Secret Access Key（用户自己的） | （用户自己的） |
| `LEDGER_S3_TEST_PATH_STYLE` | 是 | `false` = 虚拟托管，`true` = path-style | `false` |
| `LEDGER_S3_TEST_PREFIX` | 否 | 固定顶层前缀，验收对象都落在它下面；缺省 = 桶根 | `acceptance` |

填变量时别把凭据写进 shell 历史（`export VAR=值` 会落历史）。逐项静默输入即可：

```sh
read -rsp  'Secret Access Key: ' LEDGER_S3_TEST_SECRET_KEY; export LEDGER_S3_TEST_SECRET_KEY
```

其余变量同理（`ENDPOINT` / `REGION` / `BUCKET` / `ACCESS_KEY` / `PATH_STYLE` / `PREFIX`）。

### 2.3 跑

```sh
cd src-tauri
cargo test -p tauri-app --test real_bucket_acceptance -- --ignored --nocapture
```

四类用例一次跑完。`--ignored` 是必需的——用例默认忽略，常规 `cargo test` 与 CI 都不跑
它们，缺变量时脚本会一次列全缺项并指向本文件第 2 节。

**换一家厂商**：换一组环境变量重跑一次。**同一家建议两种寻址各跑一次**（预设默认见第 4
节），把真桶上实际可用的那种记进记录表。

### 2.4 清理

验收对象不自动删除（同步通道没有删除操作面）。它们全在
`[LEDGER_S3_TEST_PREFIX/]ledger-acceptance-<每次运行的随机串>/` 这一根前缀之下——
按该前缀批量删除即可；用专用桶的话直接清桶也行。

## 3. 「已实测 / 未实测」清单

> 表头列名是守门测试的判据：`预设 id`、`寻址方式`、`已知限制`、`结论` 四列必须在表头
> 里；`结论` 只接受 `已实测` / `未实测`，`寻址方式` 只接受 `虚拟托管` / `path-style`
> 且必须与预设默认一致。改写方式见第 5 节。

| 厂商 | 预设 id | 寻址方式（预设默认） | AWS SigV4 兼容性 | 已知限制 | 结论 |
| --- | --- | --- | --- | --- | --- |
| 阿里云 OSS | `aliyun-oss` | 虚拟托管 | 待核查（spec #1214 明示未定，需真桶实测） | 未实测：端点与签名兼容性均未在真桶验证；预设端点取官方 S3 专属域名，与原生 REST 端点不同名 | 未实测 |
| 腾讯云 COS | `tencent-cos` | 虚拟托管 | 待核查（未实测） | 未实测：需真桶确认签名形态与分片上传行为 | 未实测 |
| 华为云 OBS | `huawei-obs` | 虚拟托管 | 待核查（未实测） | 未实测：需真桶确认签名形态与分片上传行为 | 未实测 |
| 火山引擎 TOS | `volcengine-tos` | 虚拟托管 | 待核查（未实测） | 未实测：预设取官方 S3 专属域名，与原生端点不同名 | 未实测 |
| 七牛云 Kodo | `qiniu-kodo` | path-style | 待核查（未实测） | 未实测：预设默认 path-style，需真桶确认签名形态与分片上传行为 | 未实测 |

## 4. 逐家明细（寻址、限制与官方文档入口）

`寻址方式` 一列记的是**预设默认**（可在界面改，两种都能试）；`限制` 一列在实测前只能写
「本客户端侧的确定约束」与「待核查项」——服务端兼容性结论要等真桶。

各家用例共同面对的客户端侧约束（与厂商无关，写在这里不逐家重复）：

- **签名固定 SigV4**：端点必须是接受 AWS SigV4 的 S3 兼容入口；不接受的厂商本应用不可用。
- **大文件走分片**：超过 8 MiB 的载荷经分片上传，凭据需具备分片三件套权限。
- **探针只读不列桶**：凭据只需对象读权限即可通过「测试连接」。
- **无删除操作面**：验收对象留在桶里，由用户按前缀清理（第 2.4 节）。

### 阿里云 OSS

- 端点模板：`s3.oss-{region}.aliyuncs.com`（虚拟托管）
- 待核查项：S3 兼容端点是否接受 AWS SigV4 签名——spec #1214 明示未定，是本家实测的重点。
- 官方文档：<https://help.aliyun.com/zh/oss/developer-reference/use-amazon-s3-sdks-to-access-oss>

### 腾讯云 COS

- 端点模板：`cos.{region}.myqcloud.com`（虚拟托管）
- 待核查项：签名形态与分片上传行为需真桶确认。
- 官方文档：<https://cloud.tencent.com/document/product/436>

### 华为云 OBS

- 端点模板：`obs.{region}.myhuaweicloud.com`（虚拟托管）
- 待核查项：签名形态与分片上传行为需真桶确认。
- 官方文档：<https://support.huaweicloud.com/obs/>

### 火山引擎 TOS

- 端点模板：`tos-s3-{region}.volces.com`（虚拟托管；官方文档口径为 S3 专属域名）
- 待核查项：签名形态与分片上传行为需真桶确认。
- 官方文档：<https://www.volcengine.com/docs/6349/147050>

### 七牛云 Kodo

- 端点模板：`s3.{region}.qiniucs.com`（path-style）
- 待核查项：签名形态与分片上传行为需真桶确认。
- 官方文档：<https://developer.qiniu.com/kodo/4088/s3-access-domainname>

## 5. 结论怎么回写（翻档位）

1. 按第 2.3 节跑完并全绿（四类用例全过；某类失败即该家仍为「未实测」，把失败现象记进
   记录表备注）。
2. 把第 3 节该家的 `结论` 改成 `已实测`，并在第 6 节记录表补一行（日期、厂商、实际可用
   的寻址方式、结论、备注如「三片分片通过」）。
3. 把预设表里该家的 `verified` 改成 `true`——这是界面档位的唯一来源。
4. 跑守门与全量检查：`pnpm vitest run src/__tests__/s3-vendor-acceptance-doc.test.ts`
   （清单与档位不一致即红）与 `./scripts/check.sh`。

## 6. 记录表

| 日期 | 厂商 | 寻址方式 | 结论 | 备注 |
| --- | --- | --- | --- | --- |
| （待填） | | | | |
