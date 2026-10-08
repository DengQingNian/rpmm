<script setup lang="ts">
import { computed } from "vue";
import { NAlert, NEmpty, NCollapse, NCollapseItem } from "naive-ui";
import type { useDesktop } from "../composables/useDesktop";
import { bytes, percent } from "../format";
import MetricChart from "./MetricChart.vue";
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();
const resources = computed(() => props.desktop.state.resources);
</script>
<template>
  <NAlert v-if="desktop.state.metricsError" type="warning">{{
    desktop.state.metricsError
  }}</NAlert>
  <template v-if="resources">
    <dl class="info-grid resource-info">
      <div>
        <dt>主进程 CPU（占整机）</dt>
        <dd>{{ percent(resources.cpu) }}</dd>
      </div>
      <div>
        <dt>物理内存 / 虚拟内存</dt>
        <dd>
          {{ bytes(resources.memory) }} / {{ bytes(resources.virtual_memory) }}
        </dd>
      </div>
      <div>
        <dt>启动时间</dt>
        <dd>
          {{
            new Date(resources.start_time * 1000).toLocaleString("zh-CN", {
              hour12: false,
            })
          }}
        </dd>
      </div>
      <div>
        <dt>打开句柄数（含文件、线程等）</dt>
        <dd>{{ resources.open_files ?? "无法获取" }}</dd>
      </div>
      <div>
        <dt>进程 I/O 读取 / 写入速率</dt>
        <dd>
          {{ bytes(resources.read_per_sec) }}/s /
          {{ bytes(resources.write_per_sec) }}/s
        </dd>
      </div>
      <div>
        <dt>累计 I/O 读取 / 写入</dt>
        <dd>
          {{ bytes(resources.read_total) }} / {{ bytes(resources.write_total) }}
        </dd>
      </div>
    </dl>
    <p class="form-note">
      采集范围为托管主进程。Windows 的进程 I/O
      计数可能包含文件、网络和设备操作；句柄总数包含多种对象，当前未枚举文件路径。
    </p>
    <div class="chart-grid process-charts">
      <section class="chart-panel">
        <h2>物理内存</h2>
        <MetricChart
          :points="desktop.state.processHistory"
          :fields="[{ key: 'memory', name: '内存', color: '#99712d' }]"
          unit=" MiB"
        />
      </section>
      <section class="chart-panel">
        <h2>进程 I/O 速率</h2>
        <MetricChart
          :points="desktop.state.processHistory"
          :fields="[
            { key: 'read', name: '读取', color: '#3d7157' },
            { key: 'write', name: '写入', color: '#ac5149' },
          ]"
          unit=" MiB/s"
        />
      </section>
    </div>
    <h3>
      网络连接 <small>{{ resources.connections.length }}</small>
    </h3>
    <NAlert v-if="resources.connection_error" type="warning">{{
      resources.connection_error
    }}</NAlert>
    <div v-else class="table-scroll">
      <table class="resource-table">
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
            <td>{{ connection.local }}</td>
            <td>{{ connection.remote }}</td>
            <td>{{ connection.state }}</td>
          </tr>
          <tr v-if="!resources.connections.length">
            <td colspan="4">暂无 TCP/UDP 连接</td>
          </tr>
        </tbody>
      </table>
    </div>
    <NCollapse class="environment-panel"
      ><NCollapseItem
        :title="`环境变量（${Object.keys(resources.environment).length}）`"
        name="environment"
        ><p class="form-note">{{ resources.environment_source }}</p>
        <div class="table-scroll">
          <table class="resource-table">
            <thead>
              <tr>
                <th>名称</th>
                <th>值</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="(value, name) in resources.environment" :key="name">
                <td>{{ name }}</td>
                <td class="environment-value">{{ value }}</td>
              </tr>
            </tbody>
          </table>
        </div></NCollapseItem
      ></NCollapse
    >
  </template>
  <NEmpty
    v-else
    :description="
      desktop.selectedProcess.value?.pid
        ? '等待进程资源采样，进程也可能已退出'
        : '进程未运行，暂无资源数据'
    "
    class="empty"
  />
</template>
