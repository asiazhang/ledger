<script setup lang="ts">
import { NSelect } from 'naive-ui'
import { pinyinFilter } from '@/utils/pinyin-filter'

// 拼音可搜下拉（issue #198 试点，ADR-0027 统一模糊搜索语义）：
// 薄封装 NSelect，收口 filterable + 拼音 filter，其余 props/attrs/slots
// 原样透传（透传依赖单根节点 attrs fallthrough：attrs 合并到根 NSelect，
// 若改多根模板需显式 v-bind="$attrs"；filterable/filter 收口值可被调用方
// 同名 attr 覆盖，属预期逃逸门）。范围决策编码在调用点的组件选择上：仅实体
// 类下拉（账户、分类、标的、关联交易）使用本组件；枚举类下拉与币种下拉
// 继续用裸 NSelect。
</script>

<template>
  <NSelect filterable :filter="pinyinFilter">
    <template v-for="(_, name) in $slots" :key="name" #[name]="slotProps">
      <slot :name="name" v-bind="slotProps ?? {}" />
    </template>
  </NSelect>
</template>
