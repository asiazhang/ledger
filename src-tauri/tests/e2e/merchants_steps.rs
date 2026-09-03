use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::error::{AppError, ErrClass};
use tauri_app_lib::merchants::{
    MerchantInput, MerchantUpdateInput, create_merchant as create_merchant_domain,
    delete_merchant as delete_merchant_domain, list_merchants as list_merchants_domain,
    update_merchant as update_merchant_domain,
};
use tauri_app_lib::models::TransactionInput;
use tauri_app_lib::transaction::amount::TransactionKind;
use tauri_app_lib::transaction::create_transaction_internal;

use crate::common::query_all_transactions;
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

#[given(expr = "存在商户 {string}")]
fn given_merchant(world: &mut LedgerWorld, name: String) {
    let id = create_merchant_domain(&world_conn!(world), MerchantInput { name: name.clone() })
        .expect("创建商户失败");
    world.merchant_name_to_id.insert(name, id);
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

/// 创建商户并断言成功（注册名称→ID 映射，供后续步骤按名称引用）。
/// 与 IPC 命令同形态：经连接层统一写入口（ADR-0032），成功即置脏。
#[when(expr = "创建商户 {string}")]
fn create_merchant(world: &mut LedgerWorld, name: String) {
    let id = world
        .db
        .write(|conn| create_merchant_domain(conn, MerchantInput { name: name.clone() }))
        .expect("创建商户失败");
    world.merchant_name_to_id.insert(name, id);
}

/// 尝试创建商户并捕获错误（供「应返回错误」断言）。
#[when(expr = "尝试创建商户 {string}")]
fn try_create_merchant(world: &mut LedgerWorld, name: String) {
    let result = create_merchant_domain(&world_conn!(world), MerchantInput { name });
    world.last_error = match result {
        Err(AppError::Coded {
            class: ErrClass::Invalid,
            message,
            ..
        }) => Some(message),
        Err(e) => Some(e.to_string()),
        Ok(_) => Some("预期失败但成功了".into()),
    };
}

/// 修改商户名称（改名即时生效：引用指向 id，不回刷历史交易行）。
#[when(expr = "修改商户 {string} 名称为 {string}")]
fn rename_merchant(world: &mut LedgerWorld, old_name: String, new_name: String) {
    let id = world.merchant_id(&old_name);
    update_merchant_domain(
        &world_conn!(world),
        &id,
        MerchantUpdateInput {
            name: Some(new_name.clone()),
        },
    )
    .expect("修改商户失败");
    world.merchant_name_to_id.remove(&old_name);
    world.merchant_name_to_id.insert(new_name, id);
}

/// 尝试改名并捕获错误（供「应返回错误」断言：改名撞在用同名被拒）。
#[when(expr = "尝试修改商户 {string} 名称为 {string}")]
fn try_rename_merchant(world: &mut LedgerWorld, old_name: String, new_name: String) {
    let id = world.merchant_id(&old_name);
    let result = update_merchant_domain(
        &world_conn!(world),
        &id,
        MerchantUpdateInput {
            name: Some(new_name),
        },
    );
    world.last_error = match result {
        Err(AppError::Coded {
            class: ErrClass::Invalid,
            message,
            ..
        }) => Some(message),
        Err(e) => Some(e.to_string()),
        Ok(()) => Some("预期失败但成功了".into()),
    };
}

/// 软删商户（不可再被新交易选择；历史引用保留）。
/// 名称→ID 映射刻意**保留**：软删后商户行仍在库中（历史引用语义），
/// 后续步骤可继续按名称引用其 id，由后端拒绝新交易携带（断言「商户不存在或已删除」）。
#[when(expr = "软删商户 {string}")]
fn delete_merchant(world: &mut LedgerWorld, name: String) {
    let id = world.merchant_id(&name);
    delete_merchant_domain(&world_conn!(world), &id).expect("软删商户失败");
}

/// 创建带商户的交易（expense/income/refund 可携带）。
#[when(expr = "创建交易 类型 {string} 金额 {int} 到账户 {string} 日期 {string} 商户 {string}")]
fn create_txn_with_merchant(
    world: &mut LedgerWorld,
    kind: String,
    amount: i64,
    account_name: String,
    date: String,
    merchant_name: String,
) {
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::parse(&kind).unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）")),
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: world.account_id(&account_name),
        to_account_id: None,
        category_id: None,
        merchant_id: Some(world.merchant_id(&merchant_name)),
        refund_of_transaction_id: None,
        note: None,
        date,
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    let result = create_transaction_internal(&world_conn!(world), input);
    assert!(result.is_ok(), "创建交易失败: {:?}", result.err());
    world.last_transaction_id = Some(result.unwrap().id);
    world.transactions_list = query_all_transactions(&world_conn!(world));
}

/// 尝试创建带商户的交易并捕获错误（供「应返回错误」断言）。
#[when(expr = "尝试创建交易 类型 {string} 金额 {int} 到账户 {string} 日期 {string} 商户 {string}")]
fn try_create_txn_with_merchant(
    world: &mut LedgerWorld,
    kind: String,
    amount: i64,
    account_name: String,
    date: String,
    merchant_name: String,
) {
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::parse(&kind).unwrap_or_else(|e| panic!("非法 kind: {kind}（{e}）")),
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: world.account_id(&account_name),
        to_account_id: None,
        category_id: None,
        merchant_id: Some(world.merchant_id(&merchant_name)),
        refund_of_transaction_id: None,
        note: None,
        date,
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    let result = create_transaction_internal(&world_conn!(world), input);
    world.last_error = match result {
        Err(AppError::Coded { message, .. }) => Some(message),
        _ => Some("预期失败但成功了".into()),
    };
}

/// 尝试创建带商户的转账并捕获错误（行为层 kind 收口：transfer 拒绝商户）。
#[when(expr = "尝试创建转账 金额 {int} 从账户 {string} 日期 {string} 商户 {string}")]
fn try_transfer_with_merchant(
    world: &mut LedgerWorld,
    amount: i64,
    account_name: String,
    date: String,
    merchant_name: String,
) {
    let input = TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Transfer,
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: world.account_id(&account_name),
        to_account_id: None,
        category_id: None,
        merchant_id: Some(world.merchant_id(&merchant_name)),
        refund_of_transaction_id: None,
        note: None,
        date,
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    };
    let result = create_transaction_internal(&world_conn!(world), input);
    world.last_error = match result {
        Err(AppError::Coded { message, .. }) => Some(message),
        _ => Some("预期失败但成功了".into()),
    };
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "商户表应存在且交易表含 merchant_id 列")]
fn check_schema_in_place(world: &mut LedgerWorld) {
    // merchants 表存在（迁移后 schema 就位，含 soft-delete 列）。
    let table: i64 = world_conn!(world)
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='merchants'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(table, 1, "merchants 表应存在");

    // transactions 表含 merchant_id 列（外键置空语义由 writer 校验兜底）。
    let col: i64 = world_conn!(world)
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('transactions') WHERE name='merchant_id'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(col, 1, "transactions 表应含 merchant_id 列");
}

#[then(expr = "商户列表应包含 {int} 条记录")]
fn check_merchant_count(world: &mut LedgerWorld, expected: i64) {
    let merchants = list_merchants_domain(&world_conn!(world), false).expect("查询商户失败");
    assert_eq!(
        merchants.len() as i64,
        expected,
        "商户列表数量不匹配（应为在用行，软删不计入）"
    );
}

#[then(expr = "商户列表应包含 {string}")]
fn check_merchant_contains(world: &mut LedgerWorld, name: String) {
    let merchants = list_merchants_domain(&world_conn!(world), false).expect("查询商户失败");
    assert!(
        merchants.iter().any(|m| m.name == name),
        "商户列表应包含 '{name}'，实际: {:?}",
        merchants
            .iter()
            .map(|m| m.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[then(expr = "商户列表应不包含 {string}")]
fn check_merchant_not_contains(world: &mut LedgerWorld, name: String) {
    let merchants = list_merchants_domain(&world_conn!(world), false).expect("查询商户失败");
    assert!(
        !merchants.iter().any(|m| m.name == name),
        "商户列表不应包含 '{name}'"
    );
}

/// 商户契约回归「名字字典」（issue #223）：列表响应序列化后不应再含指定字段
/// （icon/color 已退役；请求侧结构体无对应字段由编译期保证）。
#[then(expr = "商户列表响应 JSON 不含字段 {string}")]
fn check_merchant_json_not_contain_field(world: &mut LedgerWorld, field: String) {
    let merchants = list_merchants_domain(&world_conn!(world), false).expect("查询商户失败");
    assert!(!merchants.is_empty(), "商户列表为空，无法校验响应字段契约");
    for m in &merchants {
        let json = serde_json::to_value(m).expect("商户序列化失败");
        assert!(
            json.get(&field).is_none(),
            "商户响应不应含字段 '{field}'，实际: {json}"
        );
    }
}

/// 含软删全量列表（交易列表筛选下拉的数据源）：软删商户仍在其列，
/// 其历史交易照常可按商户过滤。
#[then(expr = "商户含软删列表应包含 {int} 条记录")]
fn check_merchant_all_count(world: &mut LedgerWorld, expected: i64) {
    let merchants =
        list_merchants_domain(&world_conn!(world), true).expect("查询含软删商户列表失败");
    assert_eq!(merchants.len() as i64, expected, "含软删商户列表数量不匹配");
}

#[then(expr = "商户含软删列表应包含 {string}")]
fn check_merchant_all_contains(world: &mut LedgerWorld, name: String) {
    let merchants =
        list_merchants_domain(&world_conn!(world), true).expect("查询含软删商户列表失败");
    assert!(
        merchants.iter().any(|m| m.name == name),
        "含软删商户列表应包含 '{name}'"
    );
}

/// 断言第 N 条交易（date DESC 排序）的商户名：按 merchant_id 实时解析
/// （历史引用保留 + 改名即时生效的读回语义，与前端经参考表解析同一口径）。
#[then(expr = "第 {int} 条交易商户应为 {string}")]
fn check_txn_merchant(world: &mut LedgerWorld, index: i64, merchant_name: String) {
    let idx = (index - 1) as usize;
    assert!(
        idx < world.transactions_list.len(),
        "交易列表只有 {} 条，无法访问第 {index} 条",
        world.transactions_list.len()
    );
    let txn = &world.transactions_list[idx];
    let mid = txn
        .merchant_id
        .as_deref()
        .unwrap_or_else(|| panic!("第 {index} 条交易商户应为 '{merchant_name}'，实际无商户",));
    let name: String = world_conn!(world)
        .query_row(
            "SELECT name FROM merchants WHERE id=?1",
            params![mid],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| panic!("商户行不存在: {mid}"));
    assert_eq!(name, merchant_name, "第 {index} 条交易商户不符");
}
