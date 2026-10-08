<script setup lang="ts">
import { icons, type IconName } from "../icons";

/** 统一图标属性；颜色继承文字颜色，默认图标为装饰，名称由相邻文字表达。 */
withDefaults(
  defineProps<{ name: IconName; size?: number; strokeWidth?: number }>(),
  {
    size: 18,
    strokeWidth: 1.8,
  },
);
</script>

<template>
  <component
    :is="icons[name]"
    :size="size"
    :stroke-width="strokeWidth"
    class="app-icon"
    aria-hidden="true"
    focusable="false"
  />
</template>
