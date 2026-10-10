<script setup lang="ts">
import {
  NButton,
  NCheckbox,
  NCollapse,
  NCollapseItem,
  NDropdown,
  NEmpty,
  NInput,
  NSelect,
  NSpin,
} from "naive-ui";
import { computed, ref } from "vue";
import { DEFAULT_CATEGORY } from "../categories";
import type { useDesktop } from "../composables/useDesktop";
import AppIcon from "./AppIcon.vue";
import HealthConfigForm from "./HealthConfigForm.vue";
import ServiceOptionsForm from "./ServiceOptionsForm.vue";
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();
const options = computed(() =>
  props.desktop.state.documents.map((item) => ({
    label: item.name,
    value: item.name,
  })),
);
// 拖动中的子进程和目标分类只用于界面反馈，落盘由分类状态负责。
const dragging = ref("");
const dragTarget = ref("");

/** 记录拖动中的子进程。参数：name 为子进程名称，event 为拖动事件。返回：无。 */
function startDrag(name: string, event: DragEvent): void {
  if (props.desktop.locked.value) return;
  dragging.value = name;
  dragTarget.value = "";
  // 同时写入 dataTransfer，跨窗口拖动时仍能识别被拖动的对象。
  event.dataTransfer?.setData("text/plain", name);
  if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
}

/** 高亮当前经过的分类。参数：name 为分类名称。返回：无。 */
function dragOver(name: string): void {
  if (dragging.value) dragTarget.value = name;
}

/** 离开分类时清除高亮。参数：event 为拖动事件，name 为分类名称。返回：无。
 * 只在真正离开分类区域时清除，避免经过子元素时闪烁。 */
function dragLeave(event: DragEvent, name: string): void {
  const next = event.relatedTarget as Node | null;
  const area = event.currentTarget as HTMLElement | null;
  if (dragTarget.value === name && (!next || !area?.contains(next)))
    dragTarget.value = "";
}

/** 把拖动的子进程放到目标分类。参数：name 为分类名称。返回：无。 */
function dropCategory(name: string): void {
  const unit = dragging.value;
  endDrag();
  if (unit) void props.desktop.assignCategory(unit, name);
}

/** 结束拖动并清除高亮。参数：无。返回：无。 */
function endDrag(): void {
  dragging.value = "";
  dragTarget.value = "";
}

/** 处理"移动到分类"下拉选择。参数：unit 为子进程名称，name 为分类名称。返回：无。 */
function moveTo(unit: string, name: string | number): void {
  void props.desktop.assignCategory(unit, String(name));
}

/** 生成某一子进程可移动的目标分类。参数：unit 为子进程名称。返回：不含当前分类的下拉选项。 */
function moveOptions(unit: string) {
  const current = props.desktop.unitCategory(unit);
  return props.desktop.categoryGroups.value
    .filter((group) => group.name !== current)
    .map((group) => ({ label: group.name, key: group.name }));
}
const example = `[Unit]\nDescription=应用说明\nRequires=database.service\nAfter=database.service\n\n[Service]\nType=simple\nExecStart="C:/Apps/app.exe" --port 8080\nWorkingDirectory=C:/Apps\nEnvironment="PORT=8080"\nRestart=on-failure\nRestartSec=2s\nTimeoutStopSec=30s\nHealthType=http\nHealthUrl=http://127.0.0.1:8080/health\nHealthTimeoutSec=1s\nHealthIntervalSec=10s\n\n[Install]\nWantedBy=multi-user.target`;
</script>
<template>
  <div class="config-workspace">
    <section class="paper config-list">
      <div class="panel-heading">
        <h2>
          配置目录 <small>{{ desktop.state.statuses.length }}</small>
        </h2>
        <div class="config-heading-actions">
          <NButton
            quaternary
            size="small"
            title="新增分类"
            aria-label="新增分类"
            :disabled="desktop.locked.value"
            @click="desktop.state.categoryVisible = true"
            ><template #icon><AppIcon name="category-add" /></template
          ></NButton>
          <AppIcon name="folder" />
        </div>
      </div>
      <div v-if="desktop.state.statuses.length" class="config-groups">
        <section
          v-for="group in desktop.categoryGroups.value"
          :key="group.name"
          class="config-group"
          :class="{
            collapsed: !desktop.categoryExpanded(group.name),
            'drag-over': dragging && dragTarget === group.name,
          }"
          @dragover.prevent="dragOver(group.name)"
          @dragleave="dragLeave($event, group.name)"
          @drop.prevent="dropCategory(group.name)"
        >
          <div class="config-group-head-row">
            <button
              class="config-group-head"
              :aria-expanded="desktop.categoryExpanded(group.name)"
              @click="desktop.toggleCategory(group.name)"
            >
              <AppIcon
                :name="
                  desktop.categoryExpanded(group.name)
                    ? 'chevron-down'
                    : 'chevron-right'
                "
                :size="16"
              /><strong>{{ group.name }}</strong
              ><small>{{ group.units.length }}</small>
            </button>
            <button
              v-if="group.name !== DEFAULT_CATEGORY"
              class="config-group-remove"
              :title="`删除分类 ${group.name}`"
              :aria-label="`删除分类 ${group.name}`"
              :disabled="desktop.locked.value"
              @click="desktop.removeCategory(group.name)"
            >
              <AppIcon name="remove" :size="14" />
            </button>
          </div>
          <div
            v-show="desktop.categoryExpanded(group.name)"
            class="config-group-body"
          >
            <div
              v-for="process in group.units"
              :key="process"
              class="config-unit-row"
            >
              <button
                class="config-unit"
                :class="{
                  selected: process === desktop.state.selected,
                  dragging: dragging === process,
                }"
                :disabled="desktop.locked.value"
                :draggable="!desktop.locked.value"
                :title="`拖动 ${process} 到其他分类`"
                @click="desktop.selectProcess(process)"
                @dragstart="startDrag(process, $event)"
                @dragend="endDrag"
              >
                <AppIcon name="file" :size="16" />{{ process }}
              </button>
              <NDropdown
                v-if="desktop.categoryGroups.value.length > 1"
                trigger="click"
                :options="moveOptions(process)"
                @select="moveTo(process, $event)"
              >
                <button
                  class="config-unit-move"
                  :title="`移动 ${process} 到其他分类`"
                  :aria-label="`移动 ${process} 到其他分类`"
                  :disabled="desktop.locked.value"
                >
                  <AppIcon name="category-move" :size="14" />
                </button>
              </NDropdown>
            </div>
            <p v-if="!group.units.length" class="config-group-empty">
              拖动子进程到此分类
            </p>
          </div>
        </section>
      </div>
      <NEmpty v-else description="暂无子进程配置" class="empty" />
      <div class="config-guide">
        <strong>导入与导出</strong>
        <NSelect v-model:value="desktop.state.exportUnits" multiple :options="desktop.state.statuses.map(item => ({ label: item.name, value: item.name }))" :disabled="desktop.locked.value" placeholder="多选要导出的服务" />
        <NButton :disabled="desktop.locked.value || !desktop.state.exportUnits.length" @click="desktop.exportConfigs">导出所选配置</NButton>
        <NCheckbox v-model:checked="desktop.state.importOverwrite" :disabled="desktop.locked.value">导入时替换同名服务</NCheckbox>
        <NButton :disabled="desktop.locked.value" @click="desktop.importConfigs">导入配置包</NButton>
        <p>JSON 配置包包含主配置、覆盖文件和启用状态。导出已保存内容；关联服务请一并选择。导入前校验整包，保存后不会立即启动或重启。</p>
        <strong class="pencil-note">配置说明</strong>
        <p>
          保存时校验所有配置及依赖。运行实例保留启动时的配置，重启后使用新定义。
        </p>
        <p>覆盖文件按名称顺序合并，优先于主文件中的同名标量设置。</p>
        <p>
          删除服务会移除主配置、覆盖文件和启用状态；运行中的实例需先停止，被其他服务引用时需先解除引用。
        </p>
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
            type="error"
            secondary
            :disabled="
              desktop.locked.value ||
              desktop.state.editorLoading ||
              !desktop.state.selected
            "
            @click="desktop.deleteService"
            ><template #icon><AppIcon name="remove" /></template
            >删除服务</NButton
          ><NButton
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
        ><NCollapseItem title="启动关联、标准输出与资源上限" name="options">
          <ServiceOptionsForm :draft="desktop.state.serviceDraft" :services="desktop.state.statuses.map(item => item.name)" :current="desktop.state.selected" :disabled="desktop.locked.value || desktop.state.editorLoading || !desktop.state.document" />
          <p class="form-note">表单读取当前文档。覆盖文件不能删除主配置或其他覆盖文件中继承的 Requires/After/Before/Wants；移除关联时请编辑定义该关联的文件。</p>
          <NButton :disabled="desktop.locked.value || desktop.state.editorLoading || !desktop.state.document" @click="desktop.applyOptions">写入当前草稿</NButton>
        </NCollapseItem
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
