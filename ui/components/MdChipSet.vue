<script setup lang="ts">
import type { IconName } from "../icons";
import AppIcon from "./AppIcon.vue";

/**
 * Material 3 过滤标签：用于在多个可选服务中多选。
 * 选中时使用 secondary-container 填充并显示勾选图标。
 */
withDefaults(
  defineProps<{
    modelValue: string[];
    options: { value: string; label: string }[];
    /** 标签组名称，仅用于无障碍。 */
    label: string;
    disabled?: boolean;
    /** 空态提示。 */
    emptyText?: string;
  }>(),
  { disabled: false, emptyText: "没有可选项" },
);

const emit = defineEmits<{ "update:modelValue": [value: string[]] }>();

/** 切换某个选项。参数：value 为选项值。返回：无。 */
function toggle(value: string, modelValue: string[]): void {
  const next = modelValue.includes(value)
    ? modelValue.filter((item) => item !== value)
    : [...modelValue, value];
  emit("update:modelValue", next);
}
</script>

<template>
  <div class="md-chipset" role="group" :aria-label="label">
    <p v-if="!options.length" class="md-chipset__empty">{{ emptyText }}</p>
    <button
      v-for="option in options"
      :key="option.value"
      type="button"
      role="checkbox"
      class="md-chip"
      :class="{ 'is-selected': modelValue.includes(option.value) }"
      :aria-checked="modelValue.includes(option.value)"
      :disabled="disabled"
      @click="toggle(option.value, modelValue)"
    >
      <AppIcon
        class="md-chip__icon"
        :name="modelValue.includes(option.value) ? 'check' : 'tag'"
        :size="16"
      />
      <span class="md-chip__label">{{ option.label }}</span>
    </button>
  </div>
</template>
