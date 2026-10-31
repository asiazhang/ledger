//! 账本身份（ADR-0144 决策 1/5，issue #1984）：账本指纹与产物来源设备标识。
//!
//! - **账本指纹**（词汇表：参考数据与设置域「账本指纹（Book Fingerprint）」）：
//!   账本跳设备、跳目录的稳定身份，随数据走而不随路径走。内容锚取库内最早
//!   一笔交易的全局唯一标识（排序口径与交易列表行序同款：
//!   `date ASC, created_at ASC, id ASC`，排除软删——跨端恢复或同步后必然
//!   相同），首用算出即固化进 AppSettings（[`SettingKey::BookFingerprint`]，
//!   零 schema 迁移），此后恒读固化值；空库（尚无交易）退化为端内生成随机
//!   标识（同款「首用生成并持久化」语义见多端同步域 DeviceId）。同一逻辑
//!   账本在任意设备、任意目录下指纹相同——句柄与路径不参与计算。
//! - **来源设备标识**：多端同步域 DeviceId（`ledger_sync_protocol::device`，
//!   安装实例级，重装即新设备，ADR-0091）。取不到（attached 侧异常）时该轮
//!   产物不带设备水印 = 来源未知，不阻断备份（ADR-0144 决策 5）。
//!
//! 两者都是备份产物的自述水印（内层元数据明文 `fingerprint` / `device_id`，
//! 只含标识不含密钥）；固化的设置写入走 AppSettings 置脏豁免通道（ADR-0032：
//! 设置写入不算「账本数据变化」，不该触发待备份置脏）。

use rusqlite::{Connection, OptionalExtension};

use ledger_infra::db::new_uuid;
use ledger_infra::error::Result;
use ledger_infra::settings::{self, SettingKey};

/// 库内最早一笔交易的全局唯一标识（内容锚，ADR-0144 决策 1）：排序口径与
/// 交易列表行序同款（`date ASC, created_at ASC, id ASC`，id 最终 tiebreaker），
/// 排除软删行；空库返回 `None`。指纹首用与产物归属第三顺位（存量产物解包
/// 归并，issue #1986）共用同款算法——单一事实源。
///
/// 行序口径出处：交易域读路径 order 模块（`ledger_transaction::read::order`，
/// 与列表 `date DESC/created_at DESC/id DESC` 同键反向；口径陈述见该模块
/// `get_transaction_order_summary` 注记）。锁定测试：`tests.rs` 的
/// `content_anchor_earliest_follows_transaction_row_order`（乱序 created_at、
/// 同日多行、含软删——删除排序键或软删过滤即红）。
pub(crate) fn content_anchor(conn: &Connection) -> Result<Option<String>> {
    let id: Option<String> = conn
        .query_row(
            // 行序口径见本函数文档：与交易域列表/订单读路径同键反向，软删排除；
            // 改动本 SQL 前先过上述锁定测试。
            "SELECT id FROM transactions WHERE is_deleted = 0 \
             ORDER BY date ASC, created_at ASC, id ASC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(id)
}

/// 当前账本指纹（写侧唯一入口）：已固化恒读固化值；未固化则按内容锚计算
///（空库退化为端内生成 UUIDv7）并固化，算出即恒定。调用方持连接锁（备份
/// 产出段在锁内），读后写无竞态。
pub fn book_fingerprint(conn: &Connection) -> Result<String> {
    let fixed: Option<String> = settings::get(conn, SettingKey::BookFingerprint, None)?;
    if let Some(fp) = fixed {
        return Ok(fp);
    }
    let fp = content_anchor(conn)?.unwrap_or_else(new_uuid);
    settings::set(conn, SettingKey::BookFingerprint, &fp)?;
    tracing::debug!(fingerprint = %fp, "账本指纹首用固化");
    Ok(fp)
}

/// 只读指纹判定（列表/清理/命名等读路径消费，issue #1985）：已固化恒读固化
/// 值；未固化回内容锚——与 [`book_fingerprint`] 首用算法同源，固化前两者恒
/// 等（归属判定据此即时成立，固化仍由备份写出路径首用完成）；空库且未固化
/// = `None`（该现场本账尚无任何可归属产物）。不写设置（读连接只读）。
pub fn book_fingerprint_readonly(conn: &Connection) -> Result<Option<String>> {
    let fixed: Option<String> = settings::get(conn, SettingKey::BookFingerprint, None)?;
    if fixed.is_some() {
        return Ok(fixed);
    }
    content_anchor(conn)
}

/// 产物来源设备标识：多端同步域 DeviceId（首用生成并持久化，与同步写路径
/// 同款语义）；获取失败（attached 侧异常）返回 `None`——该轮产物不带设备
/// 水印（来源未知），不阻断备份（ADR-0144 决策 5）。
///
/// **只读路径边界**（spec #1982 review 留痕，行为不改）：本函数也被只读
/// 连接消费——清理/列表的作用域构造（`engine::BackupScope::of_conn`，
/// prune/list 读路径）。DeviceId 首用是 INSERT（`sync_device` 无行时），而
/// 只读连接写不进去：升级后本机未做过任何新备份/同步写即清理时，这里取
/// 不到设备标识 → 作用域 `device_id = None` → 本机桶判空、不删。结果自洽
///（该现场目录里也不可能有本机新命名产物，升级存量扁平产物不带设备水印
/// 本就不进桶，「保守不删」正是预期），但属行为巧合对齐而非机制保证——若
/// 未来要求只读路径也能首用 DeviceId，需把首用写收口到写路径。
pub fn source_device_id(conn: &Connection) -> Option<String> {
    match ledger_sync_protocol::device::device_id(conn) {
        Ok(id) => Some(id),
        Err(e) => {
            tracing::warn!(error = %e, "设备标识不可得，本轮产物将不带来源设备水印");
            None
        }
    }
}
