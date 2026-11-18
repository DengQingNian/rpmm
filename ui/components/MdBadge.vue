<script setup lang="ts">
import type { IconName } from "../icons";
import AppIcon from "./AppIcon.vue";
import type { StatusTone } from "../types";

/**
 * 状态徽章：圆点表达状态语义，容器用对应的 *-container 色调。
 * 只使用语义色，不直接写颜色值，保证深浅主题下对比度一致。
 */
withDefaults(
  defineProps<{
    tone?: StatusTone;
    /** 覆盖默认的状态点图标。 */
    icon?: IconName | "";
    /** 是否显示圆点。 */
    dot?: boolean;
  }>(),
  { tone: "neutral", icon: "", dot: true },
);
</script>

<template>
  <span class="md-badge" :class="'md-badge--' + tone">
    <AppIcon v-if="icon" class="md-badge__icon" :name="icon" :size="14" />
    <span v-else-if="dot" class="md-badge__dot" aria-hidden="true" />
    <slot />
  </span>
</template>
