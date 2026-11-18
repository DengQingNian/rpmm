<script setup lang="ts">
import { computed } from "vue";

/** 线性进度条：用于磁盘等需要精确读数的比例展示。 */
const props = withDefaults(
  defineProps<{
    value: number | null;
    label: string;
    dangerAt?: number;
  }>(),
  { dangerAt: 90 },
);
const clamped = computed(() => Math.min(100, Math.max(0, props.value ?? 0)));
const tone = computed(() =>
  props.value != null && props.value >= props.dangerAt ? "danger" : "normal",
);
</script>

<template>
  <div
    class="md-progress"
    :class="'md-progress--' + tone"
    role="progressbar"
    :aria-label="label"
    :aria-valuemin="0"
    :aria-valuemax="100"
    :aria-valuenow="value == null ? undefined : Math.round(clamped)"
  >
    <span class="md-progress__track" />
    <span class="md-progress__value" :style="{ width: clamped + '%' }" />
  </div>
</template>
