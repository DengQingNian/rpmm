<script setup lang="ts">
/**
 * Material 3 卡片：elevated（海拔 1）、filled（surface-container-highest）、
 * outlined（outline-variant 描边）。卡片本身不承载交互，交互元素自行处理状态层。
 */
withDefaults(
  defineProps<{
    variant?: "elevated" | "filled" | "outlined";
    padding?: "none" | "compact" | "default" | "large";
    /** 作为可点击容器时提供，并自动获得 hover/focus 状态层。 */
    interactive?: boolean;
    /** 自定义形状，默认 12dp 圆角。 */
    shape?: "medium" | "large" | "extra-large";
    ariaLabel?: string;
  }>(),
  {
    variant: "filled",
    padding: "default",
    interactive: false,
    shape: "medium",
    ariaLabel: undefined,
  },
);
</script>

<template>
  <div
    class="md-card"
    :class="[
      `md-card--${variant}`,
      `md-card--pad-${padding}`,
      `md-card--shape-${shape}`,
      { 'is-interactive': interactive },
    ]"
    :aria-label="ariaLabel"
  >
    <slot />
  </div>
</template>
