<script setup lang="ts">
import { computed } from "vue";
import {
  NButton,
  NEmpty,
  NInput,
  NSelect,
  NTabPane,
  NTabs,
  NTag,
} from "naive-ui";
import { label, statusType } from "../types";
import type { useDesktop } from "../composables/useDesktop";
import AppIcon from "./AppIcon.vue";
import LogViewer from "./LogViewer.vue";
import ResourcesPanel from "./ResourcesPanel.vue";
import HealthPanel from "./HealthPanel.vue";
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();
const running = computed(() =>
  ["active", "activating", "deactivating"].includes(
    props.desktop.selectedProcess.value?.state ?? "",
  ),
);
</script>
<template>
  <section class="monitor-page">
    <div
      class="workspace"
      :class="{ 'list-collapsed': desktop.state.listCollapsed }"
    >
      <section v-if="!desktop.state.listCollapsed" class="paper process-panel">
        <div class="panel-heading">
          <h2>
            托管进程 <small>{{ desktop.state.statuses.length }}</small>
          </h2>
          <NButton
            quaternary
            aria-label="收起托管进程列表"
            title="收起托管进程列表"
            @click="desktop.state.listCollapsed = true"
            ><template #icon><AppIcon name="collapse" /></template
          ></NButton>
        </div>
        <div class="search">
          <NInput
            v-model:value="desktop.state.search"
            clearable
            placeholder="搜索进程名称…"
            :input-props="{ 'aria-label': '搜索进程' }"
            ><template #prefix><AppIcon name="search" :size="16" /></template
          ></NInput>
        </div>
        <div class="process-list">
          <button
            v-for="process in desktop.filtered.value"
            :key="process.name"
            class="process-row"
            :class="{ selected: process.name === desktop.state.selected }"
            :disabled="desktop.locked.value"
            @click="desktop.selectProcess(process.name)"
          >
            <span class="process-icon"
              ><AppIcon name="processes" :size="20" /></span
            ><span class="process-name"
              ><strong>{{ process.name }}</strong
              ><small
                >PID {{ process.pid ?? "—" }} · 重启
                {{ process.restart_count }} 次</small
              ></span
            ><NTag size="small" :type="statusType(process.state)">{{
              label(process.state)
            }}</NTag>
          </button>
          <NEmpty
            v-if="!desktop.filtered.value.length"
            :description="
              desktop.state.search ? '没有匹配的进程' : '暂无托管进程'
            "
            class="empty"
          >
            <template #extra
              ><p>
                {{
                  desktop.state.search
                    ? "请尝试其他名称。"
                    : "点击新建子进程添加配置。"
                }}
              </p>
              <NButton
                v-if="!desktop.state.search"
                type="primary"
                :disabled="desktop.locked.value"
                @click="desktop.state.createVisible = true"
                ><template #icon><AppIcon name="add" /></template
                >新建子进程</NButton
              ></template
            >
          </NEmpty>
        </div>
      </section>
      <section class="paper detail-panel">
        <div v-if="desktop.state.listCollapsed" class="collapsed-list-bar">
          <NButton
            size="small"
            aria-label="展开托管进程列表"
            @click="desktop.state.listCollapsed = false"
            ><template #icon><AppIcon name="expand" /></template
            >托管进程</NButton
          ><NSelect
            :value="desktop.state.selected || null"
            :options="
              desktop.state.statuses.map((p) => ({
                label: p.name,
                value: p.name,
              }))
            "
            :disabled="desktop.locked.value"
            aria-label="选择托管进程"
            @update:value="desktop.selectProcess($event)"
          />
        </div>
        <template v-if="desktop.selectedProcess.value">
          <div class="panel-heading">
            <div>
              <span class="eyebrow">进程详情</span>
              <h2 class="process-title">
                {{ desktop.selectedProcess.value.name }}
              </h2>
            </div>
            <NTag :type="statusType(desktop.selectedProcess.value.state)">{{
              label(desktop.selectedProcess.value.state)
            }}</NTag>
          </div>
          <div class="process-actions">
            <NButton
              :type="running ? 'default' : 'primary'"
              :disabled="desktop.locked.value"
              @click="desktop.operate(running ? 'stop' : 'start')"
              ><template #icon
                ><AppIcon :name="running ? 'stop' : 'start'" /></template
              >{{ running ? "停止" : "启动" }}</NButton
            ><NButton
              :disabled="desktop.locked.value"
              @click="desktop.operate('restart')"
              ><template #icon><AppIcon name="restart" /></template
              >重启</NButton
            ><NButton
              :disabled="desktop.locked.value"
              @click="
                desktop.operate(
                  desktop.selectedProcess.value.enabled ? 'disable' : 'enable',
                )
              "
              ><template #icon><AppIcon name="autostart" /></template
              >{{
                desktop.selectedProcess.value.enabled
                  ? "取消随应用启动"
                  : "随应用启动"
              }}</NButton
            ><NButton
              v-if="desktop.selectedProcess.value.state === 'failed'"
              :disabled="desktop.locked.value"
              @click="desktop.operate('reset-failed')"
              ><template #icon><AppIcon name="reset" /></template
              >重置失败</NButton
            ><NButton
              text
              :disabled="desktop.locked.value"
              @click="desktop.selectProcess(desktop.state.selected, true)"
              ><template #icon><AppIcon name="edit" /></template
              >编辑配置</NButton
            >
          </div>
          <p v-if="desktop.selectedProcess.value.reason" class="diagnostic">
            {{ desktop.selectedProcess.value.reason }}
          </p>
          <NTabs
            :value="desktop.state.detailTab"
            type="line"
            animated
            @update:value="desktop.setDetailTab"
          >
            <NTabPane name="logs" tab="运行日志"
              ><LogViewer :desktop="desktop"
            /></NTabPane>
            <NTabPane name="resources" tab="资源使用"
              ><ResourcesPanel :desktop="desktop"
            /></NTabPane>
            <NTabPane name="health" tab="健康检查"
              ><HealthPanel :desktop="desktop"
            /></NTabPane>
            <NTabPane name="info" tab="实例信息"
              ><dl class="info-grid">
                <div>
                  <dt>主进程 PID</dt>
                  <dd>{{ desktop.selectedProcess.value.pid ?? "—" }}</dd>
                </div>
                <div>
                  <dt>实例编号</dt>
                  <dd>{{ desktop.selectedProcess.value.instance }}</dd>
                </div>
                <div>
                  <dt>子状态</dt>
                  <dd>{{ desktop.selectedProcess.value.substate }}</dd>
                </div>
                <div>
                  <dt>退出码</dt>
                  <dd>{{ desktop.selectedProcess.value.exit_code ?? "—" }}</dd>
                </div>
                <div>
                  <dt>重启次数</dt>
                  <dd>{{ desktop.selectedProcess.value.restart_count }}</dd>
                </div>
                <div>
                  <dt>实例配置版本</dt>
                  <dd>{{ desktop.selectedProcess.value.config_version }}</dd>
                </div>
                <div>
                  <dt>随应用启动</dt>
                  <dd>
                    {{
                      desktop.selectedProcess.value.enabled
                        ? "已启用"
                        : "未启用"
                    }}
                  </dd>
                </div>
                <div>
                  <dt>运行账户</dt>
                  <dd>当前登录用户</dd>
                </div>
              </dl></NTabPane
            >
          </NTabs>
        </template>
        <NEmpty
          v-else
          description="选择一个进程查看详情"
          class="empty detail-empty"
        ></NEmpty>
      </section>
    </div>
  </section>
</template>
