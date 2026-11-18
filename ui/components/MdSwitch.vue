<script setup lang="ts">
/** Material 3 开关：52×32 轨道与 24dp 滑块，选中时滑块放大到 24dp 并带勾选图标。 */
const props = withDefaults(
  defineProps<{
    modelValue: boolean;
    /** 无障碍名称。 */
    label: string;
    disabled?: boolean;
    /** 显示在开关左侧的选中/未选中图标。 */
    icons?: boolean;
  }>(),
  { disabled: false, icons: true },
);
const emit = defineEmits<{ "update:modelValue": [value: boolean] }>();

/** 切换开关状态。参数：无。返回：无。 */
function toggle(): void {
  if (!props.disabled) emit("update:modelValue", !props.modelValue);
}
</script>

<template>
  <button
    type="button"
    role="switch"
    class="md-switch"
    :class="{ 'is-checked': modelValue, 'is-disabled': disabled }"
    :aria-checked="modelValue"
    :aria-label="label"
    :disabled="disabled"
    @click="toggle"
  >
    <span class="md-switch__track">
      <svg
        v-if="icons"
        class="md-switch__state"
        viewBox="0 0 24 24"
        width="16"
        height="16"
        aria-hidden="true"
      >
        <path
          v-if="modelValue"
          d="M5 12.5l4.2 4.2L19 7"
          fill="none"
          stroke="currentColor"
          stroke-width="2.4"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
        <path
          v-else
          d="M7 12h10"
          fill="none"
          stroke="currentColor"
          stroke-width="2.4"
          stroke-linecap="round"
        />
      </svg>
      <span class="md-switch__handle" />
    </span>
  </button>
</template>
