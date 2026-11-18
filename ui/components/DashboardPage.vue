<script setup lang="ts">
import { computed } from "vue";
import type { useDesktop } from "../composables/useDesktop";
import { bytes, percent } from "../format";
import AppIcon from "./AppIcon.vue";
import MdAlert from "./MdAlert.vue";
import MdBadge from "./MdBadge.vue";
import MdEmpty from "./MdEmpty.vue";
import MdLinearProgress from "./MdLinearProgress.vue";
import MetricCard from "./MetricCard.vue";
import MetricChart from "./MetricChart.vue";
import StatCard from "./StatCard.vue";

/** Dashboard：托管数量统计、宿主机资源趋势与磁盘容量。 */
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();

const host = computed(() => props.desktop.state.host);

/** 统计卡片数据。参数：无。返回：四张概览卡片。 */
const stats = computed(() => [
  {
    label: "托管进程",
    value: props.desktop.counts.value.total,
    icon: "processes" as const,
    caption: "已登记的 .service 配置",
    target: "config" as const,
  },
  {
    label: "正在运行",
    value: props.desktop.counts.value.active,
    icon: "running" as const,
    caption: "状态为 active 的实例",
    target: "monitor" as const,
  },
  {
    label: "进程失败",
    value: props.desktop.counts.value.failed,
    icon: "attention" as const,
    caption: props.desktop.counts.value.failed ? "需要在监控页处理" : "没有失败的实例",
    target: "monitor" as const,
  },
  {
    label: "随 rpmm 启动",
    value: props.desktop.counts.value.enabled,
    icon: "autostart" as const,
    caption: "管理器启动时自动拉起",
    target: "monitor" as const,
  },
]);

/** 内存占用百分比。参数：无。返回：0～100 的数值。 */
const memoryPercent = computed(() => {
  const current = host.value;
  if (!current) return 0;
  return (current.memory_used / Math.max(1, current.memory_total)) * 100;
});

/** 健康检查失败的进程名。参数：无。返回：进程名列表。 */
const unhealthy = computed(() =>
  props.desktop.state.healthStatuses
    .filter((item) => item.latest?.healthy === false)
    .map((item) => item.unit),
);

/** 磁盘已用百分比。参数：total 为容量，available 为可用容量。返回：0～100。 */
function usedPercent(total: number, available: number): number {
  return Math.round(((total - available) / Math.max(1, total)) * 100);
}

/** 采样时间，便于确认数据新鲜度。参数：无。返回：本地时间文本。 */
const sampledAt = computed(() =>
  host.value
    ? new Date(host.value.time).toLocaleTimeString("zh-CN", { hour12: false })
    : "—",
);
</script>

<template>
  <section class="dashboard-page">
    <div class="stat-grid">
      <StatCard
        v-for="item in stats"
        :key="item.label"
        :label="item.label"
        :value="item.value"
        :icon="item.icon"
        :caption="item.caption"
        :target="item.target"
        @open="desktop.navigate($event)"
      />
    </div>

    <MdAlert v-if="desktop.state.metricsError" kind="warning">
      {{ desktop.state.metricsError }}
    </MdAlert>
    <MdAlert v-if="unhealthy.length" kind="error" title="健康检查失败">
      {{ unhealthy.join("、") }}
    </MdAlert>

    <template v-if="host">
      <div class="metric-grid">
        <MetricCard
          label="宿主机 CPU"
          :value="percent(host.cpu)"
          :supporting="host.name"
          icon="cpu"
          :gauge="host.cpu"
        />
        <MetricCard
          label="物理内存"
          :value="percent(memoryPercent)"
          :supporting="bytes(host.memory_used) + ' / ' + bytes(host.memory_total)"
          icon="memory"
          :gauge="memoryPercent"
        />
        <MetricCard
          label="磁盘读取"
          :value="bytes(host.read_per_sec) + '/s'"
          supporting="本次采样间隔平均速率"
          icon="net-in"
        />
        <MetricCard
          label="磁盘写入"
          :value="bytes(host.write_per_sec) + '/s'"
          supporting="本次采样间隔平均速率"
          icon="net-out"
        />
      </div>

      <div class="chart-grid">
        <section class="panel chart-panel">
          <div class="panel__head">
            <div>
              <h2 class="panel__title">CPU 与内存使用率</h2>
              <p class="panel__subtitle">最近 120 次采样</p>
            </div>
          </div>
          <MetricChart
            :points="desktop.state.hostHistory"
            :fields="[
              { key: 'cpu', name: 'CPU', color: 'var(--chart-1)' },
              { key: 'memory', name: '内存', color: 'var(--chart-2)' },
            ]"
            unit="%"
            percent
          />
        </section>
        <section class="panel chart-panel">
          <div class="panel__head">
            <div>
              <h2 class="panel__title">磁盘读写速率</h2>
              <p class="panel__subtitle">最近 120 次采样</p>
            </div>
          </div>
          <MetricChart
            :points="desktop.state.hostHistory"
            :fields="[
              { key: 'read', name: '读取', color: 'var(--chart-1)' },
              { key: 'write', name: '写入', color: 'var(--chart-3)' },
            ]"
            unit=" MiB/s"
          />
        </section>
      </div>

      <section class="panel">
        <div class="panel__head">
          <div>
            <h2 class="panel__title">磁盘容量</h2>
            <p class="panel__subtitle">采样 {{ sampledAt }}</p>
          </div>
          <MdBadge tone="neutral" dot>
            <AppIcon name="disk" :size="14" /> {{ host.disks.length }} 个卷
          </MdBadge>
        </div>
        <div class="disk-grid">
          <article v-for="disk in host.disks" :key="disk.mount" class="disk-card">
            <div class="disk-card__head">
              <strong class="disk-card__mount">{{ disk.mount }}</strong>
              <span
                class="disk-card__percent"
                :class="{
                  'is-danger': disk.available / Math.max(1, disk.total) < 0.1,
                }"
                >{{ usedPercent(disk.total, disk.available) }}%</span
              >
            </div>
            <MdLinearProgress
              :value="usedPercent(disk.total, disk.available)"
              :label="disk.mount + ' 已用空间'"
            />
            <p class="disk-card__line">
              已用 {{ bytes(disk.total - disk.available) }} / {{ bytes(disk.total) }}
              · 可用 <strong>{{ bytes(disk.available) }}</strong>
            </p>
            <p class="disk-card__line is-muted">
              读取 {{ bytes(disk.read_per_sec) }}/s · 写入
              {{ bytes(disk.write_per_sec) }}/s
            </p>
          </article>
          <MdEmpty
            v-if="!host.disks.length"
            title="未检测到可读取的磁盘"
            description="无法枚举卷容量，请确认当前账户的读取权限。"
          />
        </div>
      </section>
    </template>
    <section v-else class="panel">
      <MdEmpty
        title="等待宿主机资源采样"
        description="首次采样完成后会显示 CPU、内存与磁盘趋势。"
      />
    </section>
  </section>
</template>
