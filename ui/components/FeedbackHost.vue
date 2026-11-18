<script setup lang="ts">
import { currentConfirm, feedback, settleConfirm, type ToastKind } from "../feedback";
import type { IconName } from "../icons";
import AppIcon from "./AppIcon.vue";
import MdButton from "./MdButton.vue";

/** 提示条图标，与状态语义一一对应。 */
const TOAST_ICONS: Record<ToastKind, IconName> = {
  info: "info",
  success: "success",
  warning: "warning",
  error: "attention",
};
const request = currentConfirm;
</script>

<template>
  <Teleport to="body">
    <!-- 底部提示：Material 3 snackbar 形态，inverse-surface 底色。 -->
    <div class="md-toasts" aria-live="polite" aria-atomic="false">
      <TransitionGroup name="md-toast">
        <div
          v-for="item in feedback.toasts"
          :key="item.id"
          class="md-toast"
          :class="'md-toast--' + item.kind"
          role="status"
        >
          <AppIcon class="md-toast__icon" :name="TOAST_ICONS[item.kind]" :size="20" />
          <p class="md-toast__text">{{ item.text }}</p>
        </div>
      </TransitionGroup>
    </div>
    <!-- 确认对话：使用 dialog 元素语义，遮罩点击取消。 -->
    <Transition name="md-scrim">
      <div
        v-if="request()"
        class="md-dialog__scrim"
        @pointerdown.self="settleConfirm(false)"
      >
        <div
          class="md-dialog md-dialog--confirm"
          role="alertdialog"
          aria-modal="true"
          :aria-label="request()?.title"
        >
          <div class="md-dialog__scroll">
            <h2 class="md-dialog__title">{{ request()?.title }}</h2>
            <p class="md-dialog__text">{{ request()?.message }}</p>
          </div>
          <div class="md-dialog__actions">
            <MdButton variant="text" @click="settleConfirm(false)">{{
              request()?.cancelLabel
            }}</MdButton>
            <MdButton
              variant="filled"
              :class="{ 'is-destructive': request()?.destructive }"
              @click="settleConfirm(true)"
              >{{ request()?.confirmLabel }}</MdButton
            >
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
