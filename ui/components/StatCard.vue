<script setup lang="ts">
import { computed } from "vue";
import type { IconName } from "../icons";
import type { Page } from "../types";
import AppIcon from "./AppIcon.vue";

/**
 * 概览统计卡片：图标放在 third 色容器块内，数值用 display-small。
 * 点击后跳转到对应页面，作为 Dashboard 的主要入口。
 */
const props = defineProps<{
  label: string;
  value: number;
  icon: IconName;
  /** 次要说明。 */
  caption: string;
  /** 目标页面；为空时卡片不可点击。 */
  target?: Page;
}>();

const emit = defineEmits<{ open: [page: Page] }>();

/** 卡片语义：有目标页面时为可点击按钮。参数：无。返回：渲染标签。 */
const tag = computed(() => (props.target ? "button" : "article"));
</script>

<template>
  <component
    :is="tag"
    class="stat-card"
    :class="{ 'is-interactive': Boolean(target) }"
    :type="target ? 'button' : undefined"
    @click="target && emit('open', target)"
  >
    <span class="stat-card__icon"><AppIcon :name="icon" :size="22" /></span>
    <span class="stat-card__value">{{ value }}</span>
    <span class="stat-card__label">{{ label }}</span>
    <span class="stat-card__caption">{{ caption }}</span>
  </component>
</template>
