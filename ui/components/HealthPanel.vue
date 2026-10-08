<script setup lang="ts">
import { computed } from "vue";
import { NTag, NEmpty } from "naive-ui";
import type { useDesktop } from "../composables/useDesktop";
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();
const health = computed(() => props.desktop.selectedHealth.value);
const enabled = computed(() =>
  ["tcp", "http"].includes(health.value?.config.kind ?? ""),
);
</script>
<template>
  <template v-if="enabled && health"
    ><dl class="info-grid">
      <div>
        <dt>检查类型</dt>
        <dd>{{ health.config.kind.toUpperCase() }}</dd>
      </div>
      <div>
        <dt>当前健康状态</dt>
        <dd>
          <NTag
            :type="
              health.latest
                ? health.latest.healthy
                  ? 'success'
                  : 'error'
                : 'default'
            "
            >{{
              health.latest
                ? health.latest.healthy
                  ? "健康"
                  : "异常"
                : desktop.selectedProcess.value?.pid
                  ? "等待检查"
                  : "未运行"
            }}</NTag
          >
        </dd>
      </div>
      <div>
        <dt>目标</dt>
        <dd>
          {{
            health.config.kind === "tcp"
              ? `127.0.0.1:${health.config.port}`
              : health.config.url
          }}
        </dd>
      </div>
      <div>
        <dt>超时 / 检查间隔</dt>
        <dd>
          {{
            health.config.timeout.secs + health.config.timeout.nanos / 1e9
          }}
          秒 /
          {{
            health.config.interval.secs + health.config.interval.nanos / 1e9
          }}
          秒
        </dd>
      </div>
      <div>
        <dt>最近检查</dt>
        <dd>
          {{
            health.latest
              ? new Date(health.latest.time).toLocaleString("zh-CN", {
                  hour12: false,
                })
              : "—"
          }}
        </dd>
      </div>
      <div>
        <dt>耗时 / 结果</dt>
        <dd>
          {{
            health.latest
              ? `${health.latest.latency_ms} ms · ${health.latest.detail}`
              : "—"
          }}
        </dd>
      </div>
    </dl></template
  >
  <NEmpty
    v-else
    description="未配置健康检查，可在子进程配置中启用"
    class="empty"
  />
  <h3>检查历史 <small>最近 100 次，包含历史实例</small></h3>
  <div class="table-scroll health-history">
    <table class="resource-table">
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
        <tr
          v-for="(record, index) in [...desktop.state.healthHistory].reverse()"
          :key="index"
        >
          <td>
            {{
              new Date(record.time).toLocaleString("zh-CN", { hour12: false })
            }}
          </td>
          <td>{{ record.instance }}</td>
          <td :class="record.healthy ? 'healthy-text' : 'error-text'">
            {{ record.healthy ? "健康" : "异常" }}
          </td>
          <td>{{ record.latency_ms }} ms</td>
          <td>{{ record.detail }}</td>
        </tr>
        <tr v-if="!desktop.state.healthHistory.length">
          <td colspan="5">暂无检查记录</td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
