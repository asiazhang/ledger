//! S3 兼容对象存储通道后端（issue #1216 / ADR-0091 决策 1）。
//!
//! 对象键 = [`S3Config::prefix`] + 逻辑通道路径；对象存储没有目录概念，
//! [`Transport::ensure_dir`] 因此是无副作用的空操作。读写经官方 SDK 走 SigV4
//! 签名，支持自定义端点与 path-style / virtual-host 两种寻址。
//!
//! 兼容优先：客户端显式设置请求校验和 `WhenRequired`、响应校验和
//! `WhenRequired`——默认的 `WhenSupported` 会给上传追加 AWS 专有校验和扩展，
//! 部分 S3 兼容服务会直接拒绝。该策略在构建配置上显式落地，不读取
//! `AWS_REQUEST_CHECKSUM_CALCULATION` 等环境变量。
//!
//! 同步桥接：Transport 是同步接口，SDK 只有异步门面。本后端自持一条专用线程
//! 与 current-thread Tokio runtime，调用方经通道提交任务并阻塞等待结果——
//! 不把 `block_on` 跑在调用方线程上，因此调用方即使已在 Tokio 上下文
//! （runtime 线程或 `spawn_blocking`）中调用也不会嵌套 runtime 而 panic。
//! 线程随传输句柄 Drop 退出。

use std::sync::mpsc;
use std::thread;

use aws_sdk_s3::Client;
use aws_sdk_s3::config::{
    BehaviorVersion, Credentials, Region, RequestChecksumCalculation, ResponseChecksumValidation,
};
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};

use crate::error::{AppError, Result};

use super::{
    Transport, auth_failed_error, http_failed_error, network_failed_error, permission_denied_error,
    target_missing_error, validate_logical_path,
};

/// 触发分片上传的对象体积阈值。S3 单对象可整体 PUT，但大文件（检查点快照）
/// 走分片上传可避免单请求体过大；阈值取 8 MiB（S3 最小分片 5 MiB 之上）。
const MULTIPART_THRESHOLD_BYTES: usize = 8 * 1024 * 1024;

/// 分片大小。末片可小于该值，其余分片均 ≥ 5 MiB，满足 S3 约束。
const MULTIPART_PART_BYTES: usize = 8 * 1024 * 1024;

/// S3 连接配置（凭据与根前缀；持久化与配置分派归 #1217 壳层设置面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Config {
    /// 自定义端点（如 `https://s3.example.com` 或本地兼容服务地址）。
    pub endpoint: String,
    /// SigV4 签名区域。
    pub region: String,
    /// 目标桶名。
    pub bucket: String,
    /// Access Key ID。
    pub access_key: String,
    /// Secret Access Key。
    pub secret_key: String,
    /// 对象键前缀（首尾 `/` 归一化后拼接逻辑路径；空串合法 = 根）。
    pub prefix: String,
    /// `true` = path-style（`/{bucket}/{key}`），`false` = virtual-host。
    pub path_style: bool,
}

/// S3 哑字节通道。内部为官方异步客户端 + 专用同步桥接线程，可跨线程共享。
pub struct S3Transport {
    client: Client,
    bucket: String,
    prefix: String,
    bridge: AsyncBridge,
}

impl std::fmt::Debug for S3Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Transport")
            .field("bucket", &self.bucket)
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

impl S3Transport {
    /// 从配置构建通道（定型客户端参数；不发起网络请求）。
    pub fn new(config: S3Config) -> Result<Self> {
        let endpoint = config.endpoint.trim().trim_end_matches('/').to_string();
        if endpoint.is_empty() {
            return Err(AppError::coded(
                "sync-channel.base-url-missing",
                "同步通道地址不能为空",
            ));
        }
        let bucket = config.bucket.trim().to_string();
        if bucket.is_empty() {
            return Err(AppError::Invalid(
                "S3 桶名不能为空（同步通道配置缺陷）".to_string(),
            ));
        }
        // 空白归一与 endpoint / bucket / prefix 同规：区域参与 SigV4 签名范围，
        // 带首尾空白会签出与桩/服务端不一致的凭据范围（只报鉴权失败，根因不可见）。
        let region = config.region.trim().to_string();
        if region.is_empty() {
            return Err(AppError::Invalid(
                "S3 区域不能为空（同步通道配置缺陷）".to_string(),
            ));
        }
        let prefix = config.prefix.trim().trim_matches('/').to_string();
        if !prefix.is_empty() {
            validate_logical_path(&prefix)?;
        }
        let client = Client::from_conf(
            aws_sdk_s3::Config::builder()
                .behavior_version(BehaviorVersion::latest())
                .region(Region::new(region))
                .credentials_provider(Credentials::new(
                    config.access_key,
                    config.secret_key,
                    None,
                    None,
                    "ledger",
                ))
                .endpoint_url(endpoint)
                .force_path_style(config.path_style)
                // 兼容优先：只在服务端要求时计算/校验，避免对第三方兼容服务
                // 追加 AWS 专有校验和扩展；显式配置，不依赖环境变量。
                .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
                .response_checksum_validation(ResponseChecksumValidation::WhenRequired)
                .build(),
        );
        let bridge = AsyncBridge::spawn()?;
        Ok(Self {
            client,
            bucket,
            prefix,
            bridge,
        })
    }

    /// 逻辑路径 → 对象键（前缀归一化后拼接；空前缀直接使用逻辑路径）。
    fn object_key(&self, path: &str) -> Result<String> {
        validate_logical_path(path)?;
        if self.prefix.is_empty() {
            Ok(path.to_string())
        } else {
            Ok(format!("{}/{}", self.prefix, path))
        }
    }
}

impl Transport for S3Transport {
    fn ensure_dir(&self, path: &str) -> Result<()> {
        // 对象存储无目录：仅做路径守卫，零网络请求、零副作用。
        validate_logical_path(path)
    }

    fn read_file(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let key = self.object_key(path)?;
        let client = self.client.clone();
        let bucket = self.bucket.clone();
        self.bridge.call(move |rt| {
            rt.block_on(async move {
                match client.get_object().bucket(bucket).key(key).send().await {
                    Ok(output) => {
                        let bytes = output
                            .body
                            .collect()
                            .await
                            .map_err(|e| network_failed_error(&e.to_string()))?
                            .into_bytes()
                            .to_vec();
                        Ok(Some(bytes))
                    }
                    Err(err) if get_object_missing(&err) => Ok(None),
                    Err(err) => Err(sdk_error(&err)),
                }
            })
        })?
    }

    fn write_file(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let key = self.object_key(path)?;
        let client = self.client.clone();
        let bucket = self.bucket.clone();
        let payload = bytes.to_vec();
        self.bridge.call(move |rt| {
            rt.block_on(async move {
                if payload.len() >= MULTIPART_THRESHOLD_BYTES {
                    write_multipart(&client, &bucket, &key, payload).await
                } else {
                    client
                        .put_object()
                        .bucket(bucket)
                        .key(key)
                        .body(ByteStream::from(payload))
                        .send()
                        .await
                        .map(|_| ())
                        .map_err(|e| sdk_error(&e))
                }
            })
        })?
    }
}

/// 分片上传：Create → UploadPart×N → Complete；任一步失败尽力 Abort。
async fn write_multipart(client: &Client, bucket: &str, key: &str, payload: Vec<u8>) -> Result<()> {
    let created = client
        .create_multipart_upload()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| sdk_error(&e))?;
    let upload_id = created
        .upload_id()
        .ok_or_else(|| AppError::Invalid("S3 分片上传响应缺少 UploadId".to_string()))?
        .to_string();

    let result = upload_parts_and_complete(client, bucket, key, &upload_id, payload).await;
    if result.is_err() {
        // 清理失败不覆盖原始错误：分片残留可被服务端生命周期规则回收。
        let _ = client
            .abort_multipart_upload()
            .bucket(bucket)
            .key(key)
            .upload_id(&upload_id)
            .send()
            .await;
    }
    result
}

async fn upload_parts_and_complete(
    client: &Client,
    bucket: &str,
    key: &str,
    upload_id: &str,
    payload: Vec<u8>,
) -> Result<()> {
    let mut parts = Vec::new();
    for (index, chunk) in payload.chunks(MULTIPART_PART_BYTES).enumerate() {
        let part_number = i32::try_from(index + 1)
            .map_err(|_| AppError::Invalid("S3 分片序号超出范围".to_string()))?;
        let output = client
            .upload_part()
            .bucket(bucket)
            .key(key)
            .upload_id(upload_id)
            .part_number(part_number)
            .content_length(chunk.len() as i64)
            .body(ByteStream::from(chunk.to_vec()))
            .send()
            .await
            .map_err(|e| sdk_error(&e))?;
        let e_tag = output
            .e_tag()
            .ok_or_else(|| AppError::Invalid("S3 分片上传响应缺少 ETag".to_string()))?
            .to_string();
        parts.push(
            CompletedPart::builder()
                .part_number(part_number)
                .e_tag(e_tag)
                .build(),
        );
    }

    client
        .complete_multipart_upload()
        .bucket(bucket)
        .key(key)
        .upload_id(upload_id)
        .multipart_upload(
            CompletedMultipartUpload::builder()
                .set_parts(Some(parts))
                .build(),
        )
        .send()
        .await
        .map(|_| ())
        .map_err(|e| sdk_error(&e))
}

/// GetObject 的 404 归一：S3 无对象语义是 [`Transport::read_file`] 的 `None`
/// 常态输入，不是错误。XML `NoSuchKey` 与裸 404 状态都收口到这里。
///
/// **`NoSuchBucket` 必须排除在外**：桶不存在同样是 404，把它当成 `None` 会让
/// 「桶名写错」的端一路读空、静默当作「通道上还没有数据」（issue #1219 错误
/// 分层）；它走 [`sdk_error`] 归 [`target_missing_error`]。
fn get_object_missing(err: &SdkError<GetObjectError>) -> bool {
    if service_code(err) == Some("NoSuchBucket") {
        return false;
    }
    err.raw_response()
        .map(|raw| raw.status().as_u16() == 404)
        .unwrap_or(false)
        || matches!(err.as_service_error(), Some(GetObjectError::NoSuchKey(_)))
}

/// 凭据类服务端错误码：S3 对「密钥不存在 / 签名不符 / 临时凭据过期」回 403 而非
/// 401，按错误码把它们与「凭据有效但没权限」（`AccessDenied`）分开——前者让用户
/// 改密钥、后者让用户改授权，下一步动作不同（issue #1219 错误分层）。
const CREDENTIAL_ERROR_CODES: &[&str] = &[
    "InvalidAccessKeyId",
    "SignatureDoesNotMatch",
    "ExpiredToken",
    "InvalidToken",
    "TokenRefreshRequired",
];

/// 服务端错误码（XML `<Code>` 字段，如 `NoSuchBucket` / `AccessDenied`）；
/// 非服务端错误（连接失败、构造失败等）回 `None`。
fn service_code<E: ProvideErrorMetadata>(err: &SdkError<E>) -> Option<&str> {
    err.as_service_error().and_then(|error| error.code())
}

/// SDK 错误 → 域码化错误：按「凭据错 / 目标不存在 / 权限不足 / 网络不可达 /
/// 服务异常」五层归类（issue #1219）。不裸上抛 SDK 细节给用户层以外的调用方。
fn sdk_error<E: ProvideErrorMetadata>(err: &SdkError<E>) -> AppError {
    if let Some(raw) = err.raw_response() {
        return classify_http_failure(raw.status().as_u16(), service_code(err));
    }
    match err {
        SdkError::ConstructionFailure(_) => AppError::Invalid(format!("S3 请求构造失败: {err}")),
        _ => network_failed_error(&err.to_string()),
    }
}

/// 五层归类的单点：先按服务端错误码（同一 403 下 `AccessDenied` 是权限、
/// `InvalidAccessKeyId` 是凭据），码未给出时退回 HTTP 状态（401 凭据 / 403 权限 /
/// 其余服务异常）。
fn classify_http_failure(status: u16, code: Option<&str>) -> AppError {
    if let Some(code) = code {
        match code {
            "NoSuchBucket" => return target_missing_error(),
            "AccessDenied" => return permission_denied_error(),
            _ if CREDENTIAL_ERROR_CODES.contains(&code) => return auth_failed_error(),
            _ => {}
        }
    }
    match status {
        401 => auth_failed_error(),
        403 => permission_denied_error(),
        _ => http_failed_error(status, "S3 请求失败"),
    }
}

/// 同步 → 异步桥接的任务类型：在专用 runtime 上执行并回传结果。
type BridgeJob = Box<dyn FnOnce(&tokio::runtime::Runtime) + Send + 'static>;

/// 专用线程 + current-thread Tokio runtime。`call` 阻塞等待该线程执行任务；
/// Drop 时关闭通道并加入线程，保证 runtime 生命周期不泄漏。
struct AsyncBridge {
    tx: Option<mpsc::Sender<BridgeJob>>,
    join: Option<thread::JoinHandle<()>>,
}

impl AsyncBridge {
    fn spawn() -> Result<Self> {
        // runtime 在专用线程启动前构建：构建失败必须在此处报错，不能留下一个
        // 没有执行者的通道让后续调用永久阻塞（fail loud）。
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| AppError::Invalid(format!("S3 传输运行时初始化失败: {e}")))?;
        let (tx, rx) = mpsc::channel::<BridgeJob>();
        let join = thread::Builder::new()
            .name("ledger-s3-transport".to_string())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    job(&runtime);
                }
            })
            .map_err(|e| network_failed_error(&e.to_string()))?;
        Ok(Self {
            tx: Some(tx),
            join: Some(join),
        })
    }

    fn call<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&tokio::runtime::Runtime) -> T + Send + 'static,
    {
        let (result_tx, result_rx) = mpsc::channel();
        self.tx
            .as_ref()
            .ok_or_else(|| AppError::Invalid("S3 传输桥接已关闭".to_string()))?
            .send(Box::new(move |runtime| {
                let _ = result_tx.send(task(runtime));
            }))
            .map_err(|_| AppError::Invalid("S3 传输桥接线程不可用".to_string()))?;
        result_rx
            .recv()
            .map_err(|_| AppError::Invalid("S3 传输桥接线程未返回结果".to_string()))
    }
}

impl Drop for AsyncBridge {
    fn drop(&mut self) {
        // 先断开发送端让桥接线程退出循环，再等待其结束；Drop 不与 runtime 耦合。
        self.tx.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
