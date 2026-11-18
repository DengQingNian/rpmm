<script setup lang="ts">
import { computed, ref, useId } from "vue";
import type { IconName } from "../icons";
import AppIcon from "./AppIcon.vue";

/**
 * Material 3 轮廓文本框，支持前后缀图标、清除按钮、多行与错误状态。
 * 标签始终显示在边框缺口处，避免浮动标签带来的布局跳动。
 */
const props = withDefaults(
  defineProps<{
    modelValue: string | null;
    label: string;
    /** 隐藏在标签后的补充说明，例如单位或格式。 */
    supporting?: string;
    /** 错误说明，存在时使用错误色描边。 */
    error?: string;
    prefixIcon?: IconName;
    suffixIcon?: IconName;
    /** 为 true 时在有内容且未禁用时显示清除按钮。 */
    clearable?: boolean;
    disabled?: boolean;
    readonly?: boolean;
    placeholder?: string;
    rows?: number;
    type?: "text" | "password" | "search";
    inputmode?: "text" | "numeric" | "decimal" | "url";
    maxlength?: number;
    spellcheck?: boolean;
    /** 标签类中的额外修饰，例如是否等宽字体。 */
    mono?: boolean;
  }>(),
  {
    supporting: "",
    error: "",
    prefixIcon: undefined,
    suffixIcon: undefined,
    clearable: false,
    disabled: false,
    readonly: false,
    placeholder: "",
    rows: 0,
    type: "text",
    inputmode: undefined,
    maxlength: undefined,
    spellcheck: false,
    mono: false,
  },
);

const emit = defineEmits<{
  "update:modelValue": [value: string];
  enter: [];
}>();

const id = useId();
const focused = ref(false);
const filled = computed(() => Boolean(props.modelValue));
const multiline = computed(() => props.rows > 0);

/** 把输入值同步给父组件。参数：event 为输入事件。返回：无。 */
function onInput(event: Event): void {
  emit("update:modelValue", (event.target as HTMLInputElement | HTMLTextAreaElement).value);
}

/** 清空内容并保持焦点。参数：无。返回：无。 */
function clear(): void {
  emit("update:modelValue", "");
}

/** 回车提交，多行时交给浏览器的换行。参数：event 为键盘事件。返回：无。 */
function onKeydown(event: KeyboardEvent): void {
  if (!multiline.value && event.key === "Enter") emit("enter");
}
</script>

<template>
  <div
    class="md-field"
    :class="{
      'is-focused': focused,
      'is-filled': filled,
      'is-invalid': Boolean(error),
      'is-disabled': disabled,
      'is-mono': mono,
      'is-multiline': multiline,
    }"
  >
    <div class="md-field__box">
      <label class="md-field__label" :for="id">{{ label }}</label>
      <AppIcon
        v-if="prefixIcon"
        class="md-field__adornment"
        :name="prefixIcon"
        :size="20"
      />
      <textarea
        v-if="multiline"
        :id="id"
        class="md-field__input md-field__textarea"
        :value="modelValue ?? ''"
        :disabled="disabled"
        :readonly="readonly"
        :placeholder="placeholder"
        :rows="rows"
        :maxlength="maxlength"
        :spellcheck="spellcheck"
        @input="onInput"
        @focus="focused = true"
        @blur="focused = false"
      />
      <input
        v-else
        :id="id"
        class="md-field__input"
        :type="type"
        :value="modelValue ?? ''"
        :disabled="disabled"
        :readonly="readonly"
        :placeholder="placeholder"
        :inputmode="inputmode"
        :maxlength="maxlength"
        :spellcheck="spellcheck"
        @input="onInput"
        @keydown="onKeydown"
        @focus="focused = true"
        @blur="focused = false"
      />
      <button
        v-if="clearable && filled && !disabled && !readonly"
        type="button"
        class="md-field__clear"
        aria-label="清除输入"
        title="清除输入"
        @click="clear"
      >
        <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
          <path
            d="M6 6l12 12M18 6L6 18"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
          />
        </svg>
      </button>
      <AppIcon
        v-else-if="suffixIcon"
        class="md-field__adornment"
        :name="suffixIcon"
        :size="20"
      />
    </div>
    <p v-if="error" class="md-field__support is-error" :id="`${id}-support`">
      {{ error }}
    </p>
    <p v-else-if="supporting" class="md-field__support" :id="`${id}-support`">
      {{ supporting }}
    </p>
  </div>
</template>
