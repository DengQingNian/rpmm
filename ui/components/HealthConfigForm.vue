<script setup lang="ts">
import type { useDesktop } from "../composables/useDesktop";
import MdButton from "./MdButton.vue";
import MdSelect from "./MdSelect.vue";
import MdTextField from "./MdTextField.vue";

/** 健康检查配置表单：读取当前文档中的健康指令，并把表单写回草稿。 */
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();
const draft = props.desktop.state.healthDraft;

const kindOptions = [
  { label: "关闭", value: "none" },
  { label: "TCP 端口探测", value: "tcp" },
  { label: "HTTP 状态码探测", value: "http" },
];

/** 数值输入框的字符串模型。参数：value 为数值。返回：字符串，空值映射为空串。 */
function text(value: number | null): string {
  return value == null ? "" : String(value);
}

/** 把字符串解析为浮点数。参数：value 为输入文本。返回：数值或 null。 */
function number(value: string): number | null {
  const parsed = Number(value.trim());
  return Number.isFinite(parsed) && value.trim() !== "" ? parsed : null;
}
</script>

<template>
  <div class="health-config-form">
    <div class="field-stack">
      <span class="field-stack__label">检查类型</span>
      <MdSelect
        v-model="draft.kind"
        label="健康检查类型"
        :options="kindOptions"
        width="220px"
        :disabled="desktop.locked.value"
      />
    </div>
    <MdTextField
      v-if="draft.kind === 'tcp'"
      :model-value="text(draft.port)"
      label="本机 TCP 端口"
      placeholder="1～65535"
      inputmode="numeric"
      :disabled="desktop.locked.value"
      @update:model-value="draft.port = number($event)"
    />
    <MdTextField
      v-if="draft.kind === 'http'"
      :model-value="draft.url"
      label="HTTP / HTTPS URL（仅状态 200 成功）"
      placeholder="http://127.0.0.1:8080/health"
      :disabled="desktop.locked.value"
      mono
      @update:model-value="draft.url = $event"
    />
    <div v-if="draft.kind !== 'none'" class="form-grid form-grid--two">
      <MdTextField
        :model-value="text(draft.timeout)"
        label="超时（秒）"
        placeholder="1"
        inputmode="decimal"
        :disabled="desktop.locked.value"
        @update:model-value="draft.timeout = number($event)"
      />
      <MdTextField
        :model-value="text(draft.interval)"
        label="检查间隔（秒）"
        placeholder="10"
        inputmode="decimal"
        :disabled="desktop.locked.value"
        @update:model-value="draft.interval = number($event)"
      />
    </div>
    <p class="form-note">
      编辑当前文件的健康指令；覆盖文件按名称顺序合并。后台记录每次结果，不触发自动重启。保存并重启后应用到运行实例。
    </p>
    <MdButton
      variant="filled"
      icon="check"
      :disabled="desktop.locked.value || desktop.state.editorLoading || !desktop.state.document"
      @click="desktop.applyHealth()"
      >写入配置草稿</MdButton
    >
  </div>
</template>
