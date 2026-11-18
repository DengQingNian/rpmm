<script setup lang="ts">
import { computed } from "vue";
import type { ServiceDraft } from "../types";
import MdChipSet from "./MdChipSet.vue";
import MdTextField from "./MdTextField.vue";

/** 关联服务与运行选项表单，供新建表单与配置页共用。 */
const props = defineProps<{
  draft: ServiceDraft;
  services: string[];
  current?: string;
  disabled?: boolean;
}>();

/** 可选择的关联目标，排除自身。参数：无。返回：选项列表。 */
const options = computed(() =>
  props.services
    .filter((name) => name !== props.current && name !== props.current + ".service")
    .map((name) => ({ value: name, label: name })),
);

/** 数值输入框的字符串模型：空串表示不限。参数：value 为数值。返回：字符串。 */
function text(value: number | null): string {
  return value == null ? "" : String(value);
}

/** 把字符串还原为数值；非法或空值记为不限。参数：value 为输入文本。返回：正整数或 null。 */
function number(value: string): number | null {
  const parsed = Number(value.trim());
  return Number.isInteger(parsed) && parsed > 0 ? parsed : null;
}
</script>

<template>
  <div class="service-options">
    <div class="field-stack">
      <span class="field-stack__label">前置服务（先启动它们，再启动当前服务）</span>
      <MdChipSet
        :model-value="draft.after"
        :options="options"
        label="前置服务"
        :disabled="disabled"
        empty-text="还没有其他服务"
        @update:model-value="draft.after = $event"
      />
    </div>
    <div class="field-stack">
      <span class="field-stack__label">等待健康的前置服务（可选）</span>
      <MdChipSet
        :model-value="draft.healthAfter"
        :options="options"
        label="等待健康的前置服务"
        :disabled="disabled"
        empty-text="还没有其他服务"
        @update:model-value="draft.healthAfter = $event"
      />
      <p class="form-note">留空只参考启动状态；所选服务须配置健康检查。</p>
    </div>
    <div class="field-stack">
      <span class="field-stack__label">后置服务（同一事务中，当前服务先于它们启动）</span>
      <MdChipSet
        :model-value="draft.before"
        :options="options"
        label="后置服务"
        :disabled="disabled"
        empty-text="还没有其他服务"
        @update:model-value="draft.before = $event"
      />
    </div>
    <div class="field-stack">
      <span class="field-stack__label">其他强依赖</span>
      <MdChipSet
        :model-value="draft.requires"
        :options="options"
        label="其他强依赖"
        :disabled="disabled"
        empty-text="还没有其他服务"
        @update:model-value="draft.requires = $event"
      />
    </div>
    <div class="field-stack">
      <span class="field-stack__label">其他弱依赖（失败不阻止当前服务）</span>
      <MdChipSet
        :model-value="draft.wants"
        :options="options"
        label="其他弱依赖"
        :disabled="disabled"
        empty-text="还没有其他服务"
        @update:model-value="draft.wants = $event"
      />
    </div>
    <p class="form-note">
      前置和健康前置服务会自动拉入启动事务；后置关系只排序事务中已有服务。健康等待受当前服务
      TimeoutStartSec 约束，默认 90 秒。
    </p>
    <MdTextField
      :model-value="draft.stdoutDirectory"
      label="标准输出目录"
      placeholder="例如 D:/service-output，留空仅使用运行日志"
      :disabled="disabled"
      mono
      @update:model-value="draft.stdoutDirectory = $event"
    />
    <div class="form-grid form-grid--two">
      <MdTextField
        :model-value="text(draft.memoryMax)"
        label="进程树内存上限（MiB）"
        placeholder="不限"
        :disabled="disabled"
        inputmode="numeric"
        @update:model-value="draft.memoryMax = number($event)"
      />
      <MdTextField
        :model-value="text(draft.cpuQuota)"
        label="进程树 CPU 上限（整机 %）"
        placeholder="不限"
        :disabled="disabled"
        inputmode="numeric"
        @update:model-value="draft.cpuQuota = number($event)"
      />
    </div>
    <p class="form-note">
      输出追加到 服务名.stdout.log，文件不自动轮转。内存约束整个进程树的提交内存；额度耗尽后新分配会失败。CPU
      使用 Windows 硬额度；修改后重启服务生效。
    </p>
  </div>
</template>
