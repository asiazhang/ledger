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

use ledger_infra::error::{AppError, Result};

use super::{
    Transport, auth_failed_error, http_failed_error, network_failed_error, permission_denied_error,
    target_missing_error, validate_logical_path,
};

const MULTIPART_THRESHOLD_BYTES: usize = 8 * 1024 * 1024;

const MULTIPART_PART_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Config {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    pub prefix: String,
    pub path_style: bool,
}

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

fn get_object_missing(err: &SdkError<GetObjectError>) -> bool {
    if service_code(err) == Some("NoSuchBucket") {
        return false;
    }
    err.raw_response()
        .map(|raw| raw.status().as_u16() == 404)
        .unwrap_or(false)
        || matches!(err.as_service_error(), Some(GetObjectError::NoSuchKey(_)))
}

const CREDENTIAL_ERROR_CODES: &[&str] = &[
    "InvalidAccessKeyId",
    "SignatureDoesNotMatch",
    "ExpiredToken",
    "InvalidToken",
    "TokenRefreshRequired",
];

fn service_code<E: ProvideErrorMetadata>(err: &SdkError<E>) -> Option<&str> {
    err.as_service_error().and_then(|error| error.code())
}

fn sdk_error<E: ProvideErrorMetadata>(err: &SdkError<E>) -> AppError {
    if let Some(raw) = err.raw_response() {
        return classify_http_failure(raw.status().as_u16(), service_code(err));
    }
    match err {
        SdkError::ConstructionFailure(_) => AppError::Invalid(format!("S3 请求构造失败: {err}")),
        _ => network_failed_error(&err.to_string()),
    }
}

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

type BridgeJob = Box<dyn FnOnce(&tokio::runtime::Runtime) + Send + 'static>;

type ThreadSpawner = fn(Box<dyn FnOnce() + Send>) -> std::io::Result<thread::JoinHandle<()>>;

type BridgeStartup = std::result::Result<(), AppError>;

pub(crate) fn transport_runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| AppError::Invalid(format!("S3 传输运行时初始化失败: {e}")))
}

pub(crate) fn spawn_bridge_thread(
    body: Box<dyn FnOnce() + Send>,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("ledger-s3-transport".to_string())
        .spawn(body)
}

pub(crate) struct AsyncBridge {
    tx: Option<mpsc::Sender<BridgeJob>>,
    join: Option<thread::JoinHandle<()>>,
}

impl AsyncBridge {
    fn spawn() -> Result<Self> {
        Self::spawn_with(transport_runtime, spawn_bridge_thread)
    }

    pub(crate) fn spawn_with(
        build: impl FnOnce() -> Result<tokio::runtime::Runtime> + Send + 'static,
        spawn: ThreadSpawner,
    ) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<BridgeJob>();
        let (ready_tx, ready_rx) = mpsc::channel::<BridgeStartup>();
        let join = spawn(Box::new(move || {
            let runtime = match build() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                    return;
                }
            };
            if ready_tx.send(Ok(())).is_err() {
                return;
            }
            while let Ok(job) = rx.recv() {
                job(&runtime);
            }
        }))
        .map_err(|e| network_failed_error(&e.to_string()))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                tx: Some(tx),
                join: Some(join),
            }),
            Ok(Err(error)) => {
                let _ = join.join();
                Err(error)
            }
            Err(_) => {
                let _ = join.join();
                Err(AppError::Invalid("S3 传输桥接线程启动失败".to_string()))
            }
        }
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
        self.tx.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
