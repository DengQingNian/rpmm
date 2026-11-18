<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import type { IconName } from "../icons";
import AppIcon from "./AppIcon.vue";

/** 单选项。value 为字符串，便于直接写入配置表单。 */
export interface MdOption {
  label: string;
  value: string;
  /** 选项后的补充说明，例如计数。 */
  hint?: string;
}

/**
 * Material 3 暴露式下拉菜单：带轮廓的触发器 + 锚定在触发器下方的菜单。
 * 支持键盘上下移动、回车选择、Esc 与点击外部关闭。
 */
const props = withDefaults(
  defineProps<{
    modelValue: string | null;
    options: MdOption[];
    label: string;
    placeholder?: string;
    prefixIcon?: IconName;
    disabled?: boolean;
    /** 触发器最小宽度。 */
    width?: string;
  }>(),
  {
    placeholder: "请选择",
    prefixIcon: undefined,
    disabled: false,
    width: "",
  },
);

const emit = defineEmits<{ "update:modelValue": [value: string] }>();

const open = ref(false);
const active = ref(0);
const trigger = ref<HTMLElement | null>(null);
const menu = ref<HTMLElement | null>(null);
const current = computed(() =>
  props.options.find((option) => option.value === props.modelValue),
);

/** 打开菜单并把高亮移到最后一次选择的位置。参数：无。返回：无。 */
function show(): void {
  if (props.disabled || !props.options.length) return;
  open.value = true;
  active.value = Math.max(
    0,
    props.options.findIndex((option) => option.value === props.modelValue),
  );
  void nextTick(() => menu.value?.focus());
}

/** 关闭菜单。参数：无。返回：无。 */
function hide(): void {
  open.value = false;
}

/** 切换菜单。参数：无。返回：无。 */
function toggle(): void {
  if (open.value) hide();
  else show();
}

/** 选择一项并关闭。参数：value 为选项值。返回：无。 */
function choose(value: string): void {
  emit("update:modelValue", value);
  hide();
  trigger.value?.focus();
}

/** 菜单键盘操作。参数：event 为键盘事件。返回：无。 */
function onKeydown(event: KeyboardEvent): void {
  const count = props.options.length;
  if (event.key === "ArrowDown") {
    event.preventDefault();
    active.value = (active.value + 1) % count;
    return;
  }
  if (event.key === "ArrowUp") {
    event.preventDefault();
    active.value = (active.value - 1 + count) % count;
    return;
  }
  if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    const option = props.options[active.value];
    if (option) choose(option.value);
    return;
  }
  if (event.key === "Escape") {
    event.preventDefault();
    hide();
    trigger.value?.focus();
  }
}

/** 点击菜单外部时关闭。参数：event 为指针事件。返回：无。 */
function onOutside(event: PointerEvent): void {
  const target = event.target as Node;
  if (menu.value?.contains(target) || trigger.value?.contains(target)) return;
  hide();
}

watch(open, (value) => {
  if (value) document.addEventListener("pointerdown", onOutside, true);
  else document.removeEventListener("pointerdown", onOutside, true);
});

onBeforeUnmount(() => document.removeEventListener("pointerdown", onOutside, true));
</script>

<template>
  <div class="md-select" :style="width ? { minWidth: width } : undefined">
    <button
      ref="trigger"
      type="button"
      class="md-select__trigger"
      :class="{ 'is-open': open, 'is-placeholder': !current }"
      :disabled="disabled"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :aria-label="label"
      @click="toggle"
      @keydown.down.prevent="show"
    >
      <AppIcon
        v-if="prefixIcon"
        class="md-select__icon"
        :name="prefixIcon"
        :size="20"
      />
      <span class="md-select__value">{{ current?.label ?? placeholder }}</span>
      <svg
        class="md-select__arrow"
        :class="{ 'is-open': open }"
        viewBox="0 0 24 24"
        width="20"
        height="20"
        aria-hidden="true"
      >
        <path
          d="M7 10l5 5 5-5"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </button>
    <div
      v-if="open"
      ref="menu"
      class="md-menu"
      role="listbox"
      :aria-label="label"
      tabindex="-1"
      @keydown="onKeydown"
    >
      <button
        v-for="(option, index) in options"
        :key="option.value"
        type="button"
        role="option"
        class="md-menu__item"
        :class="{ 'is-active': index === active }"
        :aria-selected="option.value === modelValue"
        @pointerenter="active = index"
        @click="choose(option.value)"
      >
        <span class="md-menu__label">{{ option.label }}</span>
        <span v-if="option.hint" class="md-menu__hint">{{ option.hint }}</span>
        <AppIcon
          v-if="option.value === modelValue"
          class="md-menu__check"
          name="check"
          :size="18"
        />
      </button>
    </div>
  </div>
</template>
