<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import MdButton from "./MdButton.vue";

/**
 * Material 3 基础对话框：extra-large 圆角、海拔 3。
 * 打开时把焦点移入面板，并支持 Esc 关闭与焦点循环。
 */
const props = withDefaults(
  defineProps<{
    open: boolean;
    title: string;
    /** 主要操作文案；空字符串表示不显示操作区。 */
    confirmLabel?: string;
    cancelLabel?: string;
    /** 危险操作使用错误色填充按钮。 */
    destructive?: boolean;
    closable?: boolean;
    busy?: boolean;
    /** 面板宽度。 */
    width?: string;
    /** 内容区域固定高度的对话框，例如表单。 */
    tall?: boolean;
  }>(),
  {
    confirmLabel: "",
    cancelLabel: "取消",
    destructive: false,
    closable: true,
    busy: false,
    width: "min(560px, calc(100vw - 48px))",
    tall: false,
  },
);

const emit = defineEmits<{ close: []; confirm: [] }>();

const panel = ref<HTMLElement | null>(null);

/** 请求关闭：正在提交时忽略。参数：无。返回：无。 */
function requestClose(): void {
  if (!props.closable || props.busy) return;
  emit("close");
}

/** 对话框内键盘处理：Esc 关闭、Tab 在面板内循环。参数：event 为键盘事件。返回：无。 */
function onKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    event.stopPropagation();
    requestClose();
    return;
  }
  if (event.key !== "Tab" || !panel.value) return;
  const items = Array.from(
    panel.value.querySelectorAll<HTMLElement>(
      'button:not(:disabled), input:not(:disabled), textarea:not(:disabled), [tabindex="0"]',
    ),
  );
  const first = items[0];
  const last = items.at(-1);
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last?.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first?.focus();
  }
}

/** 挂载后聚焦面板并获得 Esc。参数：无。返回：无。 */
function onMountedTrack(): void {
  void nextTick(() => panel.value?.focus());
}

onMounted(onMountedTrack);
onBeforeUnmount(() => undefined);
</script>

<template>
  <Teleport to="body">
    <Transition name="md-scrim">
      <div
        v-if="open"
        class="md-dialog__scrim"
        @pointerdown.self="requestClose"
      >
        <div
          ref="panel"
          class="md-dialog"
          :class="{ 'is-tall': tall }"
          role="dialog"
          aria-modal="true"
          :aria-label="title"
          tabindex="-1"
          :style="{ width }"
          @keydown="onKeydown"
        >
          <div class="md-dialog__scroll">
            <h2 class="md-dialog__title">{{ title }}</h2>
            <div class="md-dialog__content"><slot /></div>
          </div>
          <div v-if="confirmLabel || $slots.actions" class="md-dialog__actions">
            <slot name="actions">
              <MdButton variant="text" :disabled="busy" @click="requestClose">{{
                cancelLabel
              }}</MdButton>
              <MdButton
                :variant="destructive ? 'filled' : 'filled'"
                :class="{ 'is-destructive': destructive }"
                :loading="busy"
                @click="emit('confirm')"
                >{{ confirmLabel }}</MdButton
              >
            </slot>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
