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

const ENV_ENDPOINT: &str = "LEDGER_S3_TEST_ENDPOINT";
const ENV_REGION: &str = "LEDGER_S3_TEST_REGION";
const ENV_BUCKET: &str = "LEDGER_S3_TEST_BUCKET";
const ENV_ACCESS_KEY: &str = "LEDGER_S3_TEST_ACCESS_KEY";
const ENV_SECRET_KEY: &str = "LEDGER_S3_TEST_SECRET_KEY";
const ENV_PATH_STYLE: &str = "LEDGER_S3_TEST_PATH_STYLE";
const ENV_PREFIX: &str = "LEDGER_S3_TEST_PREFIX";

const RUNBOOK: &str = "本文件模块文档「跑法」";

struct AcceptanceTarget {
    transport: S3Transport,
    root: String,
}

impl AcceptanceTarget {
    fn new() -> Self {
        let config = config_from_env();
        let transport = S3Transport::new(config)
            .expect("S3 通道构库失败：请核对端点 / 区域 / 桶名 / 前缀（见本文件模块文档「跑法」）");
        Self {
            transport,
            root: run_root(),
        }
    }

    fn key(&self, name: &str) -> String {
        format!("{}/{name}", self.root)
    }
}

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

fn required_env(name: &'static str, missing: &mut Vec<&'static str>) -> String {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            missing.push(name);
            String::new()
        }
    }
}

fn parse_path_style(raw: &str) -> bool {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => true,
        "false" | "0" => false,
        other => panic!("{ENV_PATH_STYLE} 只接受 true / false，实际值：{other}"),
    }
}

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

#[test]
#[ignore = "需要用户自己的真桶与凭据：见本文件模块文档「跑法」"]
fn large_object_roundtrips_through_multipart_path() {
    let target = AcceptanceTarget::new();
    let key = target.key("multipart.bin");
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

#[test]
#[ignore = "需要用户自己的真桶与凭据：见本文件模块文档「跑法」"]
fn connectivity_probe_passes_and_leaves_the_channel_untouched() {
    let config = config_from_env();
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
