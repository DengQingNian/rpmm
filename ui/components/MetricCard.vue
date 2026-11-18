<script setup lang="ts">
import type { IconName } from "../icons";
import AppIcon from "./AppIcon.vue";
import MdGauge from "./MdGauge.vue";

/**
 * 指标卡片：左侧为标题、主数值与补充说明，右侧可选环形仪表。
 * 主数值用 headline-small，避免长字节数换行。
 */
withDefaults(
  defineProps<{
    label: string;
    value: string;
    supporting?: string;
    icon: IconName;
    /** 有百分比时显示环形仪表。 */
    gauge?: number | null;
  }>(),
  { supporting: "", gauge: undefined },
);
</script>

<template>
  <article class="metric-card">
    <div class="metric-card__body">
      <span class="metric-card__label">
        <AppIcon :name="icon" :size="16" />
        {{ label }}
      </span>
      <strong class="metric-card__value">{{ value }}</strong>
      <span v-if="supporting" class="metric-card__support">{{ supporting }}</span>
    </div>
    <MdGauge
      v-if="gauge !== undefined"
      :value="gauge"
      :label="label"
      :size="56"
      :stroke="6"
    />
  </article>
</template>
