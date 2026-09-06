use cucumber::{given, then, when};

use tauri_app_lib::error::{AppError, ErrClass};
use tauri_app_lib::policy::{
    InsurerInput, InsurerUpdateInput, create_insurer, create_insurer_by_name,
    delete_insurer as delete_insurer_domain, list_insurers as list_insurers_domain,
    update_insurer as update_insurer_domain,
};

use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

#[given(expr = "存在保司 {string}")]
fn given_insurer(world: &mut LedgerWorld, name: String) {
    let id = create_insurer(&world_conn!(world), InsurerInput { name: name.clone() })
        .expect("创建保司失败");
    world.insurer_name_to_id.insert(name, id);
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

/// 创建保司并断言成功（注册名称→ID 映射，供后续步骤按名称引用）。
#[when(expr = "创建保司 {string}")]
fn create_insurer_step(world: &mut LedgerWorld, name: String) {
    let id = create_insurer(&world_conn!(world), InsurerInput { name: name.clone() })
        .expect("创建保司失败");
    world.insurer_name_to_id.insert(name, id);
}

/// 尝试创建保司并捕获错误（供「应返回错误」断言：创建撞在用同名被拒）。
#[when(expr = "尝试创建保司 {string}")]
fn try_create_insurer(world: &mut LedgerWorld, name: String) {
    let result = create_insurer(&world_conn!(world), InsurerInput { name });
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

/// 按名创建保司（find-or-create 即席创建语义，trim 归一）：未命中即建、精确命中复用。
/// 返回 id 记入 `last_insurer_by_name_id`（复用断言用），并注册名称→ID 映射。
#[when(expr = "按名创建保司 {string}")]
fn find_or_create_insurer(world: &mut LedgerWorld, name: String) {
    let id = create_insurer_by_name(&world_conn!(world), &name).expect("按名创建保司失败");
    world.last_insurer_by_name_id = Some(id.clone());
    world.insurer_name_to_id.insert(name, id);
}

/// 尝试按名创建保司并捕获错误（供「应返回错误」断言：trim 后空名被拒）。
#[when(expr = "尝试按名创建保司 {string}")]
fn try_find_or_create_insurer(world: &mut LedgerWorld, name: String) {
    let result = create_insurer_by_name(&world_conn!(world), &name);
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

/// 修改保司名称（改名即时生效：引用指向 id，不回刷历史行）。
#[when(expr = "修改保司 {string} 名称为 {string}")]
fn rename_insurer(world: &mut LedgerWorld, old_name: String, new_name: String) {
    let id = world.insurer_id(&old_name);
    update_insurer_domain(
        &world_conn!(world),
        &id,
        InsurerUpdateInput {
            name: Some(new_name.clone()),
        },
    )
    .expect("修改保司失败");
    world.insurer_name_to_id.remove(&old_name);
    world.insurer_name_to_id.insert(new_name, id);
}

/// 尝试改名并捕获错误（供「应返回错误」断言：改名撞在用同名被拒）。
#[when(expr = "尝试修改保司 {string} 名称为 {string}")]
fn try_rename_insurer(world: &mut LedgerWorld, old_name: String, new_name: String) {
    let id = world.insurer_id(&old_name);
    let result = update_insurer_domain(
        &world_conn!(world),
        &id,
        InsurerUpdateInput {
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

/// 软删保司（不进默认列表；含已删查询可见）。
/// 名称→ID 映射刻意**保留**：软删后保司行仍在库中（历史引用语义）。
#[when(expr = "软删保司 {string}")]
fn delete_insurer_step(world: &mut LedgerWorld, name: String) {
    let id = world.insurer_id(&name);
    delete_insurer_domain(&world_conn!(world), &id).expect("软删保司失败");
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "保司表应存在")]
fn check_insurer_table_exists(world: &mut LedgerWorld) {
    let table: i64 = world_conn!(world)
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='insurers'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(table, 1, "insurers 表应存在");
}

#[then(expr = "在用保司总数应为 {int}")]
fn check_insurer_count(world: &mut LedgerWorld, expected: i64) {
    let insurers = list_insurers_domain(&world_conn!(world), false).expect("查询保司失败");
    assert_eq!(
        insurers.len() as i64,
        expected,
        "保司列表数量不匹配（应为在用行，软删不计入；全新库含 30 条种子）"
    );
}

#[then(expr = "保司列表应包含 {string}")]
fn check_insurer_contains(world: &mut LedgerWorld, name: String) {
    let insurers = list_insurers_domain(&world_conn!(world), false).expect("查询保司失败");
    assert!(
        insurers.iter().any(|i| i.name == name),
        "保司列表应包含 '{name}'，实际: {:?}",
        insurers.iter().map(|i| i.name.as_str()).collect::<Vec<_>>()
    );
}

#[then(expr = "保司列表应不包含 {string}")]
fn check_insurer_not_contains(world: &mut LedgerWorld, name: String) {
    let insurers = list_insurers_domain(&world_conn!(world), false).expect("查询保司失败");
    assert!(
        !insurers.iter().any(|i| i.name == name),
        "保司列表不应包含 '{name}'"
    );
}

/// 含软删全量列表（保司管理「显示已删」切换的数据源）：软删保司仍在其列。
#[then(expr = "保司含已删列表应包含 {int} 条记录")]
fn check_insurer_all_count(world: &mut LedgerWorld, expected: i64) {
    let insurers = list_insurers_domain(&world_conn!(world), true).expect("查询含已删保司列表失败");
    assert_eq!(insurers.len() as i64, expected, "含已删保司列表数量不匹配");
}

#[then(expr = "保司含已删列表应包含 {string}")]
fn check_insurer_all_contains(world: &mut LedgerWorld, name: String) {
    let insurers = list_insurers_domain(&world_conn!(world), true).expect("查询含已删保司列表失败");
    assert!(
        insurers.iter().any(|i| i.name == name),
        "含已删保司列表应包含 '{name}'"
    );
}

/// find-or-create 复用断言：再次按名创建同名保司，返回 id 与上次一致（命中复用，
/// 不新建行——计数不变由调用场景的「在用保司总数应为 N」共同断言）。
#[then(expr = "按名创建保司 {string} 应复用已有行")]
fn check_find_or_create_reuses_existing(world: &mut LedgerWorld, name: String) {
    let previous = world
        .last_insurer_by_name_id
        .clone()
        .expect("复用断言前应先执行过按名创建保司");
    let id = create_insurer_by_name(&world_conn!(world), &name).expect("按名创建保司失败");
    assert_eq!(
        id, previous,
        "按名创建保司 '{name}' 应复用已有行（返回同 id），实际新建了行"
    );
    world.insurer_name_to_id.insert(name, id);
}
