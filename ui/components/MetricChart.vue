<script setup lang="ts">
import { LineChart } from "echarts/charts";
import {
  GridComponent,
  LegendComponent,
  TooltipComponent,
} from "echarts/components";
import { init, use, type EChartsType } from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { onMounted, onUnmounted, ref, watch } from "vue";
import { useTheme } from "../theme";
import type { MetricPoint } from "../types";

use([LineChart, GridComponent, TooltipComponent, LegendComponent, CanvasRenderer]);

/**
 * 趋势折线图。颜色通过 CSS 变量传入，主题切换时重新读取实际颜色值，
 * 保证图表与界面使用同一套 Material 色彩角色。
 */
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
const theme = useTheme();
let chart: EChartsType | undefined;
let observer: ResizeObserver | undefined;

/** 读取 CSS 变量的实际颜色。参数：token 为变量名或十六进制颜色。返回：可直接绘制的颜色值。 */
function resolveColor(token: string): string {
  const match = token.match(/^var\((--[\w-]+)\)$/);
  if (!match || !root.value) return token;
  return (
    getComputedStyle(root.value).getPropertyValue(match[1]!).trim() || "#999999"
  );
}

/** 更新图表配置。参数：无。返回：无；每次采样都整体替换数据，避免动画抖动。 */
function render(): void {
  if (!chart) return;
  const style = root.value ? getComputedStyle(root.value) : null;
  const text = style?.getPropertyValue("--md-sys-color-on-surface-variant").trim() || "#666";
  const grid = style?.getPropertyValue("--md-sys-color-outline-variant").trim() || "#ccc";
  const surface = style?.getPropertyValue("--md-sys-color-surface-container-lowest").trim() || "#fff";
  const border = style?.getPropertyValue("--md-sys-color-outline-variant").trim() || "#ccc";
  const onSurface = style?.getPropertyValue("--md-sys-color-on-surface").trim() || "#222";
  const colors = props.fields.map((field) => resolveColor(field.color));
  chart.setOption(
    {
      animation: false,
      color: colors,
      tooltip: {
        trigger: "axis",
        confine: true,
        backgroundColor: surface,
        borderColor: border,
        borderWidth: 1,
        padding: [8, 12],
        textStyle: { color: onSurface, fontSize: 12 },
      },
      legend: {
        top: 0,
        right: 0,
        icon: "roundRect",
        itemWidth: 10,
        itemHeight: 10,
        itemGap: 16,
        textStyle: { color: text, fontSize: 12 },
      },
      grid: { left: 8, right: 8, top: 40, bottom: 4, containLabel: true },
      xAxis: {
        type: "category",
        boundaryGap: false,
        data: props.points.map((point) =>
          new Date(point.time).toLocaleTimeString("zh-CN", { hour12: false }),
        ),
        axisLine: { show: false },
        axisTick: { show: false },
        axisLabel: { color: text, hideOverlap: true },
      },
      yAxis: {
        type: "value",
        min: 0,
        ...(props.percent ? { max: 100 } : {}),
        axisLine: { show: false },
        axisTick: { show: false },
        axisLabel: { formatter: "{value}" + props.unit, color: text },
        splitLine: { lineStyle: { color: grid, type: "solid" } },
      },
      series: props.fields.map((field) => ({
        name: field.name,
        type: "line",
        showSymbol: false,
        smooth: 0.25,
        connectNulls: false,
        data: props.points.map((point) => {
          const value = point[field.key];
          return value === null
            ? null
            : Number((value / (props.unit === " MiB/s" ? 1048576 : 1)).toFixed(2));
        }),
        lineStyle: { width: 2 },
        emphasis: { focus: "series" },
        areaStyle: props.fields.length === 1 ? { opacity: 0.12 } : undefined,
      })),
    },
    true,
  );
}

/** 初始化图表并监听尺寸变化。参数：无。返回：无。 */
function mount(): void {
  if (!root.value) return;
  chart = init(root.value, undefined, { renderer: "canvas" });
  observer = new ResizeObserver(() => chart?.resize());
  observer.observe(root.value);
  render();
}

/** 释放画布与监听。参数：无。返回：无。 */
function dispose(): void {
  observer?.disconnect();
  chart?.dispose();
}

watch(() => props.points, render, { deep: true });
watch(theme.resolved, render);
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
