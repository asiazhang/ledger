//! 分类参考数据步骤（issue #377）：交易列表「分类下钻」场景的 Given/When/Then。
//! 软删分类的名称→ID 映射刻意保留：软删后分类行仍在库中（历史交易口径，先例商户），
//! 后续步骤仍可按名称引用其 id 验证历史交易可过滤。

use cucumber::{given, then, when};

use tauri_app_lib::categories::{
    create_category, delete_category as delete_category_domain,
    list_categories as list_categories_domain,
};
use tauri_app_lib::models::CategoryInput;

use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

#[given(expr = "存在分类 {string} 类型 {string}")]
fn given_category(world: &mut LedgerWorld, name: String, kind: String) {
    let id = create_category(
        &world_conn!(world),
        CategoryInput {
            name: name.clone(),
            kind,
            parent_id: None,
            icon: None,
        },
    )
    .expect("创建分类失败");
    world.category_name_to_id.insert(name, id);
}

/// 二级分类：挂在既有父分类下（交易表单不限定叶子，直挂/二级都可能被下钻引用）。
#[given(expr = "存在二级分类 {string} 父分类 {string} 类型 {string}")]
fn given_subcategory(world: &mut LedgerWorld, name: String, parent: String, kind: String) {
    let parent_id = world.category_id(&parent);
    let id = create_category(
        &world_conn!(world),
        CategoryInput {
            name: name.clone(),
            kind,
            parent_id: Some(parent_id),
            icon: None,
        },
    )
    .expect("创建二级分类失败");
    world.category_name_to_id.insert(name, id);
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

/// 软删分类（不可再被新交易选择；历史交易引用保留，仍可按其过滤——历史交易口径）。
#[when(expr = "软删分类 {string}")]
fn delete_category(world: &mut LedgerWorld, name: String) {
    let id = world.category_id(&name);
    delete_category_domain(&world_conn!(world), &id).expect("软删分类失败");
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

/// 在用列表不含软删分类（软删后不可再被选择）。
#[then(expr = "分类列表不应包含 {string}")]
fn category_list_not_contains(world: &mut LedgerWorld, name: String) {
    let categories = list_categories_domain(&world_conn!(world), false).expect("查询分类列表失败");
    assert!(
        !categories.iter().any(|c| c.name == name),
        "分类列表不应包含 '{name}'"
    );
}

/// 含软删全量列表（前端 URL 下钻校验映射的数据源）：软删分类仍在其列，
/// 历史交易引用照常可解析（issue #377，先例商户 issue #191）。
#[then(expr = "分类含软删列表应包含 {string}")]
fn category_list_with_deleted_contains(world: &mut LedgerWorld, name: String) {
    let categories =
        list_categories_domain(&world_conn!(world), true).expect("查询含软删分类列表失败");
    assert!(
        categories.iter().any(|c| c.name == name),
        "含软删分类列表应包含 '{name}'"
    );
}
