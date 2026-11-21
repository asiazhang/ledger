use cucumber::{given, then, when};

use ledger_categories::{
    CategoryInput, create_category, delete_category as delete_category_domain,
    list_categories as list_categories_domain,
};

use crate::world::LedgerWorld;

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

#[when(expr = "软删分类 {string}")]
fn delete_category(world: &mut LedgerWorld, name: String) {
    let id = world.category_id(&name);
    delete_category_domain(&world_conn!(world), &id).expect("软删分类失败");
}

#[then(expr = "分类列表不应包含 {string}")]
fn category_list_not_contains(world: &mut LedgerWorld, name: String) {
    let categories = list_categories_domain(&world_conn!(world), false).expect("查询分类列表失败");
    assert!(
        !categories.iter().any(|c| c.name == name),
        "分类列表不应包含 '{name}'"
    );
}

#[then(expr = "分类含软删列表应包含 {string}")]
fn category_list_with_deleted_contains(world: &mut LedgerWorld, name: String) {
    let categories =
        list_categories_domain(&world_conn!(world), true).expect("查询含软删分类列表失败");
    assert!(
        categories.iter().any(|c| c.name == name),
        "含软删分类列表应包含 '{name}'"
    );
}
