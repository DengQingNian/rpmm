<script setup lang="ts">
import { reactive, ref, watch } from "vue";
import { isWindowsPath, windowsPath } from "../paths";
import { serviceDefaults } from "../serviceConfig";
import type { ProcessDraft } from "../types";
import MdCheckbox from "./MdCheckbox.vue";
import MdSelect from "./MdSelect.vue";
import MdTextField from "./MdTextField.vue";
import ServiceOptionsForm from "./ServiceOptionsForm.vue";

/** 新建子进程表单；提交与取消交给外层对话框。 */
const props = defineProps<{ busy: boolean; visible: boolean; services: string[] }>();
const emit = defineEmits<{ create: [draft: ProcessDraft]; cancel: [] }>();

const fields = ref<HTMLElement | null>(null);
const shaking = ref(false);
const draft = reactive<ProcessDraft>(emptyDraft());

const restartOptions = [
  { label: "失败时重启", value: "on-failure" },
  { label: "始终重启", value: "always" },
  { label: "不自动重启", value: "no" },
  { label: "正常退出时重启", value: "on-success" },
];

/** 创建初始表单。参数：无。返回：默认进程草稿。 */
function emptyDraft(): ProcessDraft {
  return {
    ...serviceDefaults(),
    name: "",
    description: "",
    executable: "",
    args: "",
    directory: "",
    restart: "on-failure",
    delay: "2s",
    enabled: true,
  };
}

/** 新打开弹窗时清空旧表单。参数：visible 为可见性。返回：无。 */
function resetOnOpen(visible: boolean): void {
  if (!visible) return;
  Object.assign(draft, emptyDraft());
  shaking.value = false;
}
watch(() => props.visible, resetOnOpen);

/** 归一化表单里的路径。参数：value 为用户输入。返回：规范 Windows 路径；无法识别时原样交给后端报错。 */
function path(value: string): string {
  return windowsPath(value) ?? value.trim();
}

/** 校验各字段，返回首个错误所在的字段名。参数：无。返回：错误键与说明。 */
function validate(): { key: string; message: string } | null {
  const name = draft.name.trim();
  if (!name) return { key: "name", message: "请输入进程名称" };
  if (!/^[a-zA-Z0-9_.-]+$/.test(name))
    return { key: "name", message: "名称仅支持英文、数字、点、下划线和连字符" };
  const executable = draft.executable.trim();
  if (!executable) return { key: "executable", message: "请输入可执行文件路径" };
  if (!isWindowsPath(executable))
    return {
      key: "executable",
      message: "请输入 Windows 绝对路径，例如 C:\Apps\api.exe、C:/Apps/api.exe 或 /c/Apps/api.exe",
    };
  if (draft.directory.trim() && !isWindowsPath(draft.directory))
    return { key: "directory", message: "请输入 Windows 绝对路径，例如 D:\Logs 或 D:/Logs" };
  if (!draft.delay.trim()) return { key: "delay", message: "请输入重启等待时间" };
  return null;
}

const errors = reactive<Record<string, string>>({});

/** 校验并提交新建表单。参数：无。返回：无；失败时标红字段并抖动对话框。 */
async function submit(): Promise<void> {
  if (props.busy) return;
  for (const key of Object.keys(errors)) delete errors[key];
  const problem = validate();
  if (problem) {
    errors[problem.key] = problem.message;
    shaking.value = false;
    requestAnimationFrame(() => {
      shaking.value = true;
    });
    fields.value?.querySelector<HTMLElement>('[aria-invalid="true"], input')?.focus();
    return;
  }
  emit("create", {
    ...draft,
    name: draft.name.trim(),
    executable: path(draft.executable),
    directory: draft.directory.trim() ? path(draft.directory) : "",
  });
}

defineExpose({ submit });
</script>

<template>
  <form
    ref="fields"
    class="process-form"
    :class="{ shake: shaking }"
    @submit.prevent="submit"
    @animationend="shaking = false"
  >
    <section class="form-section">
      <h3 class="form-section__title">基本信息</h3>
      <div class="form-grid form-grid--two">
        <MdTextField
          v-model="draft.name"
          label="进程名称"
          placeholder="例如 api 或 api.service"
          :error="errors.name"
          :disabled="busy"
          mono
          :maxlength="64"
        />
        <MdTextField
          v-model="draft.description"
          label="说明"
          placeholder="例如 本地 API 服务"
          :disabled="busy"
        />
      </div>
      <MdTextField
        v-model="draft.executable"
        label="可执行文件"
        placeholder="C:\Apps\api.exe 或 /c/Apps/api.exe"
        :error="errors.executable"
        :disabled="busy"
        mono
      />
      <MdTextField
        v-model="draft.args"
        label="命令参数"
        placeholder="--port 8080（含空格的参数使用双引号）"
        :disabled="busy"
        mono
      />
      <MdTextField
        v-model="draft.directory"
        label="工作目录"
        placeholder="D:\Logs（可选）"
        :error="errors.directory"
        :disabled="busy"
        mono
      />
    </section>

    <section class="form-section">
      <h3 class="form-section__title">重启策略</h3>
      <div class="form-grid form-grid--two">
        <label class="field-stack">
          <span class="field-stack__label">重启策略</span>
          <MdSelect
            v-model="draft.restart"
            label="重启策略"
            :options="restartOptions"
            :disabled="busy"
          />
        </label>
        <MdTextField
          v-model="draft.delay"
          label="重启等待"
          placeholder="例如 2s"
          :error="errors.delay"
          :disabled="busy"
          mono
        />
      </div>
    </section>

    <section class="form-section">
      <h3 class="form-section__title">依赖与运行选项</h3>
      <ServiceOptionsForm
        :draft="draft"
        :services="services"
        :current="draft.name"
        :disabled="busy"
      />
    </section>

    <MdCheckbox v-model="draft.enabled" label="随 rpmm 启动此子进程" :disabled="busy" />
    <p class="form-note">创建后不会立即启动，可在监控页面点击启动。</p>
  </form>
</template>
