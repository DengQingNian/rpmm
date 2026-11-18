<script setup lang="ts">
import { computed } from "vue";

/**
 * 环形进度指示：用 SVG 圆环表示 0～100 的百分比，
 * 数值缺失（首次采样）时显示为不确定状态。
 */
const props = withDefaults(
  defineProps<{
    /** 0～100 的百分比；null 表示采样中。 */
    value: number | null;
    /** 无障碍名称。 */
    label: string;
    /** 圆环大小（px）。 */
    size?: number;
    /** 圆环粗细（px）。 */
    stroke?: number;
    /** 超过该百分比使用错误色，例如磁盘剩余空间不足。 */
    dangerAt?: number;
  }>(),
  { size: 64, stroke: 6, dangerAt: 90 },
);

const radius = computed(() => (props.size - props.stroke) / 2);
const circumference = computed(() => 2 * Math.PI * radius.value);
const clamped = computed(() => Math.min(100, Math.max(0, props.value ?? 0)));
const offset = computed(
  () => circumference.value * (1 - clamped.value / 100),
);
const tone = computed(() =>
  props.value != null && props.value >= props.dangerAt ? "danger" : "normal",
);
</script>

<template>
  <div
    class="md-gauge"
    :class="'md-gauge--' + tone"
    :style="{ width: size + 'px', height: size + 'px' }"
    role="img"
    :aria-label="label + '：' + (value == null ? '采样中' : value.toFixed(0) + '%')"
  >
    <svg :width="size" :height="size" viewBox="0 0 64 64" aria-hidden="true">
      <circle
        class="md-gauge__track"
        cx="32"
        cy="32"
        :r="radius"
        :stroke-width="stroke"
        fill="none"
      />
      <circle
        v-if="value != null"
        class="md-gauge__value"
        cx="32"
        cy="32"
        :r="radius"
        :stroke-width="stroke"
        fill="none"
        stroke-linecap="round"
        :stroke-dasharray="circumference"
        :stroke-dashoffset="offset"
        transform="rotate(-90 32 32)"
      />
    </svg>
    <span class="md-gauge__label">{{ value == null ? "—" : value.toFixed(0) + "%" }}</span>
  </div>
</template>
