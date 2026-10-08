<script setup lang="ts">
import { computed } from "vue";
import { NFormItem, NInput, NInputNumber, NSelect } from "naive-ui";
import type { ServiceDraft } from "../types";
const props = defineProps<{ draft: ServiceDraft; services: string[]; current?: string; disabled?: boolean }>();
const options = computed(() => props.services.filter((name) => name !== props.current && name !== `${props.current}.service`).map((name) => ({ label: name, value: name })));
</script>
<template>
  <NFormItem label="前置服务（先启动它们，再启动当前服务）">
    <NSelect v-model:value="draft.after" multiple :options="options" :disabled="disabled" placeholder="选择前置服务" />
  </NFormItem>
  <NFormItem label="等待健康的前置服务（可选）">
    <NSelect v-model:value="draft.healthAfter" multiple :options="options" :disabled="disabled" placeholder="留空只参考启动状态；所选服务须配置健康检查" />
  </NFormItem>
  <NFormItem label="后置服务（同一事务中，当前服务先于它们启动）">
    <NSelect v-model:value="draft.before" multiple :options="options" :disabled="disabled" placeholder="选择后置服务" />
  </NFormItem>
  <div class="form-columns">
    <NFormItem label="其他强依赖"><NSelect v-model:value="draft.requires" multiple :options="options" :disabled="disabled" placeholder="同时启动" /></NFormItem>
    <NFormItem label="其他弱依赖"><NSelect v-model:value="draft.wants" multiple :options="options" :disabled="disabled" placeholder="失败不阻止当前服务" /></NFormItem>
  </div>
  <p class="form-note">前置和健康前置服务会自动拉入启动事务；后置关系只排序事务中已有服务。健康等待受当前服务 TimeoutStartSec 约束，默认 90 秒。</p>
  <NFormItem label="标准输出目录"><NInput v-model:value="draft.stdoutDirectory" :disabled="disabled" placeholder="例如 D:/service-output，留空仅使用运行日志" /></NFormItem>
  <div class="form-columns">
    <NFormItem label="进程树内存上限（MiB）"><NInputNumber v-model:value="draft.memoryMax" :disabled="disabled" :min="1" :precision="0" placeholder="不限" clearable /></NFormItem>
    <NFormItem label="进程树 CPU 上限（整机 %）"><NInputNumber v-model:value="draft.cpuQuota" :disabled="disabled" :min="1" :max="100" :precision="0" placeholder="不限" clearable /></NFormItem>
  </div>
  <p class="form-note">输出追加到 服务名.stdout.log，文件不自动轮转。内存约束整个进程树的提交内存；额度耗尽后新分配会失败。CPU 使用 Windows 硬额度；修改后重启服务生效。</p>
</template>
