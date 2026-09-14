//! 真桶验收脚手架（issue #1222 / 父 spec #1214）：在**用户自己的**真实 S3 兼容
//! 服务上验收「上传 → 下载 → 缺对象 → 大文件分片 → 保存前探针」四类用例。
//!
//! **全部用例默认忽略**（`#[ignore]`）：真桶验收需要用户自己的云账号凭据，仓库
//! 不持有、也绝不允许出现凭据（父 spec #1214 边界），CI 更不该把验收对象写进
//! 任何人的桶。跑法、逐家结论与「已实测 / 未实测」清单见
//! `docs/verification/1222-s3-vendor-acceptance.md`；那份清单是人读结论，界面
//! 档位（「已实测 / 未实测」）的唯一翻转点是 `packages/utils/src/s3-vendors.ts` 的
//! `verified` 字段，两者由前端守门测试逐行对齐。
//!
//! 凭据只经环境变量进入进程：不落文件、不进仓库、不进断言消息（失败信息只含桶名
//! 与对象键，不含密钥）。对象键一律在一次运行独有的前缀下，故用例可重复执行、
//! 可任意顺序执行、可并行；`Transport` 没有删除面，验收留下的对象由用户按该前缀
//! 清理（见文档第 2 节）。
//!
//! 断言强度：每条用例对准用户可观察结果——往返逐字节一致、缺对象归一为 `None`
//! （同步增量拉取的常态输入）、超阈值载荷经分片路径后仍逐字节一致、保存前探针
//! 连通且不在用户的桶里留对象。不为「让它绿」弱化任何一条。
//!
//! 四类用例各自独立成一条 `#[test]`（不做矩阵收敛）：真桶验收要按厂商**逐类**
//! 记录通过与否（见文档第 6 节记录表），收敛成一条共用断言体会丢掉逐类信号。

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

/// 真桶验收的凭据与目标（全部由用户在自己的 shell 里填写，见文档第 2 节）。
const ENV_ENDPOINT: &str = "LEDGER_S3_TEST_ENDPOINT";
const ENV_REGION: &str = "LEDGER_S3_TEST_REGION";
const ENV_BUCKET: &str = "LEDGER_S3_TEST_BUCKET";
const ENV_ACCESS_KEY: &str = "LEDGER_S3_TEST_ACCESS_KEY";
const ENV_SECRET_KEY: &str = "LEDGER_S3_TEST_SECRET_KEY";
const ENV_PATH_STYLE: &str = "LEDGER_S3_TEST_PATH_STYLE";
/// 可选的固定顶层前缀：给了它，验收对象就都落在它下面，跑完照它清理。
const ENV_PREFIX: &str = "LEDGER_S3_TEST_PREFIX";

/// 缺变量与填法都指向这份文档（单一说明处）。
const ACCEPTANCE_DOC: &str = "docs/verification/1222-s3-vendor-acceptance.md";

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
            .expect("S3 通道构库失败：请核对端点 / 区域 / 桶名 / 前缀（见验收文档第 2 节）");
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

/// 从环境变量组装被测配置；缺项 fail loud，一次列全并指向文档。
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
            "真桶验收缺少环境变量：{}\n填写说明与跑法见 {ACCEPTANCE_DOC} 第 2 节",
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

/// 寻址方式是验收要记录的结论之一（见清单第 3 节），故要求显式给出、不设默认。
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
#[ignore = "需要用户自己的真桶与凭据：见 docs/verification/1222-s3-vendor-acceptance.md"]
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
#[ignore = "需要用户自己的真桶与凭据：见 docs/verification/1222-s3-vendor-acceptance.md"]
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
#[ignore = "需要用户自己的真桶与凭据：见 docs/verification/1222-s3-vendor-acceptance.md"]
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
#[ignore = "需要用户自己的真桶与凭据：见 docs/verification/1222-s3-vendor-acceptance.md"]
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
