<script setup lang="ts">
import type { IconName } from "../icons";
import AppIcon from "./AppIcon.vue";

/** 图标按钮：standard（透明底）、filled、tonal、outlined 四种外观，大小为 40×40。 */
withDefaults(
  defineProps<{
    name: IconName;
    variant?: "standard" | "filled" | "tonal" | "outlined";
    /** 无障碍名称，必须提供。 */
    label: string;
    disabled?: boolean;
    selected?: boolean;
    size?: number;
  }>(),
  { variant: "standard", disabled: false, selected: false, size: 20 },
);
const emit = defineEmits<{ click: [event: MouseEvent] }>();
</script>

<template>
  <button
    type="button"
    class="md-icon-button"
    :class="[`md-icon-button--${variant}`, { 'is-selected': selected }]"
    :disabled="disabled"
    :aria-label="label"
    :title="label"
    :aria-pressed="selected ? 'true' : undefined"
    @click="emit('click', $event)"
  >
    <AppIcon :name="name" :size="size" />
  </button>
</template>
