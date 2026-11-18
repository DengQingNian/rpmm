<script setup lang="ts">
/** Material 3 复选框：18dp 方框 + 48dp 触控区，选中时填充主色。 */
const props = withDefaults(
  defineProps<{
    modelValue: boolean;
    label: string;
    disabled?: boolean;
  }>(),
  { disabled: false },
);
const emit = defineEmits<{ "update:modelValue": [value: boolean] }>();

/** 切换勾选状态。参数：无。返回：无。 */
function toggle(): void {
  if (!props.disabled) emit("update:modelValue", !props.modelValue);
}
</script>

<template>
  <button
    type="button"
    role="checkbox"
    class="md-checkbox"
    :class="{ 'is-checked': modelValue, 'is-disabled': disabled }"
    :aria-checked="modelValue"
    :disabled="disabled"
    @click="toggle"
  >
    <span class="md-checkbox__box">
      <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
        <path
          v-if="modelValue"
          d="M5 12.5l4.2 4.2L19 7"
          fill="none"
          stroke="currentColor"
          stroke-width="2.6"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </span>
    <span class="md-checkbox__label">{{ label }}</span>
  </button>
</template>
