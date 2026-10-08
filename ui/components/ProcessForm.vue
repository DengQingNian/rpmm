<script setup lang="ts">
import { reactive, ref, watch } from "vue";
import {
  NButton,
  NCheckbox,
  NForm,
  NFormItem,
  NInput,
  NSelect,
  type FormInst,
  type FormRules,
} from "naive-ui";
import type { ProcessDraft } from "../types";
import { serviceDefaults } from "../serviceConfig";
import ServiceOptionsForm from "./ServiceOptionsForm.vue";
const props = defineProps<{ busy: boolean; visible: boolean; services: string[] }>();
const emit = defineEmits<{ create: [draft: ProcessDraft]; cancel: [] }>();
const form = ref<FormInst | null>(null);
const shaking = ref(false);
const draft = reactive<ProcessDraft>(emptyDraft());
const restartOptions = [
  { label: "失败时重启", value: "on-failure" },
  { label: "始终重启", value: "always" },
  { label: "不自动重启", value: "no" },
  { label: "正常退出时重启", value: "on-success" },
];
const rules: FormRules = {
  name: [
    { required: true, message: "请输入进程名称", trigger: ["blur", "input"] },
    {
      pattern: /^[a-zA-Z0-9_.-]+$/,
      message: "名称仅支持英文、数字、点、下划线和连字符",
      trigger: "input",
    },
  ],
  executable: [
    { required: true, message: "请输入可执行文件路径", trigger: "blur" },
    {
      pattern: /^(?:[a-zA-Z]:[\\/]|\\\\).+/,
      message: "请使用 Windows 绝对路径",
      trigger: "blur",
    },
  ],
  delay: { required: true, message: "请输入重启等待时间", trigger: "blur" },
};
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
  if (visible) {
    Object.assign(draft, emptyDraft());
    form.value?.restoreValidation();
    shaking.value = false;
  }
}
watch(() => props.visible, resetOnOpen);
/** 校验并提交新建表单。参数：无。返回：无。 */
async function submit(): Promise<void> {
  if (props.busy) return;
  try {
    await form.value?.validate();
  } catch {
    shaking.value = false;
    requestAnimationFrame(() => {
      shaking.value = true;
    });
    return;
  }
  emit("create", {
    ...draft,
    name: draft.name.trim(),
    executable: draft.executable.trim(),
    directory: draft.directory.trim(),
  });
}
</script>
<template>
  <NForm
    ref="form"
    :model="draft"
    :rules="rules"
    :disabled="busy"
    :class="{ shake: shaking }"
    @submit.prevent="submit"
    @animationend="shaking = false"
  >
    <NFormItem label="进程名称" path="name"
      ><NInput v-model:value="draft.name" placeholder="例如 api 或 api.service"
    /></NFormItem>
    <NFormItem label="说明"
      ><NInput
        v-model:value="draft.description"
        placeholder="例如 本地 API 服务"
    /></NFormItem>
    <NFormItem label="可执行文件" path="executable"
      ><NInput v-model:value="draft.executable" placeholder="C:/Apps/api.exe"
    /></NFormItem>
    <NFormItem label="命令参数"
      ><NInput
        v-model:value="draft.args"
        placeholder="--port 8080（含空格的参数使用双引号）"
    /></NFormItem>
    <NFormItem label="工作目录"
      ><NInput v-model:value="draft.directory" placeholder="C:/Apps（可选）"
    /></NFormItem>
    <div class="form-columns">
      <NFormItem label="重启策略"
        ><NSelect
          v-model:value="draft.restart"
          :options="restartOptions" /></NFormItem
      ><NFormItem label="重启等待" path="delay"
        ><NInput v-model:value="draft.delay"
      /></NFormItem>
    </div>
    <ServiceOptionsForm :draft="draft" :services="services" :current="draft.name" :disabled="busy" />
    <NCheckbox v-model:checked="draft.enabled">随 rpmm 启动此子进程</NCheckbox>
    <p class="form-note">创建后不会立即启动，可在监控页面点击启动。</p>
    <div class="dialog-actions">
      <NButton :disabled="busy" @click="emit('cancel')">取消</NButton
      ><NButton type="primary" attr-type="submit" :loading="busy"
        >创建配置</NButton
      >
    </div>
  </NForm>
</template>
