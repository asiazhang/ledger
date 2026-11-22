<script setup lang="ts">
import { errorMessage } from "@ledger/utils/errors";
import { computed, h } from "vue";
import { NButton, NIcon, NSpace, NTree, useMessage } from "naive-ui";
import type { TreeOption, TreeDropInfo } from "naive-ui";
import AppPopconfirm from "@ledger/ui-kit/AppPopconfirm.vue";
import { api } from "@ledger/api";
import { useReferenceStore } from "@/stores/reference";
import { useWindowTier } from "@ledger/window-tier";
import { getIconComponent } from "@ledger/utils/icon";
import { buildCategoryTree } from "@ledger/utils/category-tree";
import { t } from "@ledger/i18n";
import type { Category, CategoryKind } from "@ledger/types";

const props = defineProps<{ kind: CategoryKind }>();
const emit = defineEmits<{ edit: [cat: Category] }>();

const reference = useReferenceStore();
const message = useMessage();

const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

interface TreeCategoryNode extends TreeOption {
  category: Category;
}

const treeData = computed<TreeCategoryNode[]>(
  () =>
    buildCategoryTree(reference.categories, {
      kind: props.kind,
      sort: true,
    }) as unknown as TreeCategoryNode[],
);

function renderIcon(name: string | null) {
  const Comp = getIconComponent(name);
  if (!Comp) return null;
  return h(
    NIcon,
    { size: 18, style: { marginRight: "6px", verticalAlign: "middle" } },
    { default: () => h(Comp) },
  );
}

function renderPrefix(info: { option: TreeOption }) {
  const cat = (info.option as TreeCategoryNode).category;
  return renderIcon(cat.icon);
}

function renderSuffix(info: { option: TreeOption }) {
  const cat = (info.option as TreeCategoryNode).category;
  return h(NSpace, { size: "small", align: "center" }, () => [
    h(
      NButton,
      {
        size: "tiny",
        quaternary: true,
        onClick: (e: MouseEvent) => {
          e.stopPropagation();
          emit("edit", cat);
        },
      },
      () => t("settings.categories.tree.edit"),
    ),
    h(
      AppPopconfirm,
      {
        onPositiveClick: () => removeCategory(cat.id),
      },
      {
        default: () => t("settings.categories.tree.deleteConfirm"),
        trigger: () =>
          h(
            NButton,
            {
              size: "tiny",
              type: "error",
              quaternary: true,
              onClick: (e: MouseEvent) => e.stopPropagation(),
            },
            () => t("settings.categories.tree.delete"),
          ),
      },
    ),
  ]);
}

async function handleDrop(info: TreeDropInfo) {
  const { node: targetNode, dragNode, dropPosition } = info;
  if (dropPosition === "inside") return;

  const dragCat = (dragNode as TreeCategoryNode).category;
  const targetCat = (targetNode as TreeCategoryNode).category;
  if (dragCat.parent_id !== targetCat.parent_id) return;
  if (dragCat.kind !== targetCat.kind) return;

  const siblings = reference.categories
    .filter((c) => c.parent_id === dragCat.parent_id && c.kind === dragCat.kind)
    .sort((a, b) => a.sort_order - b.sort_order);

  const fromIdx = siblings.findIndex((c) => c.id === dragCat.id);
  let toIdx = siblings.findIndex((c) => c.id === targetCat.id);
  if (fromIdx === -1 || toIdx === -1) return;
  if (dropPosition === "after") toIdx++;

  const [moved] = siblings.splice(fromIdx, 1);
  if (fromIdx < toIdx) toIdx--;
  siblings.splice(toIdx, 0, moved);

  try {
    await api.reorderCategories(siblings.map((c, i) => ({ id: c.id, sort_order: i })));
  } catch (e) {
    message.error(t("settings.categories.msg.sortFailed", { msg: errorMessage(e) }));
  }
}

async function removeCategory(id: string) {
  try {
    await api.deleteCategory(id);
    message.success(t("settings.categories.msg.deleted"));
  } catch (e) {
    message.error(t("settings.categories.msg.deleteFailed", { msg: errorMessage(e) }));
  }
}
</script>

<template>
  <NTree
    :data="treeData"
    :render-prefix="renderPrefix"
    :render-suffix="renderSuffix"
    :draggable="!isMobileTier"
    block-line
    :default-expand-all="true"
    @drop="handleDrop"
  />
</template>
