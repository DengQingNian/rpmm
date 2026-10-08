<script setup lang="ts">
import { NButton, NFormItem, NInput, NInputNumber, NSelect } from "naive-ui";
import type { useDesktop } from "../composables/useDesktop";
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();
const draft = props.desktop.state.healthDraft;
const options = [
  { label: "关闭", value: "none" },
  { label: "TCP", value: "tcp" },
  { label: "HTTP", value: "http" },
];
</script>
<template>
  <div class="health-config-form">
    <NFormItem label="检查类型"
      ><NSelect
        v-model:value="draft.kind"
        :options="options"
        :disabled="desktop.locked.value" /></NFormItem
    ><NFormItem v-if="draft.kind === 'tcp'" label="本机 TCP 端口"
      ><NInputNumber
        v-model:value="draft.port"
        :min="1"
        :max="65535"
        :precision="0"
        :disabled="desktop.locked.value" /></NFormItem
    ><NFormItem
      v-if="draft.kind === 'http'"
      label="HTTP / HTTPS URL（仅状态 200 成功）"
      ><NInput
        v-model:value="draft.url"
        placeholder="http://127.0.0.1:8080/health"
        :disabled="desktop.locked.value"
    /></NFormItem>
    <div v-if="draft.kind !== 'none'" class="form-columns">
      <NFormItem label="超时（秒）"
        ><NInputNumber
          v-model:value="draft.timeout"
          :min="0.001"
          :max="60"
          :disabled="desktop.locked.value" /></NFormItem
      ><NFormItem label="检查间隔（秒）"
        ><NInputNumber
          v-model:value="draft.interval"
          :min="1"
          :max="3600"
          :disabled="desktop.locked.value"
      /></NFormItem>
    </div>
    <p class="form-note">
      编辑当前文件的健康指令；覆盖文件按名称顺序合并。后台记录每次结果，不触发自动重启。保存并重启后应用到运行实例。
    </p>
    <NButton
      :disabled="
        desktop.locked.value ||
        desktop.state.editorLoading ||
        !desktop.state.document
      "
      @click="desktop.applyHealth"
      >写入配置草稿</NButton
    >
  </div>
</template>
