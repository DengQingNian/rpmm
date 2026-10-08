<script setup lang="ts">
import { computed } from "vue";
import { NAlert, NProgress, NEmpty } from "naive-ui";
import type { useDesktop } from "../composables/useDesktop";
import { bytes, percent } from "../format";
import MetricChart from "./MetricChart.vue";
import AppIcon from "./AppIcon.vue";
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();
const host = computed(() => props.desktop.state.host);
const cards = computed(() => [
  {
    title: "托管进程",
    value: props.desktop.counts.value.total,
    icon: "processes" as const,
  },
  {
    title: "正在运行",
    value: props.desktop.counts.value.active,
    icon: "running" as const,
  },
  {
    title: "进程失败",
    value: props.desktop.counts.value.failed,
    icon: "attention" as const,
  },
  {
    title: "随应用启动",
    value: props.desktop.counts.value.enabled,
    icon: "autostart" as const,
  },
]);
const unhealthy = computed(() =>
  props.desktop.state.healthStatuses.filter((h) => h.latest?.healthy === false),
);
</script>
<template>
  <section class="dashboard-page">
    <div class="stats">
      <article
        v-for="card in cards"
        :key="card.title"
        class="stat"
        :class="{
          green: card.icon === 'running',
          red: card.icon === 'attention' && card.value > 0,
        }"
      >
        <span>{{ card.title }}</span
        ><strong>{{ card.value }}</strong
        ><AppIcon :name="card.icon" class="stat-sketch" :size="34" />
      </article>
    </div>
    <NAlert
      v-if="desktop.state.metricsError"
      type="warning"
      class="preview-note"
      >{{ desktop.state.metricsError }}</NAlert
    >
    <NAlert v-if="unhealthy.length" type="error" class="preview-note"
      >健康检查失败：{{ unhealthy.map((h) => h.unit).join("、") }}</NAlert
    >
    <template v-if="host">
      <div class="resource-cards">
        <article class="paper metric-card">
          <span>宿主机 CPU</span><strong>{{ percent(host.cpu) }}</strong
          ><small>{{ host.name }}</small>
        </article>
        <article class="paper metric-card">
          <span>物理内存</span
          ><strong>{{
            percent((host.memory_used / Math.max(1, host.memory_total)) * 100)
          }}</strong
          ><small
            >{{ bytes(host.memory_used) }} /
            {{ bytes(host.memory_total) }}</small
          >
        </article>
        <article class="paper metric-card">
          <span>磁盘读取</span><strong>{{ bytes(host.read_per_sec) }}/s</strong
          ><small>本次采样间隔平均速率</small>
        </article>
        <article class="paper metric-card">
          <span>磁盘写入</span><strong>{{ bytes(host.write_per_sec) }}/s</strong
          ><small>本次采样间隔平均速率</small>
        </article>
      </div>
      <div class="chart-grid">
        <section class="paper chart-panel">
          <h2>CPU 与内存使用率</h2>
          <MetricChart
            :points="desktop.state.hostHistory"
            :fields="[
              { key: 'cpu', name: 'CPU', color: '#3d7157' },
              { key: 'memory', name: '内存', color: '#99712d' },
            ]"
            unit="%"
            percent
          />
        </section>
        <section class="paper chart-panel">
          <h2>磁盘读写速率</h2>
          <MetricChart
            :points="desktop.state.hostHistory"
            :fields="[
              { key: 'read', name: '读取', color: '#3d7157' },
              { key: 'write', name: '写入', color: '#ac5149' },
            ]"
            unit=" MiB/s"
          />
        </section>
      </div>
      <section class="paper capacity-panel">
        <div class="panel-heading">
          <h2>磁盘容量</h2>
          <small
            >采样
            {{
              new Date(host.time).toLocaleTimeString("zh-CN", { hour12: false })
            }}</small
          >
        </div>
        <div class="disk-grid">
          <article
            v-for="disk in host.disks"
            :key="disk.mount"
            class="disk-card"
          >
            <strong>{{ disk.mount }}</strong
            ><NProgress
              type="line"
              :percentage="
                Math.round(
                  ((disk.total - disk.available) / Math.max(1, disk.total)) *
                    100,
                )
              "
              :status="
                disk.available / Math.max(1, disk.total) < 0.1
                  ? 'error'
                  : 'success'
              "
            />
            <p>
              已用 {{ bytes(disk.total - disk.available) }} /
              {{ bytes(disk.total) }} · 可用 {{ bytes(disk.available) }}
            </p>
            <small
              >读取 {{ bytes(disk.read_per_sec) }}/s · 写入
              {{ bytes(disk.write_per_sec) }}/s</small
            >
          </article>
          <NEmpty
            v-if="!host.disks.length"
            description="未检测到可读取的磁盘"
          />
        </div>
      </section>
    </template>
    <NEmpty v-else description="等待宿主机资源采样" class="paper empty" />
  </section>
</template>
