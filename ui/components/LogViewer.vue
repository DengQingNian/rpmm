<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import type { useDesktop } from "../composables/useDesktop";
import { searchLogs } from "../logSearch";
import { label } from "../types";
import type { LogRecord } from "../types";
import AppIcon from "./AppIcon.vue";
import MdButton from "./MdButton.vue";
import MdCheckbox from "./MdCheckbox.vue";
import MdIconButton from "./MdIconButton.vue";
import MdSelect from "./MdSelect.vue";
import MdTextField from "./MdTextField.vue";

/** 运行日志面板：来源与行数筛选、关键字搜索（含正则与上下文）与全屏浏览。 */
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();
const state = props.desktop.state;
const viewer = ref<HTMLElement | null>(null);
const scroller = ref<HTMLElement | null>(null);
const fullscreenButton = ref<InstanceType<typeof MdIconButton> | null>(null);
let previousOverflow = "";

const ROW_HEIGHT = 28;
const OVERSCAN = 8;
const scrollTop = ref(0);
const viewportHeight = ref(420);
const measuredHeight = ref(0);

const result = computed(() =>
  searchLogs(
    state.records,
    state.logQuery,
    state.logRegex,
    state.logIgnoreCase,
    state.logContext,
  ),
);

/** 可见行范围。参数：无。返回：起始与结束下标（含结束）。 */
const window_ = computed(() => {
  const rows = result.value.rows;
  if (!rows.length) return { start: 0, end: 0, offset: 0, total: 0 };
  const height = state.logFullscreen
    ? window.innerHeight - 300
    : Math.max(viewportHeight.value, measuredHeight.value);
  const visible = Math.max(1, Math.ceil(Math.max(height, ROW_HEIGHT) / ROW_HEIGHT));
  const start = Math.min(
    rows.length - 1,
    Math.max(0, Math.floor(scrollTop.value / ROW_HEIGHT) - OVERSCAN),
  );
  const end = Math.min(rows.length, start + visible + OVERSCAN * 2);
  return { start, end, offset: start * ROW_HEIGHT, total: rows.length };
});

/** 当前渲染的行。参数：无。返回：行切片。 */
const visibleRows = computed(() =>
  result.value.rows.slice(window_.value.start, window_.value.end),
);

const sourceOptions = [
  { label: "全部来源", value: "" },
  { label: "标准输出", value: "stdout" },
  { label: "错误输出", value: "stderr" },
  { label: "生命周期", value: "manager" },
];
const linesOptions = [200, 500, 1000, 10000].map((value) => ({
  label: "最近 " + value + " 行",
  value: String(value),
}));
const refreshOptions = [0, 500, 1000, 2000, 5000, 10000, 30000].map((value) => ({
  label: value ? value / 1000 + " 秒刷新" : "暂停刷新",
  value: String(value),
}));

/** 把上下文行数限制在 0～100。参数：value 为输入文本。返回：无。 */
function setContext(value: string): void {
  const parsed = Number(value);
  state.logContext = Number.isFinite(parsed)
    ? Math.min(100, Math.max(0, Math.round(parsed)))
    : 0;
}

/** 跟随筛选后的最新行。参数：无。返回：无；关闭自动滚动时保留当前位置。 */
async function follow(): Promise<void> {
  await nextTick();
  if (!state.logScroll || !scroller.value) return;
  scroller.value.scrollTop = scroller.value.scrollHeight;
  scrollTop.value = scroller.value.scrollTop;
}
watch(() => result.value.rows, follow);
watch(() => state.logScroll, follow);
watch(
  () => state.logFullscreen,
  () => void nextTick(measure),
);

/** 记录滚动位置以渲染正确的行窗口。参数：无。返回：无。 */
function onScroll(): void {
  const top = scroller.value?.scrollTop ?? 0;
  scrollTop.value = top;
  measuredHeight.value = Math.max(measuredHeight.value, top + (scroller.value?.clientHeight ?? 0));
}

/** 记录可视高度并重置窗口基准。参数：无。返回：无。 */
function measure(): void {
  viewportHeight.value = scroller.value?.clientHeight ?? 420;
  measuredHeight.value = 0;
  onScroll();
}

/** 控制全屏时的页面滚动。参数：fullscreen 为全屏状态。返回：无。 */
function lockPage(fullscreen: boolean): void {
  if (fullscreen) {
    previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
  } else document.body.style.overflow = previousOverflow;
}
watch(() => state.logFullscreen, lockPage);

/** 切换全屏并恢复触发按钮焦点。参数：无。返回：无。 */
async function toggleFullscreen(): Promise<void> {
  state.logFullscreen = !state.logFullscreen;
  await nextTick();
  fullscreenButton.value?.$el?.focus();
  measure();
  await follow();
}

/** 全屏下的快捷键：Esc 退出，Tab 在面板内循环。参数：event 为键盘事件。返回：无。 */
function onKey(event: KeyboardEvent): void {
  if (!state.logFullscreen) return;
  if (event.key === "Escape") {
    void toggleFullscreen();
    return;
  }
  if (event.key !== "Tab" || !viewer.value) return;
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

/** 下载当前筛选结果。参数：无。返回：无；保留时间、来源、原始行号和上下文分隔。 */
async function download(): Promise<void> {
  const text = result.value.rows
    .map(
      (row) =>
        (row.separator ? "--\n" : "") +
        (row.index + 1) +
        "\t" +
        row.record.time +
        "\t[" +
        row.record.source +
        "] " +
        row.record.text.replace(/\r?\n$/, ""),
    )
    .join("\n");
  if (!props.desktop.preview) {
    await props.desktop.downloadLogs(text);
    return;
  }
  const url = URL.createObjectURL(new Blob([text], { type: "text/plain;charset=utf-8" }));
  const link = document.createElement("a");
  link.href = url;
  link.download =
    (state.selected || "rpmm") +
    "-" +
    new Date().toISOString().replaceAll(":", "-") +
    ".log";
  link.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** 把刷新间隔字符串写回状态。参数：value 为选项值。返回：无。 */
function setRefresh(value: string): void {
  state.logRefresh = Number(value);
}

/** 把行数选项写回状态并立即重新查询。参数：value 为选项值。返回：无。 */
function setLines(value: string): void {
  state.logLines = Number(value);
  void props.desktop.updateLogs();
}

/** 把来源写回状态并立即重新查询。参数：value 为选项值。返回：无。 */
function setSource(value: string): void {
  state.logSource = value;
  void props.desktop.updateLogs();
}

/** 格式化时间。参数：time 为 ISO 时间。返回：时分秒文本。 */
function clock(time: string): string {
  return new Date(time).toLocaleTimeString("zh-CN", { hour12: false });
}

/** 摘要文本。参数：无。返回：统计或错误说明。 */
const summary = computed(() => {
  if (result.value.error) return result.value.error;
  const matched = state.logQuery ? " · 命中 " + result.value.matches + " 行" : "";
  return (
    "已加载 " + state.records.length + " 行 · 显示 " + result.value.rows.length + " 行" + matched
  );
});

/** 挂载：安装快捷键并测量尺寸。参数：无。返回：无。 */
function mount(): void {
  window.addEventListener("keydown", onKey);
  window.addEventListener("resize", measure);
  void nextTick(() => {
    measure();
    void follow();
  });
}

/** 释放快捷键与全屏状态。参数：无。返回：无。 */
function dispose(): void {
  window.removeEventListener("keydown", onKey);
  window.removeEventListener("resize", measure);
  state.logFullscreen = false;
  document.body.style.overflow = previousOverflow;
}
onMounted(mount);
onUnmounted(dispose);

/** 日志来源的样式类。参数：source 为来源。返回：样式类名。 */
function sourceClass(source: LogRecord["source"]): string {
  return "log-source log-source--" + source;
}
</script>

<template>
  <Teleport to="body" :disabled="!state.logFullscreen">
    <section
      ref="viewer"
      class="log-viewer"
      :class="{ 'log-viewer--fullscreen': state.logFullscreen }"
      :role="state.logFullscreen ? 'dialog' : undefined"
      :aria-modal="state.logFullscreen || undefined"
      aria-label="运行日志"
    >
      <header class="log-viewer__head">
        <div>
          <h3 class="log-viewer__title">{{ state.selected }} · 运行日志</h3>
          <p class="log-viewer__summary" :class="{ 'is-error': result.error }">
            {{ summary }}
          </p>
        </div>
        <div class="log-viewer__actions">
          <MdButton
            variant="tonal"
            size="small"
            icon="reload"
            :loading="state.logLoading"
            :disabled="desktop.preview"
            @click="desktop.updateLogs()"
            >刷新</MdButton
          >
          <MdButton
            variant="text"
            size="small"
            icon="download"
            :disabled="!result.rows.length"
            @click="download"
            >导出</MdButton
          >
          <MdIconButton
            ref="fullscreenButton"
            :name="state.logFullscreen ? 'restore' : 'expand'"
            :label="state.logFullscreen ? '退出全屏日志' : '全屏日志'"
            :selected="state.logFullscreen"
            @click="toggleFullscreen"
          />
        </div>
      </header>

      <div class="log-viewer__controls">
        <MdSelect
          :model-value="state.logSource"
          :options="sourceOptions"
          label="日志来源"
          width="132px"
          @update:model-value="setSource"
        />
        <MdSelect
          :model-value="String(state.logLines)"
          :options="linesOptions"
          label="最近日志行数"
          width="132px"
          @update:model-value="setLines"
        />
        <MdSelect
          :model-value="String(state.logRefresh)"
          :options="refreshOptions"
          label="日志刷新间隔"
          width="128px"
          @update:model-value="setRefresh"
        />
        <MdCheckbox v-model="state.logScroll" label="自动滚动" />
      </div>

      <div class="log-viewer__search">
        <div class="log-viewer__query">
          <MdTextField
            v-model="state.logQuery"
            label="搜索日志内容"
            placeholder="关键字"
            prefix-icon="search"
            clearable
          />
        </div>
        <MdCheckbox v-model="state.logRegex" label="正则" />
        <MdCheckbox v-model="state.logIgnoreCase" label="忽略大小写" />
        <div class="log-viewer__context">
          <MdTextField
            :model-value="String(state.logContext)"
            label="-C 上下文"
            inputmode="numeric"
            @update:model-value="setContext"
          />
        </div>
      </div>

      <div
        ref="scroller"
        class="log-viewer__output"
        tabindex="0"
        role="log"
        aria-live="off"
        aria-label="日志内容"
        @scroll="onScroll"
      >
        <div class="log-viewer__spacer" :style="{ height: window_.total * ROW_HEIGHT + 'px' }">
          <div class="log-viewer__window" :style="{ transform: 'translateY(' + window_.offset + 'px)' }">
            <template v-for="row in visibleRows" :key="row.key">
              <div v-if="row.separator" class="log-separator">
                <span>-- 断开的上下文 --</span>
              </div>
              <div
                class="log-line"
                :class="{ 'is-match': row.matched }"
                :style="{ height: ROW_HEIGHT + 'px' }"
              >
                <span class="log-line__number">{{ row.index + 1 }}</span>
                <time class="log-line__time">{{ clock(row.record.time) }}</time>
                <span :class="sourceClass(row.record.source)">{{ label(row.record.source) }}</span>
                <span class="log-line__text">{{ row.record.text }}</span>
              </div>
            </template>
          </div>
        </div>
        <div v-if="!result.rows.length" class="log-viewer__empty">
          <AppIcon name="search-empty" :size="24" />
          <p>{{ state.logQuery ? "没有匹配的日志，试试新的关键字或关闭正则" : "暂无运行日志" }}</p>
        </div>
      </div>
      <p class="log-viewer__foot">搜索范围：所选最近行数；-C 为命中行前后的上下文行数。</p>
    </section>
  </Teleport>
</template>