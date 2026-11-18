import { reactive } from "vue";

/** 提示的类型，对应 Material 的状态色。 */
export type ToastKind = "info" | "success" | "warning" | "error";

/** 一条底部提示。 */
export interface ToastItem {
  id: number;
  kind: ToastKind;
  text: string;
  /** 常驻提示不自动消失，例如“正在停止所有子进程”。 */
  sticky: boolean;
}

/** 一个待确认的对话请求。 */
export interface ConfirmRequest {
  title: string;
  message: string;
  confirmLabel: string;
  cancelLabel: string;
  /** 危险操作使用错误色填充按钮。 */
  destructive: boolean;
  resolve: (value: boolean) => void;
}

let sequence = 0;

/** 页面共享的提示与确认状态，由 App.vue 统一渲染。 */
export const feedback = reactive({
  toasts: [] as ToastItem[],
  queue: [] as ConfirmRequest[],
});

/** 当前展示的确认请求。参数：无。返回：队首请求或 undefined。 */
export function currentConfirm(): ConfirmRequest | undefined {
  return feedback.queue[0];
}

/** 关闭队首确认并返回结果。参数：value 为是否确认。返回：无。 */
export function settleConfirm(value: boolean): void {
  const request = feedback.queue.shift();
  request?.resolve(value);
}

/** 移除提示。参数：id 为提示编号。返回：无。 */
export function dismissToast(id: number): void {
  const index = feedback.toasts.findIndex((item) => item.id === id);
  if (index >= 0) feedback.toasts.splice(index, 1);
}

/** 显示提示并在 4 秒后自动消失；同内容提示不会重复堆叠。参数：options 为提示内容与类型。返回：无。 */
function show({
  kind,
  text,
  sticky = false,
}: {
  kind: ToastKind;
  text: string;
  sticky?: boolean;
}): void {
  const same = feedback.toasts.find((item) => item.text === text);
  if (same) {
    dismissToast(same.id);
  }
  const id = ++sequence;
  feedback.toasts.push({ id, kind, text, sticky });
  // 最多同时保留 3 条，避免提示遮住日志区域。
  while (feedback.toasts.length > 3) feedback.toasts.shift();
  if (!sticky)
    window.setTimeout(() => dismissToast(id), 4000);
}

/** 统一提示入口：既可直接调用 ui.toast({kind, text})，也可按类型调用。 */
export const ui = {
  toast: Object.assign(show, {
    /** 普通信息提示。参数：text 为文案。返回：无。 */
    info: (text: string, options: { sticky?: boolean } = {}) =>
      show({ kind: "info", text, ...options }),
    /** 成功提示。参数：text 为文案。返回：无。 */
    success: (text: string, options: { sticky?: boolean } = {}) =>
      show({ kind: "success", text, ...options }),
    /** 警示提示。参数：text 为文案。返回：无。 */
    warning: (text: string, options: { sticky?: boolean } = {}) =>
      show({ kind: "warning", text, ...options }),
    /** 错误提示。参数：text 为文案。返回：无。 */
    error: (text: string, options: { sticky?: boolean } = {}) =>
      show({ kind: "error", text, ...options }),
  }),
  /** 弹出确认对话。参数：options 为标题、说明与按钮文案。返回：用户是否确认。 */
  confirm(options: Omit<ConfirmRequest, "resolve">): Promise<boolean> {
    return new Promise((resolve) => {
      feedback.queue.push({ ...options, resolve });
    });
  },
};
