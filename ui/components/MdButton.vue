<script setup lang="ts">
import type { IconName } from "../icons";
import AppIcon from "./AppIcon.vue";

/**
 * Material 3 按钮：filled、tonal、outlined、text、elevated 五种外观。
 * 宽度由内容决定，只有 icon-only 形态为 40×40 的正方形。
 */
withDefaults(
  defineProps<{
    /** 外观变体。 */
    variant?: "filled" | "tonal" | "outlined" | "text" | "elevated";
    /** 尺寸：default 为 40dp 高，small 为 32dp 高。 */
    size?: "default" | "small";
    /** 循环中的按钮使用空前缀而不是无图标。 */
    icon?: IconName | "";
    /** 仅显示图标，此时必须提供 ariaLabel。 */
    iconOnly?: boolean;
    disabled?: boolean;
    loading?: boolean;
    /** 原生按钮类型。 */
    type?: "button" | "submit" | "reset";
    /** 按下状态（切换按钮）。 */
    selected?: boolean;
    /** 内容换行而不是省略。 */
    wrap?: boolean;
    /** icon-only 按钮的无障碍名称。 */
    ariaLabel?: string;
    title?: string;
  }>(),
  {
    variant: "text",
    size: "default",
    icon: "",
    iconOnly: false,
    disabled: false,
    loading: false,
    type: "button",
    selected: false,
    wrap: false,
    ariaLabel: undefined,
    title: undefined,
  },
);

const emit = defineEmits<{ click: [event: MouseEvent] }>();

/** 转发点击；禁用或加载中时浏览器原生拦截。参数：event 为鼠标事件。返回：无。 */
function onClick(event: MouseEvent): void {
  emit("click", event);
}
</script>

<template>
  <button
    :type="type"
    class="md-button"
    :class="[
      `md-button--${variant}`,
      `md-button--${size}`,
      {
        'md-button--icon': iconOnly,
        'is-selected': selected,
        'is-loading': loading,
      },
    ]"
    :disabled="disabled || loading"
    :aria-label="ariaLabel"
    :aria-pressed="selected ? 'true' : undefined"
    :title="title"
    @click="onClick"
  >
    <span v-if="loading" class="md-button__spinner" aria-hidden="true" />
    <AppIcon
      v-else-if="icon"
      class="md-button__icon"
      :name="icon"
      :size="size === 'small' ? 18 : 18"
    />
    <span v-if="!iconOnly" class="md-button__label" :class="{ 'is-wrap': wrap }"
      ><slot /></span
    >
  </button>
</template>
