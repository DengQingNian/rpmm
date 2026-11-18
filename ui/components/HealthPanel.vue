<script setup lang="ts">
import { computed } from "vue";
import type { useDesktop } from "../composables/useDesktop";
import type { StatusTone } from "../types";
import MdBadge from "./MdBadge.vue";
import MdEmpty from "./MdEmpty.vue";

/** 健康检查：当前配置、最近结果与检查历史。 */
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();
const health = computed(() => props.desktop.selectedHealth.value);
const enabled = computed(() => ["tcp", "http"].includes(health.value?.config.kind ?? ""));

/** 当前健康状态文案。参数：无。返回：状态说明。 */
const stateLabel = computed(() => {
  if (!health.value?.latest)
    return props.desktop.selectedProcess.value?.pid ? "等待检查" : "未运行";
  return health.value.latest.healthy ? "健康" : "异常";
});

/** 当前健康状态的语义色。参数：无。返回：状态语义。 */
const stateTone = computed<StatusTone>(() => {
  if (!health.value?.latest) return "neutral";
  return health.value.latest.healthy ? "success" : "error";
});

/** 时间显示。参数：time 为 ISO 时间或空。返回：本地时间文本。 */
function stamp(time: string | undefined): string {
  return time ? new Date(time).toLocaleString("zh-CN", { hour12: false }) : "—";
}
</script>

<template>
  <div class="health-panel">
    <template v-if="enabled && health">
      <dl class="info-grid">
        <div class="info-grid__item">
          <dt>检查类型</dt>
          <dd>{{ health.config.kind.toUpperCase() }}</dd>
        </div>
        <div class="info-grid__item">
          <dt>当前健康状态</dt>
          <dd><MdBadge :tone="stateTone">{{ stateLabel }}</MdBadge></dd>
        </div>
        <div class="info-grid__item">
          <dt>目标</dt>
          <dd class="is-mono">
            {{
              health.config.kind === "tcp"
                ? "127.0.0.1:" + health.config.port
                : health.config.url
            }}
          </dd>
        </div>
        <div class="info-grid__item">
          <dt>超时 / 检查间隔</dt>
          <dd>
            {{ health.config.timeout.secs + health.config.timeout.nanos / 1e9 }} 秒 /
            {{ health.config.interval.secs + health.config.interval.nanos / 1e9 }} 秒
          </dd>
        </div>
        <div class="info-grid__item">
          <dt>最近检查</dt>
          <dd>{{ stamp(health.latest?.time) }}</dd>
        </div>
        <div class="info-grid__item">
          <dt>耗时 / 结果</dt>
          <dd>
            {{
              health.latest ? health.latest.latency_ms + " ms · " + health.latest.detail : "—"
            }}
          </dd>
        </div>
      </dl>
    </template>
    <MdEmpty
      v-else
      icon="shield"
      title="未配置健康检查"
      description="可在服务配置页的“健康监控配置”中启用 TCP 或 HTTP 探测。"
    />

    <section class="panel">
      <div class="panel__head">
        <div>
          <h2 class="panel__title">检查历史</h2>
          <p class="panel__subtitle">本次运行最近 100 次，包含历史实例，不写日志</p>
        </div>
      </div>
      <div class="table-scroll">
        <table class="data-table">
          <thead>
            <tr>
              <th>时间</th>
              <th>实例</th>
              <th>状态</th>
              <th>耗时</th>
              <th>结果</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(record, index) in [...desktop.state.healthHistory].reverse()" :key="index">
              <td>{{ stamp(record.time) }}</td>
              <td>{{ record.instance }}</td>
              <td>
                <MdBadge :tone="record.healthy ? 'success' : 'error'">{{
                  record.healthy ? "健康" : "异常"
                }}</MdBadge>
              </td>
              <td>{{ record.latency_ms }} ms</td>
              <td>{{ record.detail }}</td>
            </tr>
            <tr v-if="!desktop.state.healthHistory.length">
              <td colspan="5" class="is-empty">暂无检查记录</td>
            </tr>
          </tbody>
        </table>
      </div>
    </section>
  </div>
</template>
