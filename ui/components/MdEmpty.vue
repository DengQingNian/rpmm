<script setup lang="ts">
import type { IconName } from "../icons";
import AppIcon from "./AppIcon.vue";

/** 空状态：图标 + 标题 + 说明，可附带一个主要操作。 */
withDefaults(
  defineProps<{
    icon?: IconName;
    title: string;
    description?: string;
    /** 紧凑变体用于小面板内部。 */
    compact?: boolean;
  }>(),
  { icon: "empty", description: "", compact: false },
);
</script>

<template>
  <div class="md-empty" :class="{ 'is-compact': compact }">
    <span class="md-empty__icon"><AppIcon :name="icon" :size="28" /></span>
    <p class="md-empty__title">{{ title }}</p>
    <p v-if="description" class="md-empty__text">{{ description }}</p>
    <slot />
  </div>
</template>
