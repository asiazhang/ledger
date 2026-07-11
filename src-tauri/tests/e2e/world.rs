use std::collections::HashMap;
use std::fmt;

use cucumber::World;
use rusqlite::Connection;

use tauri_app_lib::db::{init_db, open_in_memory};
use tauri_app_lib::models::Transaction;

/// Cucumber World：每个 Scenario 独立持有一个 in-memory SQLite 数据库。
#[derive(World)]
#[world(init = Self::new)]
pub struct LedgerWorld {
    /// In-memory 数据库连接（每个 scenario 独立）
    pub conn: Connection,
    /// 账户名称到 ID 的映射（Given 步骤插入账户后注册）
    pub account_name_to_id: HashMap<String, String>,
    /// 最新创建的交易 ID（用于关联操作如退款）
    pub last_transaction_id: Option<String>,
    /// 最近一次操作错误（检查失败场景）
    pub last_error: Option<String>,
    /// 交易列表快照（用于 Then 断言）
    pub transactions_list: Vec<Transaction>,
}

impl fmt::Debug for LedgerWorld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LedgerWorld")
            .field("account_count", &self.account_name_to_id.len())
            .field("last_transaction_id", &self.last_transaction_id)
            .field("last_error", &self.last_error)
            .field("transactions_count", &self.transactions_list.len())
            .finish()
    }
}

impl LedgerWorld {
    fn new() -> Self {
        let mut conn = open_in_memory().expect("无法创建内存数据库");
        init_db(&mut conn).expect("数据库初始化失败");
        Self {
            conn,
            account_name_to_id: HashMap::new(),
            last_transaction_id: None,
            last_error: None,
            transactions_list: Vec::new(),
        }
    }

    /// 获取账户 ID，按名称查找
    pub fn account_id(&self, name: &str) -> String {
        self.account_name_to_id
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("账户 '{}' 不存在", name))
    }
}
