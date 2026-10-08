<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import { init, use, type EChartsType } from "echarts/core";
import { LineChart } from "echarts/charts";
import {
  GridComponent,
  TooltipComponent,
  LegendComponent,
} from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import type { MetricPoint } from "../types";
use([
  LineChart,
  GridComponent,
  TooltipComponent,
  LegendComponent,
  CanvasRenderer,
]);
const props = defineProps<{
  points: MetricPoint[];
  fields: {
    key: "cpu" | "memory" | "read" | "write";
    name: string;
    color: string;
  }[];
  unit: string;
  percent?: boolean;
}>();
const root = ref<HTMLElement | null>(null);
let chart: EChartsType | undefined;
let observer: ResizeObserver | undefined;
/** 更新趋势图。参数：无，读取图表属性。返回：无。 */
function render(): void {
  chart?.setOption(
    {
      animation: false,
      color: props.fields.map((field) => field.color),
      tooltip: { trigger: "axis", confine: true },
      legend: { top: 0, textStyle: { color: "#79746b" } },
      grid: { left: 55, right: 16, top: 35, bottom: 30 },
      xAxis: {
        type: "category",
        boundaryGap: false,
        data: props.points.map((point) =>
          new Date(point.time).toLocaleTimeString("zh-CN", { hour12: false }),
        ),
        axisLabel: { color: "#79746b", hideOverlap: true },
      },
      yAxis: {
        type: "value",
        min: 0,
        ...(props.percent ? { max: 100 } : {}),
        axisLabel: { formatter: `{value}${props.unit}`, color: "#79746b" },
        splitLine: { lineStyle: { type: "dashed", color: "#d0c7b8" } },
      },
      series: props.fields.map((field) => ({
        name: field.name,
        type: "line",
        showSymbol: false,
        connectNulls: false,
        data: props.points.map((point) => {
          const value = point[field.key];
          return value === null
            ? null
            : Number(
                (value / (props.unit === " MiB/s" ? 1048576 : 1)).toFixed(2),
              );
        }),
        lineStyle: { width: 2 },
        areaStyle: props.fields.length === 1 ? { opacity: 0.08 } : undefined,
      })),
    },
    true,
  );
}
/** 初始化图表并监听布局变化。参数：无。返回：无。 */
function mount(): void {
  if (!root.value) return;
  chart = init(root.value);
  observer = new ResizeObserver(() => chart?.resize());
  observer.observe(root.value);
  render();
}
/** 释放画布和尺寸监听。参数：无。返回：无。 */
function dispose(): void {
  observer?.disconnect();
  chart?.dispose();
}
watch(() => props.points, render, { deep: true });
onMounted(mount);
onUnmounted(dispose);
</script>
<template>
  <div
    ref="root"
    class="metric-chart"
    role="img"
    :aria-label="fields.map((field) => field.name).join('、') + '趋势图'"
  ></div>
</template>
