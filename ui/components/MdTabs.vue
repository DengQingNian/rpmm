<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from "vue";

/** 一个标签页。badge 用于展示计数或状态。 */
export interface MdTabItem {
  key: string;
  label: string;
  badge?: string | number;
}

/**
 * Material 3 主标签：短指示器沿基线滑动，键盘左右切页。
 */
const props = defineProps<{
  modelValue: string;
  /** 标签组所属的 aria-label。 */
  label: string;
  tabs: MdTabItem[];
}>();

const emit = defineEmits<{ "update:modelValue": [value: string] }>();

const root = ref<HTMLElement | null>(null);
const indicator = ref({ left: 0, width: 0 });

/** 查询某个标签按钮。参数：key 为标签标识。返回：按钮元素。 */
function tabElement(key: string): HTMLElement | undefined {
  return root.value?.querySelector<HTMLElement>('[data-tab="' + key + '"]') ?? undefined;
}

/** 把指示器移动到当前标签下方。参数：无。返回：无。 */
async function placeIndicator(): Promise<void> {
  await nextTick();
  const element = tabElement(props.modelValue);
  if (!element) return;
  indicator.value = { left: element.offsetLeft, width: element.offsetWidth };
}

/** 键盘左右切换标签。参数：event 为键盘事件。返回：无。 */
function onKeydown(event: KeyboardEvent): void {
  const index = props.tabs.findIndex((tab) => tab.key === props.modelValue);
  if (index < 0) return;
  const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
  if (!step) return;
  event.preventDefault();
  const next = props.tabs[(index + step + props.tabs.length) % props.tabs.length];
  if (!next) return;
  emit("update:modelValue", next.key);
  void nextTick(() => tabElement(next.key)?.focus());
}

watch(() => [props.modelValue, props.tabs.length], placeIndicator, { deep: true });
onMounted(placeIndicator);
</script>

<template>
  <div class="md-tabs" role="tablist" :aria-label="label" ref="root" @keydown="onKeydown">
    <button
      v-for="tab in tabs"
      :key="tab.key"
      type="button"
      role="tab"
      class="md-tabs__tab"
      :class="{ 'is-selected': tab.key === modelValue }"
      :data-tab="tab.key"
      :aria-selected="tab.key === modelValue"
      :tabindex="tab.key === modelValue ? 0 : -1"
      @click="emit('update:modelValue', tab.key)"
    >
      <span class="md-tabs__label">{{ tab.label }}</span>
      <span v-if="tab.badge !== undefined" class="md-tabs__badge">{{ tab.badge }}</span>
    </button>
    <span
      class="md-tabs__indicator"
      :style="{ transform: 'translateX(' + indicator.left + 'px)', width: indicator.width + 'px' }"
    />
  </div>
</template>
