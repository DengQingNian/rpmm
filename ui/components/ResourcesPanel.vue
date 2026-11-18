<script setup lang="ts">
import { computed, ref } from "vue";
import type { useDesktop } from "../composables/useDesktop";
import { bytes, percent } from "../format";
import AppIcon from "./AppIcon.vue";
import MdAlert from "./MdAlert.vue";
import MdBadge from "./MdBadge.vue";
import MdEmpty from "./MdEmpty.vue";
import MetricChart from "./MetricChart.vue";

/** 资源使用：主进程关键指标、趋势图、网络连接与进程环境变量。 */
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();
const resources = computed(() => props.desktop.state.resources);
const showEnvironment = ref(false);
</script>

<template>
  <div class="resources-panel">
    <MdAlert v-if="desktop.state.metricsError" kind="warning">
      {{ desktop.state.metricsError }}
    </MdAlert>

    <template v-if="resources">
      <div class="metric-grid metric-grid--compact">
        <article class="metric-card">
          <div class="metric-card__body">
            <span class="metric-card__label"><AppIcon name="cpu" :size="16" />主进程 CPU</span>
            <strong class="metric-card__value">{{ percent(resources.cpu) }}</strong>
            <span class="metric-card__support">占整机处理器</span>
          </div>
        </article>
        <article class="metric-card">
          <div class="metric-card__body">
            <span class="metric-card__label"><AppIcon name="memory" :size="16" />物理内存</span>
            <strong class="metric-card__value">{{ bytes(resources.memory) }}</strong>
            <span class="metric-card__support">虚拟内存 {{ bytes(resources.virtual_memory) }}</span>
          </div>
        </article>
        <article class="metric-card">
          <div class="metric-card__body">
            <span class="metric-card__label"><AppIcon name="net-in" :size="16" />I/O 读取</span>
            <strong class="metric-card__value">{{ bytes(resources.read_per_sec) }}/s</strong>
            <span class="metric-card__support">累计 {{ bytes(resources.read_total) }}</span>
          </div>
        </article>
        <article class="metric-card">
          <div class="metric-card__body">
            <span class="metric-card__label"><AppIcon name="net-out" :size="16" />I/O 写入</span>
            <strong class="metric-card__value">{{ bytes(resources.write_per_sec) }}/s</strong>
            <span class="metric-card__support">累计 {{ bytes(resources.write_total) }}</span>
          </div>
        </article>
      </div>

      <dl class="info-grid">
        <div class="info-grid__item">
          <dt>启动时间</dt>
          <dd>
            {{
              new Date(resources.start_time * 1000).toLocaleString("zh-CN", {
                hour12: false,
              })
            }}
          </dd>
        </div>
        <div class="info-grid__item">
          <dt>打开句柄数（含文件、线程等）</dt>
          <dd>{{ resources.open_files ?? "无法获取" }}</dd>
        </div>
      </dl>
      <p class="form-note">
        采集范围为托管主进程。Windows 的进程 I/O 计数可能包含文件、网络和设备操作；句柄总数包含多种对象，当前未枚举文件路径。
      </p>

      <div class="chart-grid">
        <section class="panel chart-panel">
          <div class="panel__head">
            <div>
              <h2 class="panel__title">物理内存</h2>
              <p class="panel__subtitle">单位 MiB，最近 120 次采样</p>
            </div>
          </div>
          <MetricChart
            :points="desktop.state.processHistory"
            :fields="[{ key: 'memory', name: '内存', color: 'var(--chart-2)' }]"
            unit=" MiB"
          />
        </section>
        <section class="panel chart-panel">
          <div class="panel__head">
            <div>
              <h2 class="panel__title">进程 I/O 速率</h2>
              <p class="panel__subtitle">单位 MiB/s，最近 120 次采样</p>
            </div>
          </div>
          <MetricChart
            :points="desktop.state.processHistory"
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
            <h2 class="panel__title">网络连接</h2>
            <p class="panel__subtitle">主进程的 TCP/UDP 端点</p>
          </div>
          <MdBadge tone="neutral" dot>{{ resources.connections.length }}</MdBadge>
        </div>
        <MdAlert v-if="resources.connection_error" kind="warning">
          {{ resources.connection_error }}
        </MdAlert>
        <div v-else class="table-scroll">
          <table class="data-table">
            <thead>
              <tr>
                <th>协议</th>
                <th>本地地址</th>
                <th>远程地址</th>
                <th>状态</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="(connection, index) in resources.connections" :key="index">
                <td>{{ connection.protocol }}</td>
                <td class="is-mono">{{ connection.local }}</td>
                <td class="is-mono">{{ connection.remote }}</td>
                <td>{{ connection.state }}</td>
              </tr>
              <tr v-if="!resources.connections.length">
                <td colspan="4" class="is-empty">暂无 TCP/UDP 连接</td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>

      <section class="panel">
        <div class="panel__head">
          <div>
            <h2 class="panel__title">环境变量</h2>
            <p class="panel__subtitle">{{ resources.environment_source }}</p>
          </div>
          <button
            type="button"
            class="md-text-toggle"
            :aria-expanded="showEnvironment"
            @click="showEnvironment = !showEnvironment"
          >
            <AppIcon :name="showEnvironment ? 'chevron-down' : 'chevron-right'" :size="18" />
            {{ showEnvironment ? "收起" : "展开" }} {{ Object.keys(resources.environment).length }} 项
          </button>
        </div>
        <div v-if="showEnvironment" class="table-scroll">
          <table class="data-table">
            <thead>
              <tr>
                <th>名称</th>
                <th>值</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="(value, name) in resources.environment" :key="name">
                <td class="is-mono">{{ name }}</td>
                <td class="is-mono environment-value">{{ value }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>
    </template>

    <MdEmpty
      v-else
      icon="memory"
      :title="
        desktop.selectedProcess.value?.pid ? '等待进程资源采样' : '进程未运行，暂无资源数据'
      "
      :description="
        desktop.selectedProcess.value?.pid
          ? '进程可能刚刚退出，请稍后刷新或检查运行日志。'
          : '启动进程后这里会显示 CPU、内存、I/O 与连接信息。'
      "
    />
  </div>
</template>
