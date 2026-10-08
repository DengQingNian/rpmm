<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import {
  NButton,
  NInput,
  NInputNumber,
  NSelect,
  NCheckbox,
  NVirtualList,
  type VirtualListInst,
} from "naive-ui";
import type { useDesktop } from "../composables/useDesktop";
import { label } from "../types";
import { searchLogs } from "../logSearch";
import AppIcon from "./AppIcon.vue";
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();
const state = props.desktop.state;
const output = ref<VirtualListInst | null>(null);
const viewer = ref<HTMLElement | null>(null);
let previousOverflow = "";
const fullscreenButton = ref<InstanceType<typeof NButton> | null>(null);
const result = computed(() =>
  searchLogs(
    state.records,
    state.logQuery,
    state.logRegex,
    state.logIgnoreCase,
    state.logContext,
  ),
);
const sourceOptions = [
  { label: "全部来源", value: "" },
  { label: "标准输出", value: "stdout" },
  { label: "错误输出", value: "stderr" },
  { label: "生命周期", value: "manager" },
];
const linesOptions = [200, 500, 1000, 10000].map((value) => ({
  label: `最近 ${value} 行`,
  value,
}));
const refreshOptions = [0, 500, 1000, 2000, 5000, 10000, 30000].map(
  (value) => ({ label: value ? `${value / 1000} 秒刷新` : "暂停刷新", value }),
);
/** 跟随筛选后的最新行。参数：无。返回：无；关闭自动滚动时保留当前位置。 */
async function follow(): Promise<void> {
  await nextTick();
  if (state.logScroll && result.value.rows.length)
    output.value?.scrollTo({
      index: result.value.rows.length - 1,
      position: "bottom",
    });
}
watch(() => result.value.rows, follow);
watch(() => state.logScroll, follow);
/** 控制全屏时的页面滚动。参数：fullscreen 为全屏状态。返回：无。 */
function lockPage(fullscreen: boolean): void {
  if (fullscreen) {
    previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
  } else document.body.style.overflow = previousOverflow;
}
watch(() => state.logFullscreen, lockPage);
/** 切换日志全屏并恢复触发按钮焦点。参数：无。返回：无。 */
async function toggleFullscreen(): Promise<void> {
  state.logFullscreen = !state.logFullscreen;
  await nextTick();
  fullscreenButton.value?.$el.focus();
  await follow();
}
/** 响应退出全屏快捷键。参数：event 为键盘事件。返回：无。 */
function onKey(event: KeyboardEvent): void {
  if (!state.logFullscreen) return;
  if (event.key === "Escape") {
    void toggleFullscreen();
    return;
  }
  // 将 Tab 焦点限定在全屏日志的首尾控件之间，避免进入遮挡的页面。
  if (event.key === "Tab" && viewer.value) {
    const elements = Array.from(
      viewer.value.querySelectorAll<HTMLElement>(
        'button:not(:disabled), input:not(:disabled), [tabindex="0"]',
      ),
    );
    const first = elements[0];
    const last = elements.at(-1);
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  }
}
/** 下载当前筛选结果。参数：无。返回：无；保留时间、来源、原始行号和上下文分隔。 */
async function download(): Promise<void> {
  const text = result.value.rows
    .map(
      (row) =>
        `${row.separator ? "--\n" : ""}${row.index + 1}\t${row.record.time}\t[${row.record.source}] ${row.record.text.replace(/\r?\n$/, "")}`,
    )
    .join("\n");
  if (!props.desktop.preview) {
    await props.desktop.downloadLogs(text);
    return;
  }
  const url = URL.createObjectURL(
    new Blob([text], { type: "text/plain;charset=utf-8" }),
  );
  const link = document.createElement("a");
  link.href = url;
  link.download = `${state.selected}-${new Date().toISOString().replaceAll(":", "-")}.log`;
  link.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}
/** 安装快捷键。参数：无。返回：无。 */
function mount(): void {
  window.addEventListener("keydown", onKey);
  void follow();
}
/** 释放快捷键与全屏状态。参数：无。返回：无。 */
function dispose(): void {
  window.removeEventListener("keydown", onKey);
  state.logFullscreen = false;
  document.body.style.overflow = previousOverflow;
}
onMounted(mount);
onUnmounted(dispose);
</script>
<template>
  <Teleport to="body" :disabled="!state.logFullscreen">
    <section
      ref="viewer"
      class="log-viewer"
      :class="{ 'log-fullscreen': state.logFullscreen }"
      :role="state.logFullscreen ? 'dialog' : undefined"
      :aria-modal="state.logFullscreen || undefined"
      aria-label="运行日志"
    >
      <div class="log-viewer-heading">
        <strong>{{ state.selected }} · 运行日志</strong>
        <div class="inline-actions">
          <NButton
            size="small"
            :loading="state.logLoading"
            :disabled="desktop.preview"
            @click="desktop.updateLogs"
            ><template #icon><AppIcon name="reload" /></template>刷新</NButton
          >
          <NButton
            size="small"
            :disabled="!result.rows.length"
            @click="download"
            ><template #icon><AppIcon name="download" /></template
            >下载日志</NButton
          >
          <NButton
            ref="fullscreenButton"
            quaternary
            :title="state.logFullscreen ? '退出全屏（Esc）' : '全屏日志'"
            :aria-label="state.logFullscreen ? '退出全屏日志' : '全屏日志'"
            @click="toggleFullscreen"
            ><template #icon
              ><AppIcon
                :name="
                  state.logFullscreen ? 'restore' : 'fullscreen'
                " /></template
          ></NButton>
        </div>
      </div>
      <div class="log-controls">
        <NSelect
          v-model:value="state.logSource"
          :options="sourceOptions"
          aria-label="日志来源"
          @update:value="desktop.updateLogs"
        />
        <NSelect
          v-model:value="state.logLines"
          :options="linesOptions"
          aria-label="最近日志行数"
          @update:value="desktop.updateLogs"
        />
        <NSelect
          v-model:value="state.logRefresh"
          :options="refreshOptions"
          aria-label="日志刷新时间"
        />
        <NCheckbox v-model:checked="state.logScroll">自动滚动</NCheckbox>
      </div>
      <div class="log-search-controls">
        <NInput
          v-model:value="state.logQuery"
          clearable
          placeholder="搜索日志内容"
          :input-props="{ 'aria-label': '搜索日志' }"
          ><template #prefix><AppIcon name="search" :size="16" /></template
        ></NInput>
        <NCheckbox v-model:checked="state.logRegex">正则</NCheckbox
        ><NCheckbox v-model:checked="state.logIgnoreCase">忽略大小写</NCheckbox>
        <label class="context-control"
          >-C
          <NInputNumber
            v-model:value="state.logContext"
            :min="0"
            :max="100"
            :precision="0"
            :input-props="{ 'aria-label': '匹配前后上下文行数' }"
        /></label>
      </div>
      <p class="log-summary" :class="{ 'error-text': result.error }">
        {{
          result.error ||
          `已加载 ${state.records.length} 行 · 显示 ${result.rows.length} 行${state.logQuery ? ` · 命中 ${result.matches} 行` : ""}`
        }}<span>搜索范围：所选最近行数；-C 为前后上下文</span>
      </p>
      <NVirtualList
        ref="output"
        class="log-output virtual-logs"
        :items="result.rows"
        :item-size="25"
        item-resizable
        key-field="key"
      >
        <template #default="{ item }"
          ><div class="log-row-wrapper" :key="item.key">
            <div v-if="item.separator" class="log-separator">--</div>
            <div class="log-line" :class="{ 'log-match': item.matched }">
              <span class="line-number">{{ item.index + 1 }}</span
              ><time>{{
                new Date(item.record.time).toLocaleTimeString("zh-CN", {
                  hour12: false,
                })
              }}</time
              ><span class="log-source" :class="item.record.source">{{
                label(item.record.source)
              }}</span
              ><span class="log-text">{{ item.record.text }}</span>
            </div>
          </div></template
        >
      </NVirtualList>
      <p v-if="!result.rows.length" class="log-empty">
        {{ state.logQuery ? "没有匹配的日志" : "暂无运行日志" }}
      </p>
    </section>
  </Teleport>
</template>
