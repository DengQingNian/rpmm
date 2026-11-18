<script setup lang="ts">
import { computed } from "vue";
import type { useDesktop } from "../composables/useDesktop";
import { label, statusTone } from "../types";
import AppIcon from "./AppIcon.vue";
import HealthPanel from "./HealthPanel.vue";
import LogViewer from "./LogViewer.vue";
import MdBadge from "./MdBadge.vue";
import MdButton from "./MdButton.vue";
import MdEmpty from "./MdEmpty.vue";
import MdIconButton from "./MdIconButton.vue";
import MdSelect from "./MdSelect.vue";
import MdTabs from "./MdTabs.vue";
import MdTextField from "./MdTextField.vue";
import ResourcesPanel from "./ResourcesPanel.vue";

/** 进程监控：左侧进程列表（可收起），右侧详情与标签页。 */
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();

const selected = computed(() => props.desktop.selectedProcess.value);
const running = computed(() =>
  ["active", "activating", "deactivating"].includes(selected.value?.state ?? ""),
);

const tabs = [
  { key: "logs", label: "运行日志" },
  { key: "resources", label: "资源使用" },
  { key: "health", label: "健康检查" },
  { key: "info", label: "实例信息" },
];

const selectOptions = computed(() =>
  props.desktop.state.statuses.map((item) => ({
    label: item.name,
    value: item.name,
    hint: label(item.state),
  })),
);

/** 实例信息行。参数：无。返回：键值对列表。 */
const infoRows = computed(() => {
  const process = selected.value;
  if (!process) return [];
  return [
    { key: "主进程 PID", value: process.pid ?? "—" },
    { key: "实例编号", value: process.instance },
    { key: "子状态", value: process.substate },
    { key: "退出码", value: process.exit_code ?? "—" },
    { key: "重启次数", value: process.restart_count },
    { key: "实例配置版本", value: process.config_version },
    { key: "随 rpmm 启动", value: process.enabled ? "已启用" : "未启用" },
    { key: "运行账户", value: "当前登录用户" },
  ];
});
</script>

<template>
  <section class="monitor-page" :class="{ 'is-collapsed': desktop.state.listCollapsed }">
    <!-- 进程列表 -->
    <aside v-if="!desktop.state.listCollapsed" class="panel process-panel">
      <div class="panel__head">
        <div>
          <h2 class="panel__title">托管进程</h2>
          <p class="panel__subtitle">{{ desktop.state.statuses.length }} 个已登记</p>
        </div>
        <MdIconButton
          name="collapse"
          label="收起托管进程列表"
          @click="desktop.state.listCollapsed = true"
        />
      </div>
      <div class="process-panel__search">
        <MdTextField
          v-model="desktop.state.search"
          label="搜索进程名称"
          placeholder="输入名称过滤"
          prefix-icon="search"
          clearable
        />
      </div>
      <ul class="process-list" role="listbox" aria-label="托管进程">
        <li v-for="process in desktop.filtered.value" :key="process.name">
          <button
            type="button"
            role="option"
            class="process-item"
            :class="{ 'is-selected': process.name === desktop.state.selected }"
            :aria-selected="process.name === desktop.state.selected"
            :disabled="desktop.locked.value"
            @click="desktop.selectProcess(process.name)"
          >
            <span class="process-item__avatar" :class="'is-' + statusTone(process.state)">
              <AppIcon name="processes" :size="18" />
            </span>
            <span class="process-item__text">
              <strong class="process-item__name">{{ process.name }}</strong>
              <span class="process-item__meta">
                PID {{ process.pid ?? "—" }} · 重启 {{ process.restart_count }} 次
              </span>
            </span>
            <MdBadge :tone="statusTone(process.state)">{{ label(process.state) }}</MdBadge>
          </button>
        </li>
      </ul>
      <MdEmpty
        v-if="!desktop.filtered.value.length"
        compact
        :icon="desktop.state.search ? 'search-empty' : 'empty'"
        :title="desktop.state.search ? '没有匹配的进程' : '暂无托管进程'"
        :description="
          desktop.state.search ? '请尝试其他名称。' : '点击“新建子进程”添加配置。'
        "
      >
        <MdButton
          v-if="!desktop.state.search"
          variant="tonal"
          icon="add"
          :disabled="desktop.locked.value"
          @click="desktop.state.createVisible = true"
          >新建子进程</MdButton
        >
      </MdEmpty>
    </aside>

    <!-- 详情 -->
    <section class="panel detail-panel">
      <div v-if="desktop.state.listCollapsed" class="detail-panel__bar">
        <MdButton
          variant="tonal"
          size="small"
          icon="expand"
          @click="desktop.state.listCollapsed = false"
          >托管进程</MdButton
        >
        <MdSelect
          :model-value="desktop.state.selected || null"
          :options="selectOptions"
          label="选择托管进程"
          placeholder="选择进程"
          width="240px"
          :disabled="desktop.locked.value"
          @update:model-value="desktop.selectProcess($event)"
        />
      </div>

      <template v-if="selected">
        <header class="detail-panel__head">
          <div class="detail-panel__title">
            <span class="detail-panel__eyebrow">进程详情</span>
            <h2>{{ selected.name }}</h2>
            <MdBadge :tone="statusTone(selected.state)">{{ label(selected.state) }}</MdBadge>
          </div>
          <div class="detail-panel__actions">
            <MdButton
              :variant="running ? 'filled' : 'filled'"
              :icon="running ? 'stop' : 'start'"
              :class="{ 'is-destructive': running }"
              :disabled="desktop.locked.value"
              @click="desktop.operate(running ? 'stop' : 'start')"
              >{{ running ? "停止" : "启动" }}</MdButton
            >
            <MdButton
              variant="tonal"
              icon="restart"
              :disabled="desktop.locked.value"
              @click="desktop.operate('restart')"
              >重启</MdButton
            >
            <MdButton
              variant="outlined"
              icon="autostart"
              :disabled="desktop.locked.value"
              @click="desktop.operate(selected.enabled ? 'disable' : 'enable')"
              >{{ selected.enabled ? "取消随 rpmm 启动" : "随 rpmm 启动" }}</MdButton
            >
            <MdButton
              v-if="selected.state === 'failed'"
              variant="outlined"
              icon="reset"
              :disabled="desktop.locked.value"
              @click="desktop.operate('reset-failed')"
              >重置失败</MdButton
            >
            <MdButton
              variant="text"
              icon="edit"
              :disabled="desktop.locked.value"
              @click="desktop.selectProcess(desktop.state.selected, true)"
              >编辑配置</MdButton
            >
          </div>
        </header>

        <div v-if="selected.reason" class="detail-panel__diagnostic">
          <AppIcon name="attention" :size="18" />
          <span>{{ selected.reason }}</span>
        </div>

        <MdTabs
          :model-value="desktop.state.detailTab"
          :tabs="tabs"
          label="进程详情标签"
          class="detail-panel__tabs"
          @update:model-value="desktop.setDetailTab($event)"
        />

        <div class="detail-panel__body">
          <LogViewer v-if="desktop.state.detailTab === 'logs'" :desktop="desktop" />
          <ResourcesPanel v-else-if="desktop.state.detailTab === 'resources'" :desktop="desktop" />
          <HealthPanel v-else-if="desktop.state.detailTab === 'health'" :desktop="desktop" />
          <dl v-else class="info-grid">
            <div v-for="row in infoRows" :key="row.key" class="info-grid__item">
              <dt>{{ row.key }}</dt>
              <dd>{{ row.value }}</dd>
            </div>
          </dl>
        </div>
      </template>
      <MdEmpty
        v-else
        title="选择一个进程查看详情"
        description="左侧列表展示所有托管配置，选中后可查看日志、资源与健康状态。"
      />
    </section>
  </section>
</template>
