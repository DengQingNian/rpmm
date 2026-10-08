/** 后端返回的子进程状态快照。 */
export interface ProcessStatus {
  name: string;
  state: string;
  substate: string;
  pid: number | null;
  instance: number;
  exit_code: number | null;
  reason: string | null;
  restart_count: number;
  config_version: number;
  enabled: boolean;
}
/** 配置正文及其相对于 units 的文件名。 */
export interface ConfigDocument {
  name: string;
  text: string;
}
/** 本地界面偏好。 */
export interface Preferences {
  start_hidden: boolean;
  refresh_ms: number;
}
/** 桌面设置及数据根目录。 */
export interface Settings {
  root: string;
  next_root: string;
  preferences: Preferences;
  autostart: boolean;
}
/** 按来源分类的日志记录。 */
export interface LogRecord {
  time: string;
  source: string;
  text: string;
  instance: number;
}
/** 新建子进程表单。 */
export interface ServiceDraft {
  after: string[];
  before: string[];
  requires: string[];
  wants: string[];
  healthAfter: string[];
  stdoutDirectory: string;
  memoryMax: number | null;
  cpuQuota: number | null;
}
/** 新建子进程表单及关联、资源限制。 */
export interface ProcessDraft extends ServiceDraft {
  name: string;
  description: string;
  executable: string;
  args: string;
  directory: string;
  restart: string;
  delay: string;
  enabled: boolean;
}
export type Page = "dashboard" | "monitor" | "config" | "settings";
/** 健康检查配置表单。 */
export interface HealthDraft {
  kind: string;
  port: number | null;
  url: string;
  timeout: number | null;
  interval: number | null;
}
/** 健康检查结果和后台配置。 */
export interface HealthRecord {
  time: string;
  instance: number;
  healthy: boolean;
  latency_ms: number;
  detail: string;
}
export interface HealthStatus {
  unit: string;
  config: {
    kind: string;
    port: number;
    url: string;
    timeout: { secs: number; nanos: number };
    interval: { secs: number; nanos: number };
  };
  latest: HealthRecord | null;
}
/** 宿主机资源快照，首次采样的速率为空。 */
export interface HostMetrics {
  time: string;
  name: string;
  cpu: number | null;
  memory_used: number;
  memory_total: number;
  read_per_sec: number | null;
  write_per_sec: number | null;
  disks: {
    mount: string;
    total: number;
    available: number;
    read_per_sec: number;
    write_per_sec: number;
  }[];
}
/** 托管主进程资源和连接明细。 */
export interface ProcessResources {
  pid: number;
  start_time: number;
  cpu: number | null;
  memory: number;
  virtual_memory: number;
  read_total: number;
  write_total: number;
  read_per_sec: number | null;
  write_per_sec: number | null;
  open_files: number | null;
  environment: Record<string, string>;
  environment_source: string;
  connections: {
    protocol: string;
    local: string;
    remote: string;
    state: string;
  }[];
  connection_error: string | null;
}
/** 图表的单次采样。 */
export interface MetricPoint {
  time: string;
  cpu: number | null;
  memory: number;
  read: number | null;
  write: number | null;
}
export const stateLabels: Record<string, string> = {
  active: "运行中",
  inactive: "已停止",
  activating: "启动中",
  deactivating: "停止中",
  failed: "失败",
  stdout: "标准输出",
  stderr: "错误输出",
  manager: "生命周期",
  health: "健康检查",
};
/** 获取状态中文名称。参数：state 为状态或日志来源。返回：中文名称或原值。 */
export function label(state: string): string {
  return stateLabels[state] ?? state;
}
/** 匹配状态标签颜色。参数：state 为进程状态。返回：Naive UI 标签类型。 */
export function statusType(
  state: string,
): "success" | "error" | "warning" | "default" {
  // 中间状态统一使用警示色，未知状态保留中性色。
  if (state === "active") return "success";
  if (state === "failed") return "error";
  if (["activating", "deactivating"].includes(state)) return "warning";
  return "default";
}
