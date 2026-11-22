use cucumber::then;
use rusqlite::params;

use ledger_scheduled::ScheduledKind;
use ledger_transaction::{
    TransactionListFilter, TransactionSource, TransactionSourceKind, TransactionSourceStatus,
    list_transactions_internal,
};

use crate::world::LedgerWorld;

fn policy_id_by_number(world: &LedgerWorld, number: &str) -> String {
    world_conn!(world)
        .query_row(
            "SELECT id FROM policies WHERE policy_number=?1",
            params![number],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| panic!("来源断言：保单 {number} 应已存在"))
}

fn assert_policy_source(
    world: &LedgerWorld,
    source: &ledger_transaction::TransactionSource,
    policy_number: &str,
    product_name: &str,
    expected_status: Option<TransactionSourceStatus>,
) {
    assert_eq!(
        source.kind,
        TransactionSourceKind::Policy,
        "来源类型应为保单"
    );
    assert_eq!(
        source.entity_id,
        policy_id_by_number(world, policy_number),
        "来源实体 id 应为挂单保单 id"
    );
    assert_eq!(source.display_name, product_name, "来源展示名应为险种名");
    assert_eq!(source.status, expected_status, "来源状态标注不匹配");
}

fn assert_list_nth_policy_source(
    world: &mut LedgerWorld,
    index: usize,
    policy_number: &str,
    product_name: &str,
    expected_status: Option<TransactionSourceStatus>,
) {
    let result = list_transactions_internal(&world_conn!(world), &TransactionListFilter::default())
        .expect("交易列表查询失败");
    let txn = result
        .items
        .get(index - 1)
        .unwrap_or_else(|| panic!("交易列表第 {index} 条不存在"));
    let source = txn
        .source
        .as_ref()
        .unwrap_or_else(|| panic!("第 {index} 条交易应携带来源，实际为空"));
    assert_policy_source(world, source, policy_number, product_name, expected_status);
}

#[then(expr = "交易列表第 {int} 条来源应为保单 {string} 险种 {string}")]
fn list_nth_source_policy(
    world: &mut LedgerWorld,
    index: usize,
    policy_number: String,
    product_name: String,
) {
    assert_list_nth_policy_source(world, index, &policy_number, &product_name, None);
}

#[then(expr = "交易列表第 {int} 条来源应为已删除保单 {string} 险种 {string}")]
fn list_nth_source_deleted_policy(
    world: &mut LedgerWorld,
    index: usize,
    policy_number: String,
    product_name: String,
) {
    assert_list_nth_policy_source(
        world,
        index,
        &policy_number,
        &product_name,
        Some(TransactionSourceStatus::Deleted),
    );
}

fn assert_plan_source(
    world: &LedgerWorld,
    source: &TransactionSource,
    kind: ScheduledKind,
    note: &str,
    expected_status: Option<TransactionSourceStatus>,
) {
    let expected_kind = match kind {
        ScheduledKind::Installment => TransactionSourceKind::InstallmentPlan,
        ScheduledKind::Subscription => TransactionSourceKind::Subscription,
        ScheduledKind::ScheduledTransfer => TransactionSourceKind::ScheduledTransfer,
    };
    let plan_id = world_conn!(world)
        .query_row(
            "SELECT id FROM scheduled_transactions WHERE note=?1",
            params![note],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| panic!("来源断言：备注为 {note} 的计划应已存在"));
    assert_eq!(source.kind, expected_kind, "来源类型应为计划形态 {kind}");
    assert_eq!(
        source.entity_id, plan_id,
        "来源实体 id 应为生成该期次的计划 id"
    );
    assert_eq!(source.display_name, note, "来源展示名应为计划名（备注）");
    assert_eq!(source.status, expected_status, "来源状态标注不匹配");
}

#[derive(Clone, Copy)]
enum NthList {
    List,
    Search,
}

fn nth_source(world: &mut LedgerWorld, list: NthList, index: usize) -> TransactionSource {
    let source = match list {
        NthList::List => {
            let result =
                list_transactions_internal(&world_conn!(world), &TransactionListFilter::default())
                    .expect("交易列表查询失败");
            result.items.into_iter().nth(index - 1)
        }
        NthList::Search => world
            .txn
            .last_search
            .clone()
            .expect("搜索结果快照缺失（先执行搜索步骤）")
            .items
            .into_iter()
            .nth(index - 1),
    };
    let txn = source.unwrap_or_else(|| panic!("目标快照第 {index} 条不存在"));
    txn.source
        .unwrap_or_else(|| panic!("第 {index} 条交易应携带来源，实际为空"))
}

fn assert_nth_plan_source(
    world: &mut LedgerWorld,
    list: NthList,
    index: usize,
    kind: ScheduledKind,
    note: &str,
    expected_status: Option<TransactionSourceStatus>,
) {
    let source = nth_source(world, list, index);
    assert_plan_source(world, &source, kind, note, expected_status);
}

#[then(expr = "交易列表第 {int} 条来源应为分期计划 备注 {string}")]
fn list_nth_source_installment(world: &mut LedgerWorld, index: usize, note: String) {
    assert_nth_plan_source(
        world,
        NthList::List,
        index,
        ScheduledKind::Installment,
        &note,
        None,
    );
}

#[then(expr = "交易列表第 {int} 条来源应为订阅计划 备注 {string}")]
fn list_nth_source_subscription(world: &mut LedgerWorld, index: usize, note: String) {
    assert_nth_plan_source(
        world,
        NthList::List,
        index,
        ScheduledKind::Subscription,
        &note,
        None,
    );
}

#[then(expr = "交易列表第 {int} 条来源应为定时转账计划 备注 {string}")]
fn list_nth_source_transfer(world: &mut LedgerWorld, index: usize, note: String) {
    assert_nth_plan_source(
        world,
        NthList::List,
        index,
        ScheduledKind::ScheduledTransfer,
        &note,
        None,
    );
}

#[then(expr = "交易列表第 {int} 条来源应为已取消订阅计划 备注 {string}")]
fn list_nth_source_cancelled_subscription(world: &mut LedgerWorld, index: usize, note: String) {
    assert_nth_plan_source(
        world,
        NthList::List,
        index,
        ScheduledKind::Subscription,
        &note,
        Some(TransactionSourceStatus::Cancelled),
    );
}

#[then(expr = "交易列表第 {int} 条来源应为无备注订阅计划 展示名为空")]
fn list_nth_source_subscription_without_note(world: &mut LedgerWorld, index: usize) {
    let result = list_transactions_internal(&world_conn!(world), &TransactionListFilter::default())
        .expect("交易列表查询失败");
    let txn = result
        .items
        .get(index - 1)
        .unwrap_or_else(|| panic!("交易列表第 {index} 条不存在"));
    let source = txn
        .source
        .as_ref()
        .unwrap_or_else(|| panic!("第 {index} 条交易应携带来源，实际为空"));
    assert_eq!(
        source.kind,
        TransactionSourceKind::Subscription,
        "来源类型应为订阅计划"
    );
    let plan_id = world_conn!(world)
        .query_row(
            "SELECT id FROM scheduled_transactions WHERE note IS NULL",
            [],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| panic!("来源断言：无备注计划应已存在"));
    assert_eq!(source.entity_id, plan_id, "来源实体 id 应为计划 id");
    assert_eq!(source.display_name, "", "无备注计划展示名应为空串");
    assert_eq!(source.status, None, "进行中计划不应带状态标注");
}

#[then(expr = "搜索结果第 {int} 条来源应为订阅计划 备注 {string}")]
fn search_nth_source_subscription(world: &mut LedgerWorld, index: usize, note: String) {
    assert_nth_plan_source(
        world,
        NthList::Search,
        index,
        ScheduledKind::Subscription,
        &note,
        None,
    );
}

#[then(expr = "交易列表第 {int} 条应无来源")]
fn list_nth_no_source(world: &mut LedgerWorld, index: usize) {
    let result = list_transactions_internal(&world_conn!(world), &TransactionListFilter::default())
        .expect("交易列表查询失败");
    let txn = result
        .items
        .get(index - 1)
        .unwrap_or_else(|| panic!("交易列表第 {index} 条不存在"));
    assert!(
        txn.source.is_none(),
        "第 {index} 条交易应无来源，实际: {:?}",
        txn.source
    );
}

#[then(expr = "搜索结果第 {int} 条来源应为保单 {string} 险种 {string}")]
fn search_nth_source_policy(
    world: &mut LedgerWorld,
    index: usize,
    policy_number: String,
    product_name: String,
) {
    let snapshot = world
        .txn
        .last_search
        .clone()
        .expect("搜索结果快照缺失（先执行搜索步骤）");
    let txn = snapshot
        .items
        .get(index - 1)
        .unwrap_or_else(|| panic!("搜索结果第 {index} 条不存在"));
    let source = txn
        .source
        .as_ref()
        .unwrap_or_else(|| panic!("搜索结果第 {index} 条应携带来源，实际为空"));
    assert_policy_source(world, source, &policy_number, &product_name, None);
}

#[then(expr = "搜索结果第 {int} 条应无来源")]
fn search_nth_no_source(world: &mut LedgerWorld, index: usize) {
    let snapshot = world
        .txn
        .last_search
        .clone()
        .expect("搜索结果快照缺失（先执行搜索步骤）");
    let txn = snapshot
        .items
        .get(index - 1)
        .unwrap_or_else(|| panic!("搜索结果第 {index} 条不存在"));
    assert!(
        txn.source.is_none(),
        "搜索结果第 {index} 条应无来源，实际: {:?}",
        txn.source
    );
}

fn item_id_by_name(world: &LedgerWorld, name: &str) -> String {
    world_conn!(world)
        .query_row(
            "SELECT id FROM items WHERE name=?1 AND is_deleted=0",
            params![name],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| panic!("来源断言：物品 {name} 应已存在"))
}

fn assert_item_source(
    world: &LedgerWorld,
    source: &TransactionSource,
    name: &str,
    expected_status: Option<TransactionSourceStatus>,
) {
    assert_eq!(source.kind, TransactionSourceKind::Item, "来源类型应为物品");
    assert_eq!(
        source.entity_id,
        item_id_by_name(world, name),
        "来源实体 id 应为溯源物品 id"
    );
    assert_eq!(source.display_name, name, "来源展示名应为物品名");
    assert_eq!(source.status, expected_status, "来源状态标注不匹配");
}

#[then(expr = "交易列表第 {int} 条来源应为物品 {string}")]
fn list_nth_source_item(world: &mut LedgerWorld, index: usize, name: String) {
    let source = nth_source(world, NthList::List, index);
    assert_item_source(world, &source, &name, None);
}

#[then(expr = "交易列表第 {int} 条来源应为已处置物品 {string}")]
fn list_nth_source_disposed_item(world: &mut LedgerWorld, index: usize, name: String) {
    let source = nth_source(world, NthList::List, index);
    assert_item_source(
        world,
        &source,
        &name,
        Some(TransactionSourceStatus::Disposed),
    );
}

#[then(expr = "搜索结果第 {int} 条来源应为物品 {string}")]
fn search_nth_source_item(world: &mut LedgerWorld, index: usize, name: String) {
    let source = nth_source(world, NthList::Search, index);
    assert_item_source(world, &source, &name, None);
}

fn assert_instrument_source(
    world: &LedgerWorld,
    source: &TransactionSource,
    symbol: &str,
    name: &str,
) {
    let instrument_id: String = world_conn!(world)
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1",
            params![symbol],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| panic!("来源断言：标的 {symbol} 应已存在"));
    assert_eq!(
        source.kind,
        TransactionSourceKind::Instrument,
        "来源类型应为标的"
    );
    assert_eq!(
        source.entity_id, instrument_id,
        "来源实体 id 应为证券交易记录指向的标的 id"
    );
    let expected_label = format!("{symbol} {name}").trim().to_string();
    assert_eq!(
        source.display_name, expected_label,
        "来源展示名应为代码 + 名称（空格连接，无名称退化为裸代码）"
    );
    assert_eq!(source.status, None, "标的来源不应携带状态标注");
}

#[then(expr = "交易列表第 {int} 条来源应为标的 {string} 名称 {string}")]
fn list_nth_source_instrument(world: &mut LedgerWorld, index: usize, symbol: String, name: String) {
    let source = nth_source(world, NthList::List, index);
    assert_instrument_source(world, &source, &symbol, &name);
}

#[then(expr = "搜索结果第 {int} 条来源应为标的 {string} 名称 {string}")]
fn search_nth_source_instrument(
    world: &mut LedgerWorld,
    index: usize,
    symbol: String,
    name: String,
) {
    let source = nth_source(world, NthList::Search, index);
    assert_instrument_source(world, &source, &symbol, &name);
}
