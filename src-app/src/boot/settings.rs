//! 设置接缝（#2053）：日志档位读写（基建 settings 共享接缝直调，运行期
//! 滤镜接管即生效）、DataLocation 信息聚合与更改意图 / 恢复默认直调
//! 基础设施引导层三步校验与意图落盘。

use std::path::Path;

use ledger_infra::db::DbState;
use ledger_infra::error::{AppError, Result};

use super::CODE_LOG_LEVEL_INVALID;
use super::boot_record;
use crate::logging;

// ---------------------------------------------------------------------------
// 设置接缝（#2053）：日志档位读写（基建 settings 共享接缝直调，运行期滤镜
// 经 crate::logging 接管）与 DataLocation 信息聚合 / 更改意图（基础设施
// data_location 直调——旧壳 get_log_level / set_log_level / get_data_location_
// info / submit_data_location_change / restore_default_data_location 命令消费
// 的同一域函数族，不经 IPC；码化错误沿旧壳同码同文案，用户可见语义一致）。
// 读写接缝零新口径：档位值域闭集在基建常量单点，默认档位回退口径与旧壳
// persisted_level 同形；DataLocation 校验三步 / 二选一/ 目录唯一性住基础设施
// 引导层，本壳不自建守卫语义（默认数据目录 = 启动参数账本目录——身份即目录，
// 立桩引导契约在设置面的延伸）。文件 IO 是阻塞面：调用方在 blocking 任务内
// 消费（卡流程同交易表单/备份先例）。写连接是成对建连的写侧。
// ---------------------------------------------------------------------------

/// 读取持久化日志档位（旧壳 `get_log_level` 命令同一执行协议）：缺 key /
/// 缺 `app_settings` 表（旧版本备份）回默认 info；库内残留闭集外字符串回
/// 默认 info 并告警（读路径不因坏值上抛）。读连接是只读侧。
pub fn log_level(db: &DbState) -> Result<String> {
    Ok(logging::persisted_level(db))
}

/// 设置日志档位（旧壳 `set_log_level` 命令同一执行协议）：闭集校验（错误码
/// [`CODE_LOG_LEVEL_INVALID`]〔旧壳同码同文案〕「日志等级非法：未知日志档位:
/// `x`」，未落库、未接管）→ 基建设置接缝落库（settings 单点收口，设置 KV
/// 不置脏——ADR-0032 豁免随接缝自带）→ 运行期滤镜接管（crate::logging，切档
/// 立即生效）。写连接是成对建连的写侧。
pub fn set_log_level(db: &DbState, level_str: &str) -> Result<()> {
    if !ledger_infra::settings::LOG_LEVEL_DIRECTIVES.contains(&level_str) {
        return Err(AppError::codedp(
            CODE_LOG_LEVEL_INVALID,
            format!("日志等级非法：未知日志档位: `{level_str}`"),
            &[level_str],
        ));
    }
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_infra::settings::set(
        &conn,
        ledger_infra::settings::SettingKey::LogLevel,
        &level_str,
    )?;
    // 闭集内指令恒可解析；解析失败走告警路径（滤镜未登记 / 换档失败不回写）。
    if let Some(level) = logging::parse_directive(level_str) {
        logging::set_level(level);
    }
    Ok(())
}

/// DataLocation 当前信息聚合（旧壳 `get_data_location_info` 命令同一执行协议
/// [`data_location::gather_info_from_boot`]）：引导记录缺失时按出厂降级呈现
/// 生效目录 = 默认目录（异常时序的诚实降级，旧壳同口径）。零库操作（意图读
/// 取是文件 IO：默认目录下引导文件）。
pub fn data_location_info(launch_dir: &Path) -> ledger_infra::db::data_location::DataLocationInfo {
    ledger_infra::db::data_location::gather_info_from_boot(launch_dir, boot_record().as_ref())
}

/// 提交更改位置意图（旧壳 `submit_data_location_change` 命令同一执行协议）：
/// 引导态预检（推迟搬迁窗口拦截）→ 三步校验与意图落盘全在基础设施
/// [`data_location::validate_and_commit`] 单正文（自动建目录 / 试写探针 /
/// 目标已有同名库二选一信号不做静默覆盖）。真实搬迁只发生在下次启动
/// （plan_boot 消费意图），文案必须讲清「下次启动生效」。文件 IO（建目录 /
/// 试写 / 落盘引导文件）为阻塞面。
pub fn submit_data_location_change(
    launch_dir: &Path,
    target_dir: &str,
    adopt_existing: bool,
) -> Result<ledger_infra::db::data_location::DataLocationChangeOutcome> {
    ledger_infra::db::data_location::ensure_relocation_settled(boot_record().as_ref())?;
    let trimmed = target_dir.trim();
    if trimmed.is_empty() {
        return Err(AppError::coded(
            "data-location.dir-required",
            "目标目录不能为空",
        ));
    }
    ledger_infra::db::data_location::validate_and_commit(
        launch_dir,
        Path::new(trimmed),
        adopt_existing,
    )
}

/// 恢复默认位置（旧壳 `restore_default_data_location` 命令同一执行协议）：
/// 目标 = 本壳默认数据目录（启动参数目录），校验与写意图机制与更改完全相同
/// （二选一信号 / 目录唯一性拒绝随同一入口）。
pub fn restore_default_data_location(
    launch_dir: &Path,
    adopt_existing: bool,
) -> Result<ledger_infra::db::data_location::DataLocationChangeOutcome> {
    ledger_infra::db::data_location::ensure_relocation_settled(boot_record().as_ref())?;
    ledger_infra::db::data_location::validate_and_commit(launch_dir, launch_dir, adopt_existing)
}
