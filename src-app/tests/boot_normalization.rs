//! 启动归一化外来形态明文库的壳接线集成测试（issue #1453 / ADR-0087 接线证明）。
//!
//! #2083 切轨随迁：原 Tauri 壳版断言相位（`get_boot_status` → `ready`）与
//! `restart_app` 原位重引导；NSH 壳的对应接线是 [`nsh_app::boot::boot_ledger`]
//! 一步可观察——外来形态库引导成功、保留字节归零、夹具数据完整、备份可用。
//! 相位机与失败恢复屏随旧壳退役（NSH 无相位面，窗口直接呈现错误）。
//!
//! 守卫是进程级单例：boot_record 槽是进程级量，本测试二进制只放这一个用例。
//!
//! 三层权威（ADR-0087）：探测与归一化的行为权威在基础设施单测
//!（`infra::db::encryption::tests`，含「归一化后 `VACUUM INTO` 可执行」的根因
//! 级证明）；本文件只钉壳层接线一步可观察的结果（引导成功 + 文件被归一化）。

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

use ledger_infra::db::{self};
use ledger_infra::test_utils::{
    FOREIGN_RESERVED_BYTES, read_reserved_byte, write_foreign_form_plaintext_db,
};
use nsh_app::test_support::isolate_home;

/// 验收判据（issue #1453，NSH 形态）：以畸形夹具引导 → `boot_ledger` 成功、
/// 库文件被归一化（偏移 20 = 0）、夹具数据经归一化后的真实连接仍可读、
/// 备份路径（`VACUUM INTO`）可用。
#[test]
fn boot_ledger_normalizes_foreign_form_plaintext_ledger() {
    isolate_home();
    let dir = std::env::temp_dir().join(format!(
        "ledger-test-boot-normalization-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join(db::data_location::DB_FILE_NAME);
    write_foreign_form_plaintext_db(&db_path, 3);
    assert_eq!(
        read_reserved_byte(&db_path),
        FOREIGN_RESERVED_BYTES,
        "前置：库文件应是外来形态（保留字节 12）"
    );

    let state = nsh_app::boot::boot_ledger(&dir).expect("外来形态明文库应经归一化引导成功");

    assert_eq!(
        read_reserved_byte(&db_path),
        0,
        "启动归一化应把库文件的每页保留字节归零（删除该接线即在此变红）"
    );
    // 归一化是整库重写：夹具数据必须完整地留在归一化后的库里，且应用连接读到
    // 的正是归一化后的库（一步可观察证明）。
    {
        let conn = state.conn.lock().unwrap();
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM fixture_probe", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 3, "归一化后的库存应完整保留夹具数据");
    }
    // 用户可观察结果之二：**备份可用**。`VACUUM INTO` 正是自动备份与多端同步
    // 检查点产出的同一条语句，也是现场「全灭」的那条路径——归一化没生效
    //（接线被删）时它照旧失败，本用例随之变红。
    let snapshot = dir.join("snapshot.db");
    {
        let conn = db::open_connection(&db_path).unwrap();
        conn.execute_batch(&format!("VACUUM INTO '{}'", snapshot.display()))
            .expect("归一化后的库应能产出快照（备份可用）");
    }
    assert_eq!(
        read_reserved_byte(&snapshot),
        0,
        "归一化后产出的快照不再携带外来保留字节"
    );

    std::fs::remove_dir_all(&dir).ok();
}
