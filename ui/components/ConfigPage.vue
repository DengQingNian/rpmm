<script setup lang="ts">
import {
  NButton,
  NCollapse,
  NCollapseItem,
  NEmpty,
  NInput,
  NSelect,
  NSpin,
} from "naive-ui";
import { computed } from "vue";
import type { useDesktop } from "../composables/useDesktop";
import AppIcon from "./AppIcon.vue";
import HealthConfigForm from "./HealthConfigForm.vue";
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();
const options = computed(() =>
  props.desktop.state.documents.map((item) => ({
    label: item.name,
    value: item.name,
  })),
);
const example = `[Unit]\nDescription=应用说明\nRequires=database.service\nAfter=database.service\n\n[Service]\nType=simple\nExecStart="C:/Apps/app.exe" --port 8080\nWorkingDirectory=C:/Apps\nEnvironment="PORT=8080"\nRestart=on-failure\nRestartSec=2s\nTimeoutStopSec=30s\nHealthType=http\nHealthUrl=http://127.0.0.1:8080/health\nHealthTimeoutSec=1s\nHealthIntervalSec=10s\n\n[Install]\nWantedBy=multi-user.target`;
</script>
<template>
  <div class="config-workspace">
    <section class="paper config-list">
      <div class="panel-heading">
        <h2>配置目录</h2>
        <AppIcon name="folder" />
      </div>
      <button
        v-for="process in desktop.state.statuses"
        :key="process.name"
        class="config-unit"
        :class="{ selected: process.name === desktop.state.selected }"
        :disabled="desktop.locked.value"
        @click="desktop.selectProcess(process.name)"
      >
        <AppIcon name="file" :size="16" />{{ process.name }}</button
      ><NEmpty
        v-if="!desktop.state.statuses.length"
        description="暂无子进程配置"
        class="empty"
      />
      <div class="config-guide">
        <strong class="pencil-note">配置说明</strong>
        <p>
          保存时校验所有配置及依赖。运行实例保留启动时的配置，重启后使用新定义。
        </p>
        <p>覆盖文件按名称顺序合并，优先于主文件中的同名标量设置。</p>
      </div>
    </section>
    <section class="paper editor-panel">
      <div class="panel-heading">
        <div>
          <span class="eyebrow"
            >配置文件 /
            {{ desktop.state.document ? "编辑中" : "等待选择" }}</span
          >
          <h2>{{ desktop.state.selected || "选择子进程" }}</h2>
        </div>
        <NButton
          :disabled="
            desktop.locked.value ||
            desktop.state.editorLoading ||
            !desktop.state.selected
          "
          @click="desktop.state.dropinVisible = true"
          ><template #icon><AppIcon name="add" /></template>覆盖文件</NButton
        >
      </div>
      <div class="editor-toolbar">
        <NSelect
          :value="desktop.state.document || null"
          :options="options"
          :disabled="
            desktop.locked.value ||
            desktop.state.editorLoading ||
            !options.length
          "
          placeholder="选择配置文件"
          aria-label="配置文件"
          @update:value="desktop.changeDocument"
        /><small>UTF-8 · systemd 配置子集</small>
      </div>
      <NSpin :show="desktop.state.editorLoading"
        ><NInput
          v-model:value="desktop.state.text"
          type="textarea"
          class="config-editor"
          :readonly="
            desktop.locked.value ||
            desktop.state.editorLoading ||
            !desktop.state.document
          "
          :autosize="{ minRows: 18, maxRows: 26 }"
          :input-props="{ spellcheck: false, 'aria-label': '子进程配置正文' }"
          placeholder="选择一个子进程以编辑配置"
      /></NSpin>
      <div class="editor-footer">
        <span class="editor-sync" :class="{ dirty: desktop.dirty.value }"
          ><AppIcon
            v-if="desktop.state.document"
            :name="desktop.dirty.value ? 'dirty' : 'synced'"
            :size="14"
          />{{
            desktop.dirty.value
              ? "有未保存的修改"
              : desktop.state.document
                ? "已与磁盘同步"
                : "等待选择配置"
          }}</span
        >
        <div>
          <NButton
            :disabled="
              desktop.locked.value ||
              desktop.state.editorLoading ||
              !desktop.state.document
            "
            @click="desktop.saveConfig(false)"
            ><template #icon><AppIcon name="save" /></template>保存配置</NButton
          ><NButton
            type="primary"
            :disabled="
              desktop.locked.value ||
              desktop.state.editorLoading ||
              !desktop.state.document
            "
            @click="desktop.saveConfig(true)"
            ><template #icon><AppIcon name="restart" /></template
            >保存并重启</NButton
          >
        </div>
      </div>
      <NCollapse class="syntax-help"
        ><NCollapseItem title="健康监控配置" name="health"
          ><HealthConfigForm :desktop="desktop" /></NCollapseItem
        ><NCollapseItem title="支持的配置指令与示例" name="syntax">
          <pre>{{ example }}</pre>
          <p>
            命令不经过 shell。路径需使用 Windows
            绝对路径，推荐正斜杠。PowerShell 自身的 $ 变量写为 $$。
          </p></NCollapseItem
        ></NCollapse
      >
    </section>
  </div>
</template>
