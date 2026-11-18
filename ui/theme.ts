/**
 * 主题切换：默认跟随系统深浅色，用户可在标题栏手动指定。
 * 偏好保存在 localStorage 的 rpmm.theme，取值 auto/light/dark。
 */
import { computed, ref } from "vue";

export type ThemeMode = "auto" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

const STORAGE_KEY = "rpmm.theme";
const media =
  typeof window === "undefined"
    ? null
    : window.matchMedia("(prefers-color-scheme: dark)");

/** 读取已保存的主题偏好。参数：无。返回：合法的主题模式，未保存时为 auto。 */
function readMode(): ThemeMode {
  const saved = window.localStorage.getItem(STORAGE_KEY);
  return saved === "light" || saved === "dark" || saved === "auto"
    ? saved
    : "auto";
}

/** 计算系统当前的深浅色。参数：无。返回：深色为 dark，否则为 light。 */
function systemTheme(): ResolvedTheme {
  return media?.matches ? "dark" : "light";
}

const mode = ref<ThemeMode>(readMode());
const system = ref<ResolvedTheme>(systemTheme());

/** 把主题写到根元素。参数：resolved 为实际生效的主题。返回：无。 */
function apply(resolved: ResolvedTheme): void {
  const root = document.documentElement;
  root.dataset.theme = resolved;
  root.style.colorScheme = resolved;
}

const resolved = computed<ResolvedTheme>(() =>
  mode.value === "auto" ? system.value : mode.value,
);

/** 切换主题模式并持久化。参数：next 为主题模式。返回：无。 */
function setMode(next: ThemeMode): void {
  mode.value = next;
  window.localStorage.setItem(STORAGE_KEY, next);
  apply(resolved.value);
}

/** 在浅色、深色、跟随系统之间循环。参数：无。返回：切换后的模式。 */
function cycle(): ThemeMode {
  const order: ThemeMode[] = ["auto", "light", "dark"];
  const next = order[(order.indexOf(mode.value) + 1) % order.length]!;
  setMode(next);
  return next;
}

/** 订阅系统主题变化。参数：无。返回：无。挂载时调用一次。 */
function start(): void {
  apply(resolved.value);
  media?.addEventListener("change", () => {
    system.value = systemTheme();
    if (mode.value === "auto") apply(system.value);
  });
}

/** 主题状态与操作。参数：无（保持同一实例）。返回：只读状态与 setMode/cycle。 */
export function useTheme() {
  return { mode, resolved, setMode, cycle };
}

start();
