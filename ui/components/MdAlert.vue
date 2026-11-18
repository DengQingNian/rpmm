<script setup lang="ts">
import type { IconName } from "../icons";
import AppIcon from "./AppIcon.vue";

/** 页面内提示条：info、success、warning、error 四种语义。 */
withDefaults(
  defineProps<{
    kind?: "info" | "success" | "warning" | "error";
    /** 标题为空时只显示一行正文。 */
    title?: string;
  }>(),
  { kind: "info", title: "" },
);

const ICONS: Record<string, IconName> = {
  info: "info",
  success: "success",
  warning: "warning",
  error: "attention",
};
</script>

<template>
  <div class="md-alert" :class="'md-alert--' + kind" role="status">
    <AppIcon class="md-alert__icon" :name="ICONS[kind] ?? 'info'" :size="20" />
    <div class="md-alert__body">
      <strong v-if="title" class="md-alert__title">{{ title }}</strong>
      <div class="md-alert__text"><slot /></div>
    </div>
    <slot name="action" />
  </div>
</template>
