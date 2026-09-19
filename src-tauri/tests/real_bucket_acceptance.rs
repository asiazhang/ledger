//! 真桶验收脚手架（issue #1222 / 父 spec #1214）：在**用户自己的**真实 S3 兼容
//! 服务上验收「上传 → 下载 → 缺对象 → 大文件分片 → 保存前探针」四类用例。
//!
//! **全部用例默认忽略**（`#[ignore]`）：真桶验收需要用户自己的云账号凭据，仓库
//! 不持有、也绝不允许出现凭据（父 spec #1214 边界），CI 更不该把验收对象写进
//! 任何人的桶。本文件即跑法、前置与结论回写的**单一说明处**；界面档位（「已实测 /
//! 未实测」）的唯一来源与翻转点是 `packages/utils/src/s3-vendors.ts` 的 `verified`
//! 字段，这里只讲怎么把某一家跑成「已实测」。
//!
//! # 诚实边界（未跑过的一律「未实测」）
//!
//! - 本仓库不持有任何云账号凭据，验收对象是**用户自己的桶**。用户按下面跑完之前，
//!   厂商档位一律「未实测」——不凭官方文档的兼容性声明、也不凭第三方经验推断。
//! - 「已实测」的唯一含义：在**同一家**厂商上跑完全部四类用例且全部通过，并把预设
//!   表里该家的 `verified` 改成 `true`。
//! - 官方文档给出「S3 兼容端点」≠ 实测通过。尤其**是否接受 AWS SigV4 签名**，spec
//!   #1214 已明示尚待事实核查（阿里云 OSS 是其中之一），未实测前不得写成结论。
//! - 预设默认的寻址方式来自各家公开文档（#1220 落地时核对），仍属未实测：真桶上
//!   两种寻址都允许试，跑完把实际可用的那种记下来。
//!
//! # 跑法
//!
//! ## 前置
//!
//! - 该厂商的一个桶。建议**专用桶**，或至少给验收一个专用顶层前缀
//!   （`LEDGER_S3_TEST_PREFIX`）。
//! - 一份**最小权限**凭据：`GetObject`、`PutObject`、`CreateMultipartUpload`、
//!   `UploadPart`、`CompleteMultipartUpload`、`AbortMultipartUpload`（后两项为分片
//!   用例所需）。**不需要** `ListBucket`——「测试连接」探针只读单个保留键、不列桶。
//! - 该桶的端点、区域与寻址方式（可先用设置页的「测试连接」验证一遍再填变量）。
//!
//! 凭据只经环境变量进入进程：不落文件、不进仓库、不进断言消息（失败信息只含桶名
//! 与对象键，不含密钥）。填变量时别把凭据写进 shell 历史（`export VAR=值` 会落
//! 历史），逐项静默输入即可：
//!
//! ```sh
//! read -rsp 'Secret Access Key: ' LEDGER_S3_TEST_SECRET_KEY
//! export LEDGER_S3_TEST_SECRET_KEY
//! ```
//!
//! | 变量 | 必填 | 说明 |
//! | --- | --- | --- |
//! | `LEDGER_S3_TEST_ENDPOINT` | 是 | S3 兼容端点，含 scheme |
//! | `LEDGER_S3_TEST_REGION` | 是 | SigV4 签名区域 |
//! | `LEDGER_S3_TEST_BUCKET` | 是 | 桶名 |
//! | `LEDGER_S3_TEST_ACCESS_KEY` | 是 | Access Key ID（用户自己的） |
//! | `LEDGER_S3_TEST_SECRET_KEY` | 是 | Secret Access Key（用户自己的） |
//! | `LEDGER_S3_TEST_PATH_STYLE` | 是 | `false` = 虚拟托管，`true` = path-style |
//! | `LEDGER_S3_TEST_PREFIX` | 否 | 固定顶层前缀，验收对象都落在它下面；缺省 = 桶根 |
//!
//! ## 跑
//!
//! ```sh
//! cd src-tauri
//! cargo test -p tauri-app --test real_bucket_acceptance -- --ignored --nocapture
//! ```
//!
//! 四类用例一次跑完。`--ignored` 是必需的——用例默认忽略，常规 `cargo test` 与 CI
//! 都不跑它们，缺变量时会一次列全缺项。**换一家厂商**换一组环境变量重跑；**同一家
//! 建议两种寻址各跑一次**，把真桶上实际可用的那种记下来。
//!
//! ## 清理
//!
//! 验收对象不自动删除（`Transport` 没有删除操作面）。它们全在
//! `[LEDGER_S3_TEST_PREFIX/]ledger-acceptance-<每次运行的随机串>/` 这一根前缀之下，
//! 故用例可重复执行、可任意顺序执行、可并行；由用户按该前缀批量删除即可，用专用桶
//! 的话直接清桶也行。
//!
//! ## 结论回写
//!
//! 跑通某一家后，把 `packages/utils/src/s3-vendors.ts` 里该家的 `verified` 改成
//! `true`——那是界面「已实测 / 未实测」档位的唯一来源。
//!
//! # 断言强度
//!
//! 每条用例对准用户可观察结果——往返逐字节一致、缺对象归一为 `None`（同步增量拉取
//! 的常态输入）、超阈值载荷经分片路径后仍逐字节一致、保存前探针连通且不在用户的桶里
//! 留对象。不为「让它绿」弱化任何一条。
//!
//! 四类用例各自独立成一条 `#[test]`（不做矩阵收敛）：真桶验收要按厂商**逐类**记录
//! 通过与否，收敛成一条共用断言体会丢掉逐类信号。

// 测试整体豁免（ADR-0060）：集成测试 crate 经 cfg(test) 放行六件套，生产构建零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

use ledger_infra::db::new_uuid;
use ledger_sync_engine::{
    ChannelLayout, S3Config, S3Transport, SyncChannelConfig, Transport, probe_channel,
};

/// 真桶验收的凭据与目标（全部由用户在自己的 shell 里填写，见本文件模块文档「跑法」）。
const ENV_ENDPOINT: &str = "LEDGER_S3_TEST_ENDPOINT";
const ENV_REGION: &str = "LEDGER_S3_TEST_REGION";
const ENV_BUCKET: &str = "LEDGER_S3_TEST_BUCKET";
const ENV_ACCESS_KEY: &str = "LEDGER_S3_TEST_ACCESS_KEY";
const ENV_SECRET_KEY: &str = "LEDGER_S3_TEST_SECRET_KEY";
const ENV_PATH_STYLE: &str = "LEDGER_S3_TEST_PATH_STYLE";
/// 可选的固定顶层前缀：给了它，验收对象就都落在它下面，跑完照它清理。
const ENV_PREFIX: &str = "LEDGER_S3_TEST_PREFIX";

/// 缺变量与填法都指向本文件的模块文档（单一说明处）。
const RUNBOOK: &str = "本文件模块文档「跑法」";

/// 用例失败时的定位信息只到桶名与对象键这一层，不含任何凭据。
struct AcceptanceTarget {
    transport: S3Transport,
    root: String,
}

impl AcceptanceTarget {
    /// 构库（不发网络请求）并分配本次运行独有的对象键根。
    fn new() -> Self {
        let config = config_from_env();
        let transport = S3Transport::new(config)
            .expect("S3 通道构库失败：请核对端点 / 区域 / 桶名 / 前缀（见本文件模块文档「跑法」）");
        Self {
            transport,
            root: run_root(),
        }
    }

    /// 本次运行的完整对象键（根前缀 + 用例自己的对象名）。
    fn key(&self, name: &str) -> String {
        format!("{}/{name}", self.root)
    }
}

/// 从环境变量组装被测配置；缺项 fail loud，一次列全并指向模块文档。
fn config_from_env() -> S3Config {
    let mut missing: Vec<&str> = Vec::new();
    let endpoint = required_env(ENV_ENDPOINT, &mut missing);
    let region = required_env(ENV_REGION, &mut missing);
    let bucket = required_env(ENV_BUCKET, &mut missing);
    let access_key = required_env(ENV_ACCESS_KEY, &mut missing);
    let secret_key = required_env(ENV_SECRET_KEY, &mut missing);
    let path_style = required_env(ENV_PATH_STYLE, &mut missing);
    if !missing.is_empty() {
        panic!(
            "真桶验收缺少环境变量：{}\n填写说明与跑法见 {RUNBOOK}",
            missing.join(" / ")
        );
    }
    S3Config {
        endpoint,
        region,
        bucket,
        access_key,
        secret_key,
        prefix: std::env::var(ENV_PREFIX).unwrap_or_default(),
        path_style: parse_path_style(&path_style),
    }
}

/// 取一个必填环境变量；缺失或全空白记名后回空串（由调用方统一报错）。
fn required_env(name: &'static str, missing: &mut Vec<&'static str>) -> String {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            missing.push(name);
            String::new()
        }
    }
}

/// 寻址方式是验收要记录的结论之一（两种都允许试），故要求显式给出、不设默认。
fn parse_path_style(raw: &str) -> bool {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => true,
        "false" | "0" => false,
        other => panic!("{ENV_PATH_STYLE} 只接受 true / false，实际值：{other}"),
    }
}

/// 本次运行的对象键根：`[LEDGER_S3_TEST_PREFIX/]ledger-acceptance-<uuid>`。
/// 每次运行一个根，用例之间不共享对象，重复执行不互相污染。
fn run_root() -> String {
    let base = std::env::var(ENV_PREFIX).unwrap_or_default();
    let base = base.trim().trim_matches('/').to_string();
    let run = format!("ledger-acceptance-{}", new_uuid());
    if base.is_empty() {
        run
    } else {
        format!("{base}/{run}")
    }
}

/// 确定性载荷（自造线性同余生成器，不引随机依赖）：逐字节比对能抓到任何错位、
/// 截断或分片边界错拼——比「全是同一个字节」的载荷强。
fn payload(len: usize, seed: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    let mut state = seed | 1;
    for _ in 0..len {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        out.push((state >> 33) as u8);
    }
    out
}

/// 上传 → 下载：往返逐字节一致（最小验收面）。
#[test]
#[ignore = "需要用户自己的真桶与凭据：见本文件模块文档「跑法」"]
fn upload_then_download_roundtrips_byte_identical() {
    let target = AcceptanceTarget::new();
    let key = target.key("roundtrip.bin");
    let body = payload(64 * 1024 + 7, 0x1222_0001);

    target.transport.write_file(&key, &body).expect("上传失败");
    let read = target.transport.read_file(&key).expect("下载失败");

    assert_eq!(
        read.as_deref(),
        Some(body.as_slice()),
        "对象 {key} 往返后字节不一致"
    );
    println!("✅ 上传 / 下载逐字节一致：{key}（{} 字节）", body.len());
}

/// 缺对象：读不存在的对象回 `None`（不是错误）——同步增量拉取以 404 为常态输入。
#[test]
#[ignore = "需要用户自己的真桶与凭据：见本文件模块文档「跑法」"]
fn missing_object_reads_as_none() {
    let target = AcceptanceTarget::new();
    let key = target.key("missing-object.bin");

    let read = target
        .transport
        .read_file(&key)
        .expect("读取缺对象不该报错");

    assert_eq!(
        read, None,
        "对象 {key} 不该存在：缺对象必须归一为 None，不能是错误"
    );
    println!("✅ 缺对象归一为 None：{key}");
}

/// 大文件分片：载荷超过 8 MiB 阈值，写入走 `create → uploadPart×3 → complete`
/// 而非单次 PUT；往返仍逐字节一致。
///
/// 本用例只能断言用户可观察结果（往返一致）——阈值跨越由载荷长度保证，具体
/// 分片请求数不在断言面内（本脚手架没有中间代理可观测线上形态）。
#[test]
#[ignore = "需要用户自己的真桶与凭据：见本文件模块文档「跑法」"]
fn large_object_roundtrips_through_multipart_path() {
    let target = AcceptanceTarget::new();
    let key = target.key("multipart.bin");
    // 8 MiB 阈值之上：16 MiB（两整片）+ 1 MiB + 余量 = 三片。
    let body = payload(17 * 1024 * 1024 + 1234, 0x1222_0002);

    target
        .transport
        .write_file(&key, &body)
        .expect("分片上传失败");
    let read = target.transport.read_file(&key).expect("分片对象下载失败");

    assert_eq!(
        read.as_deref(),
        Some(body.as_slice()),
        "分片对象 {key} 往返后字节不一致"
    );
    println!(
        "✅ 大文件经分片路径往返一致：{key}（{} 字节，阈值 8 MiB）",
        body.len()
    );
}

/// 保存前「测试连接」探针：在真桶上连通，且跑完通道上仍没有探针对象——
/// 探针是只读的，不在用户的桶里留垃圾（探针键是同步轮次永不写的保留键）。
#[test]
#[ignore = "需要用户自己的真桶与凭据：见本文件模块文档「跑法」"]
fn connectivity_probe_passes_and_leaves_the_channel_untouched() {
    let config = config_from_env();
    // 探针路径按同步空间派生（`book-<space>/probe/...`）：给本次运行一个独有
    // 空间，避免与真实同步数据互认。
    let space_id = format!("acceptance-{}", new_uuid());
    let channel = SyncChannelConfig {
        space_id: space_id.clone(),
        endpoint: config.endpoint.clone(),
        region: config.region.clone(),
        bucket: config.bucket.clone(),
        prefix: config.prefix.clone(),
        access_key: config.access_key.clone(),
        secret_key: config.secret_key.clone(),
        path_style: config.path_style,
    };

    probe_channel(&channel).expect("保存前探针应连通（凭据 / 目标 / 权限 / 网络任一未就位即失败）");

    let layout = ChannelLayout::new(&space_id).expect("通道布局构造失败");
    let transport = S3Transport::new(config).expect("S3 通道构库失败");
    let probe_object = transport
        .read_file(&layout.probe_path())
        .expect("读取探针键不该报错");
    assert_eq!(
        probe_object, None,
        "探针不得写入通道：探针键必须保持不存在（写入会在用户的桶里留垃圾对象）"
    );
    println!("✅ 测试连接探针连通且只读：{}", layout.probe_path());
}
