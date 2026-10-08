import { computed, onMounted, onUnmounted, reactive, watch } from "vue";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useDialog, useMessage } from "naive-ui";
import type {
  ConfigDocument,
  LogRecord,
  Page,
  ProcessDraft,
  ProcessStatus,
  Settings,
  HealthDraft,
  HealthStatus,
  HealthRecord,
  HostMetrics,
  ProcessResources,
  MetricPoint,
} from "../types";

/** 调用桌面接口。参数：command 为命令，args 为命令参数。返回：后端响应。 */
async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!isTauri()) throw new Error("请通过 Tauri 桌面应用连接进程管理器");
  return invoke<T>(command, args);
}

/** 对配置中的命令值加引号。参数：value 为原始值。返回：systemd 转义后的文本。 */
function quote(value: string): string {
  return `"${value.replaceAll("\\", "\\\\").replaceAll('"', '\\"').replaceAll("\n", "\\n").replaceAll("\r", "\\r")}"`;
}

/** 管理桌面页面共享状态和后端操作。参数：无。返回：响应式状态、派生值与操作接口。 */
export function useDesktop() {
  const message = useMessage();
  const dialog = useDialog();
  const state = reactive({
    page: "dashboard" as Page,
    statuses: [] as ProcessStatus[],
    selected: "",
    documents: [] as ConfigDocument[],
    document: "",
    text: "",
    original: null as string | null,
    busy: false,
    refreshing: false,
    connected: false,
    editorLoading: false,
    quitting: false,
    settings: null as Settings | null,
    settingsDraft: { autostart: false, start_hidden: false, refresh_ms: 1000 },
    detailTab: "logs",
    search: "",
    logSource: "",
    records: [] as LogRecord[],
    listCollapsed: false,
    logFullscreen: false,
    logLines: 200,
    logScroll: true,
    logRefresh: 1000,
    logQuery: "",
    logRegex: false,
    logIgnoreCase: true,
    logContext: 0,
    logLoading: false,
    host: null as HostMetrics | null,
    resources: null as ProcessResources | null,
    hostHistory: [] as MetricPoint[],
    processHistory: [] as MetricPoint[],
    metricsError: "",
    healthStatuses: [] as HealthStatus[],
    healthHistory: [] as HealthRecord[],
    healthDraft: {
      kind: "none",
      port: 8080,
      url: "",
      timeout: 1,
      interval: 10,
    } as HealthDraft,
    updated: "",
    createVisible: false,
    dropinVisible: false,
  });
  let timer = 0;
  let logTimer = 0;
  let metricsGeneration = 0;
  let editorGeneration = 0;
  let logGeneration = 0;
  let disposed = false;
  let unlisten: UnlistenFn | undefined;
  const dirty = computed(
    () =>
      !!state.document &&
      (state.original === null || state.text !== state.original),
  );
  const locked = computed(() => state.busy || state.quitting);
  const selectedProcess = computed(() =>
    state.statuses.find((item) => item.name === state.selected),
  );
  const filtered = computed(() =>
    state.statuses.filter((item) =>
      item.name.toLowerCase().includes(state.search.toLowerCase()),
    ),
  );
  const counts = computed(() => ({
    total: state.statuses.length,
    active: state.statuses.filter((item) => item.state === "active").length,
    failed: state.statuses.filter((item) => item.state === "failed").length,
    enabled: state.statuses.filter((item) => item.enabled).length,
  }));
  const selectedHealth = computed(() =>
    state.healthStatuses.find((item) => item.unit === state.selected),
  );

  /** 提示操作错误。参数：error 为错误值。返回：无。 */
  function report(error: unknown): void {
    message.error(String(error), { duration: 9000 });
  }

  /** 等待确认弹窗结果。参数：title 为标题，content 为说明，positive 为确认文字。返回：是否确认。 */
  function confirm(
    title: string,
    content: string,
    positive: string,
  ): Promise<boolean> {
    return new Promise((resolve) => {
      // 所有关闭方式都会完成 Promise；重复关闭不会再次修改业务状态。
      const instance = dialog.warning({
        title,
        content,
        positiveText: positive,
        negativeText: "取消",
        onPositiveClick: () => resolve(true),
        onNegativeClick: () => resolve(false),
        onClose: () => resolve(false),
        onMaskClick: () => resolve(false),
        onEsc: () => resolve(false),
      });
      if (disposed) {
        instance.destroy();
        resolve(false);
      }
    });
  }

  /** 确认编辑切换。参数：无。返回：是否允许放弃当前编辑。 */
  async function mayDiscard(): Promise<boolean> {
    return (
      !dirty.value ||
      (await confirm(
        "放弃未保存的修改？",
        "切换配置会丢失当前未保存的内容。",
        "放弃修改",
      ))
    );
  }

  /** 选择一份配置。参数：item 为文档，fresh 表示未落盘。返回：无。 */
  function selectDocument(item: ConfigDocument, fresh = false): void {
    state.document = item.name;
    state.text = item.text;
    state.original = fresh ? null : item.text;
  }

  /** 切换配置文件并保护未保存内容。参数：name 为文件名。返回：无。 */
  async function changeDocument(name: string): Promise<void> {
    if (
      locked.value ||
      state.editorLoading ||
      name === state.document ||
      !(await mayDiscard())
    )
      return;
    const item = state.documents.find((document) => document.name === name);
    if (item) selectDocument(item, !persistedNames.has(item.name));
  }

  // 新建覆盖文件与磁盘文档分开记录，确保首次保存使用 expected=null。
  const persistedNames = new Set<string>();

  /** 加载配置并忽略旧选择的迟到响应。参数：unit 为进程名称。返回：无。 */
  async function loadDocuments(unit: string): Promise<void> {
    const generation = ++editorGeneration;
    state.editorLoading = true;
    state.document = "";
    state.text = "";
    state.original = null;
    state.documents = [];
    try {
      const documents = await call<ConfigDocument[]>("documents", { unit });
      if (
        disposed ||
        generation !== editorGeneration ||
        state.selected !== unit
      )
        return;
      state.documents = documents;
      persistedNames.clear();
      documents.forEach((item) => persistedNames.add(item.name));
      const item = documents.find((item) => item.name === unit) ?? documents[0];
      if (item) selectDocument(item);
    } catch (error) {
      if (generation === editorGeneration) report(error);
    } finally {
      if (generation === editorGeneration) state.editorLoading = false;
    }
  }

  /** 更新日志并阻止旧进程或旧来源覆盖当前列表。参数：无。返回：无。 */
  async function refreshLogs(): Promise<void> {
    if (
      !state.selected ||
      state.page !== "monitor" ||
      state.detailTab !== "logs"
    )
      return;
    const generation = ++logGeneration;
    const unit = state.selected;
    const source = state.logSource || null;
    const lines = state.logLines;
    state.logLoading = true;
    try {
      const records = await call<LogRecord[]>("logs", { unit, source, lines });
      if (
        !disposed &&
        generation === logGeneration &&
        unit === state.selected &&
        source === (state.logSource || null) &&
        lines === state.logLines
      )
        state.records = records;
    } finally {
      if (generation === logGeneration) state.logLoading = false;
    }
  }

  /** 刷新资源及探测记录。参数：无。返回：无；旧进程响应会被丢弃。
   * 状态、资源和探测各自校验选择，避免页面切换或重启期间展示旧快照。 */
  async function refreshObservability(): Promise<void> {
    if (!isTauri() || disposed) return;
    const unit = state.selected;
    const generation = ++metricsGeneration;
    const wantsResources =
      state.page === "monitor" && state.detailTab === "resources";
    try {
      state.healthStatuses = await call<HealthStatus[]>("health_status");
      if (state.page === "dashboard" || wantsResources) {
        const result = await call<{
          host: HostMetrics;
          process: ProcessResources | null;
          instance: number | null;
        }>("metrics", { unit: wantsResources ? unit : null });
        if (disposed || generation !== metricsGeneration) return;
        state.host = result.host;
        const host = result.host;
        state.hostHistory.push({
          time: host.time,
          cpu: host.cpu,
          memory: (host.memory_used / Math.max(1, host.memory_total)) * 100,
          read: host.read_per_sec,
          write: host.write_per_sec,
        });
        state.hostHistory = state.hostHistory.slice(-120);
        if (
          wantsResources &&
          unit === state.selected &&
          result.instance === selectedProcess.value?.instance
        ) {
          state.resources = result.process;
          const p = result.process;
          if (p)
            state.processHistory.push({
              time: host.time,
              cpu: p.cpu,
              memory: p.memory / 1048576,
              read: p.read_per_sec,
              write: p.write_per_sec,
            });
          state.processHistory = state.processHistory.slice(-120);
        }
      }
      if (unit && state.page === "monitor" && state.detailTab === "health") {
        const records = await call<HealthRecord[]>("health_history", { unit });
        if (!disposed && unit === state.selected) state.healthHistory = records;
      }
      state.metricsError = "";
    } catch (error) {
      state.metricsError = String(error);
    }
  }

  /** 更新日志选项并立即查询。参数：无。返回：无。 */
  async function updateLogs(): Promise<void> {
    if (!isTauri()) return;
    try {
      await refreshLogs();
    } catch (error) {
      report(error);
    }
  }
  /** 使用系统保存对话框下载日志快照。参数：text 为当前筛选结果。返回：无。 */
  async function downloadLogs(text: string): Promise<void> {
    try {
      const path = await call<string | null>("export_logs", {
        unit: state.selected,
        text,
      });
      if (path) message.success(`日志已保存：${path}`);
    } catch (error) {
      report(error);
    }
  }

  /** 刷新进程状态、健康状态及当前页面的资源。参数：无。返回：无。 */
  async function refresh(): Promise<void> {
    if (disposed || state.refreshing || state.quitting || !isTauri()) return;
    state.refreshing = true;
    try {
      const statuses = await call<ProcessStatus[]>("status");
      if (disposed) return;
      state.statuses = statuses;
      state.connected = true;
      state.updated = new Date().toLocaleTimeString("zh-CN", { hour12: false });
      // 无脏编辑时才能自动迁移已经从磁盘移除的选择。
      if (
        !state.selected ||
        (!statuses.some((item) => item.name === state.selected) && !dirty.value)
      ) {
        state.selected = statuses[0]?.name ?? "";
        state.records = [];
        ++logGeneration;
        if (state.selected) await loadDocuments(state.selected);
        else {
          ++editorGeneration;
          state.editorLoading = false;
          state.document = "";
          state.documents = [];
          state.text = "";
        }
      }
      await refreshObservability();
    } catch (error) {
      if (state.connected) report(error);
      state.connected = false;
    } finally {
      state.refreshing = false;
    }
  }

  /** 执行变更并刷新状态。参数：task 为异步操作，success 为成功说明。返回：无。 */
  async function mutate(
    task: () => Promise<unknown>,
    success: string,
  ): Promise<void> {
    if (locked.value) return;
    state.busy = true;
    try {
      await task();
      message.success(success);
      await refresh();
    } catch (error) {
      report(error);
    } finally {
      state.busy = false;
    }
  }

  /** 选择进程并读取配置和日志。参数：unit 为名称，edit 表示切到配置页。返回：无。 */
  async function selectProcess(unit: string, edit = false): Promise<void> {
    if (locked.value) return;
    if (state.selected !== unit) {
      if (!(await mayDiscard())) return;
      state.selected = unit;
      state.records = [];
      state.resources = null;
      state.processHistory = [];
      state.healthHistory = [];
      ++metricsGeneration;
      ++logGeneration;
      await loadDocuments(unit);
    } else if (!state.document) await loadDocuments(unit);
    if (edit) state.page = "config";
    try {
      await refreshLogs();
      await refreshObservability();
    } catch (error) {
      report(error);
    }
  }

  /** 设置日志筛选。参数：source 为来源。返回：无。 */
  async function setLogSource(source: string): Promise<void> {
    state.logSource = source;
    try {
      await refreshLogs();
    } catch (error) {
      report(error);
    }
  }

  /** 切换监控详情标签。参数：tab 为标签标识。返回：无。 */
  async function setDetailTab(tab: string): Promise<void> {
    state.detailTab = tab;
    try {
      await refreshLogs();
      await refreshObservability();
    } catch (error) {
      report(error);
    }
  }

  /** 切换页面并保留编辑草稿。参数：page 为页面。返回：无。 */
  async function navigate(page: Page): Promise<void> {
    state.page = page;
    try {
      await refreshLogs();
      await refreshObservability();
    } catch (error) {
      report(error);
    }
  }

  /** 执行子进程操作。参数：action 为生命周期命令。返回：无。 */
  async function operate(action: string): Promise<void> {
    if (!state.selected) return;
    const unit = state.selected;
    await mutate(() => call("operate", { unit, action }), `${unit} 操作已完成`);
  }

  /** 保存配置并按需重启。参数：restart 表示保存后重启。返回：无。 */
  async function saveConfig(restart: boolean): Promise<void> {
    if (!state.document || state.editorLoading) return;
    const { document: name, text, selected: unit, original: expected } = state;
    await mutate(
      async () => {
        await call("save_document", { name, text, expected });
        state.original = text;
        persistedNames.add(name);
        const item = state.documents.find((item) => item.name === name);
        if (item) item.text = text;
        if (restart) {
          try {
            await call("operate", { unit, action: "restart" });
          } catch (error) {
            throw new Error(`配置已保存，但重启失败：${String(error)}`);
          }
        }
      },
      restart ? "配置已保存，子进程已重启" : "配置已校验并保存，下次启动时应用",
    );
  }

  /** 重载磁盘配置，保留脏编辑。参数：无。返回：无。 */
  async function reload(): Promise<void> {
    await mutate(async () => {
      await call("reload");
      if (state.selected && !dirty.value) await loadDocuments(state.selected);
    }, "磁盘配置已校验并重载");
  }

  /** 生成子进程配置。参数：draft 为新建表单。返回：无。 */
  async function createProcess(draft: ProcessDraft): Promise<void> {
    if (!(await mayDiscard())) return;
    const name = draft.name.endsWith(".service")
      ? draft.name
      : `${draft.name}.service`;
    // 表单只生成基础配置，完整语义及依赖仍由后端统一校验。
    const text = `[Unit]\nDescription=${draft.description.replaceAll("%", "%%")}\n\n[Service]\nType=simple\nExecStart=${quote(draft.executable.replaceAll("%", "%%"))}${draft.args ? ` ${draft.args}` : ""}\n${draft.directory ? `WorkingDirectory=${draft.directory.replaceAll("\\", "/").replaceAll("%", "%%")}\n` : ""}Restart=${draft.restart}\nRestartSec=${draft.delay}\n\n[Install]\nWantedBy=multi-user.target\n`;
    await mutate(async () => {
      await call("save_document", { name, text, expected: null });
      state.createVisible = false;
      state.selected = name;
      state.records = [];
      ++logGeneration;
      await loadDocuments(name);
      state.page = "config";
      if (draft.enabled) {
        try {
          await call("operate", { unit: name, action: "enable" });
        } catch (error) {
          throw new Error(`配置已创建，但启用失败：${String(error)}`);
        }
      }
    }, "子进程配置已创建，可在监控页面启动");
  }

  /** 将健康表单写入当前文档。参数：无。返回：无；保存时由后端校验合并定义。
   * 使用单独 Service 节追加并覆盖标量，保留原有配置和注释。 */
  function applyHealth(): void {
    if (locked.value || state.editorLoading || !state.document) return;
    const h = state.healthDraft;
    if (
      h.kind !== "none" &&
      (!(h.timeout && h.timeout > 0 && h.timeout <= 60) ||
        !(h.interval && h.interval >= 1 && h.interval <= 3600))
    ) {
      report("健康检查超时须为 (0,60] 秒，间隔须为 1～3600 秒");
      return;
    }
    if (
      h.kind === "tcp" &&
      !(h.port && Number.isInteger(h.port) && h.port > 0 && h.port <= 65535)
    ) {
      report("TCP 端口必须为 1～65535");
      return;
    }
    if (h.kind === "http" && !/^https?:\/\//.test(h.url.trim())) {
      report("请输入完整 HTTP/HTTPS URL");
      return;
    }
    const cleaned = state.text
      .split("\n")
      .filter(
        (line) =>
          !/^\s*Health(Type|Port|Url|TimeoutSec|IntervalSec)\s*=/.test(line),
      )
      .join("\n")
      .trimEnd();
    state.text = `${cleaned}\n\n[Service]\nHealthType=${h.kind}\nHealthPort=${h.port ?? 0}\nHealthUrl=${h.url.trim()}\nHealthTimeoutSec=${h.timeout ?? 1}s\nHealthIntervalSec=${h.interval ?? 10}s\n`;
    message.info("健康配置已写入草稿，请保存；运行进程重启后使用新配置");
  }

  /** 从当前文档解析健康表单。参数：text 为编辑正文。返回：无。
   * 只提取 Service 节的标量，最终完整语义由后端解析。 */
  function syncHealthDraft(text: string): void {
    const h: HealthDraft = {
      kind: "none",
      port: 8080,
      url: "",
      timeout: 1,
      interval: 10,
    };
    let section = "";
    for (const line of text.split("\n")) {
      const trimmed = line.trim();
      if (trimmed.startsWith("[")) section = trimmed;
      if (section !== "[Service]") continue;
      const match = trimmed.match(
        /^Health(Type|Port|Url|TimeoutSec|IntervalSec)\s*=(.*)$/,
      );
      if (!match) continue;
      const value = match[2]!.trim();
      if (match[1] === "Type") h.kind = value || "none";
      if (match[1] === "Port") h.port = Number(value) || 8080;
      if (match[1] === "Url") h.url = value;
      if (match[1] === "TimeoutSec") h.timeout = durationSeconds(value, 1);
      if (match[1] === "IntervalSec") h.interval = durationSeconds(value, 10);
    }
    Object.assign(state.healthDraft, h);
  }
  /** 解析表单常用时间值。参数：value 为配置值，fallback 为默认秒数。返回：秒数或空值。 */
  function durationSeconds(value: string, fallback: number): number | null {
    if (!value) return fallback;
    const match = value.match(/^(\d+(?:\.\d+)?)\s*(ms|s|sec|m|min)?$/);
    if (!match) return null;
    return (
      Number(match[1]) *
      (match[2] === "ms"
        ? 0.001
        : ["m", "min"].includes(match[2] ?? "")
          ? 60
          : 1)
    );
  }
  watch(() => state.text, syncHealthDraft);

  /** 安装独立日志轮询。参数：无。返回：无；零间隔为暂停。 */
  function scheduleLogs(): void {
    window.clearInterval(logTimer);
    if (state.logRefresh && !disposed && !state.quitting)
      logTimer = window.setInterval(() => {
        if (!state.logLoading) void updateLogs();
      }, state.logRefresh);
  }
  watch(() => state.logRefresh, scheduleLogs);
  // 实例或 PID 改变时清空资源趋势，防止新旧进程曲线混合。
  watch(
    () =>
      `${state.selected}:${selectedProcess.value?.instance}:${selectedProcess.value?.pid}`,
    () => {
      state.resources = null;
      state.processHistory = [];
    },
    { flush: "sync" },
  );

  /** 新建未落盘的覆盖配置。参数：filename 为普通 .conf 文件名。返回：是否创建成功。 */
  async function createDropin(filename: string): Promise<boolean> {
    if (locked.value || state.editorLoading || !state.selected) return false;
    if (!/^[a-zA-Z0-9_-][a-zA-Z0-9_.-]*\.conf$/.test(filename)) {
      report("请输入普通的 .conf 文件名");
      return false;
    }
    if (!(await mayDiscard())) return false;
    const name = `${state.selected}.d/${filename}`;
    if (state.documents.some((item) => item.name === name)) {
      report("该覆盖文件已存在，请从列表选择");
      return false;
    }
    const item = {
      name,
      text: "# 本地覆盖配置\n[Service]\nRestart=on-failure\n",
    };
    state.documents.push(item);
    selectDocument(item, true);
    state.dropinVisible = false;
    return true;
  }

  /** 根据已保存偏好安装轮询。参数：无。返回：无。 */
  function scheduleRefresh(): void {
    window.clearInterval(timer);
    if (!disposed && !state.quitting)
      timer = window.setInterval(
        refresh,
        state.settings?.preferences.refresh_ms ?? 1000,
      );
  }

  /** 保存桌面启动设置。参数：无。返回：无。 */
  async function saveSettings(): Promise<void> {
    await mutate(async () => {
      const { autostart, start_hidden, refresh_ms } = state.settingsDraft;
      await call("save_settings", {
        autostart,
        preferences: { start_hidden, refresh_ms },
      });
      state.settings = await call<Settings>("settings");
      scheduleRefresh();
    }, "应用设置已保存");
  }

  /** 隐藏到托盘。参数：无。返回：无。 */
  async function hide(): Promise<void> {
    try {
      await call("hide_window");
    } catch (error) {
      report(error);
    }
  }

  /** 确认并退出桌面管理器。参数：无。返回：无。 */
  async function quit(): Promise<void> {
    if (
      locked.value ||
      !(await confirm(
        "退出 rpmm？",
        `${dirty.value ? "当前未保存的编辑将丢失。" : ""}所有托管子进程将按依赖顺序停止，完成后自动退出。`,
        "停止子进程并退出",
      ))
    )
      return;
    try {
      await call("quit");
    } catch (error) {
      report(error);
    }
  }

  /** 初始化后端设置与事件订阅。参数：无。返回：无。 */
  async function initialize(): Promise<void> {
    if (!isTauri()) return;
    try {
      unlisten = await listen("quitting", () => {
        state.quitting = true;
        window.clearInterval(timer);
        window.clearInterval(logTimer);
        message.info("正在停止所有子进程，完成后自动退出", { duration: 0 });
      });
      if (disposed) {
        unlisten();
        return;
      }
      state.settings = await call<Settings>("settings");
      Object.assign(state.settingsDraft, state.settings.preferences, {
        autostart: state.settings.autostart,
      });
      await refresh();
      await updateLogs();
    } catch (error) {
      report(error);
    } finally {
      scheduleRefresh();
      scheduleLogs();
    }
  }

  /** 释放定时器、事件和迟到请求。参数：无。返回：无。 */
  function dispose(): void {
    disposed = true;
    ++editorGeneration;
    ++logGeneration;
    ++metricsGeneration;
    window.clearInterval(timer);
    window.clearInterval(logTimer);
    unlisten?.();
  }
  onMounted(initialize);
  onUnmounted(dispose);
  return {
    state,
    dirty,
    locked,
    selectedProcess,
    filtered,
    counts,
    selectedHealth,
    updateLogs,
    downloadLogs,
    applyHealth,
    preview: !isTauri(),
    refresh,
    selectProcess,
    changeDocument,
    setLogSource,
    setDetailTab,
    navigate,
    operate,
    saveConfig,
    reload,
    createProcess,
    createDropin,
    saveSettings,
    hide,
    quit,
  };
}
