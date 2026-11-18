<script setup lang="ts">
import { computed, ref } from "vue";
import { DEFAULT_CATEGORY } from "../categories";
import type { useDesktop } from "../composables/useDesktop";
import type { IconName } from "../icons";
import AppIcon from "./AppIcon.vue";
import HealthConfigForm from "./HealthConfigForm.vue";
import MdButton from "./MdButton.vue";
import MdCheckbox from "./MdCheckbox.vue";
import MdChipSet from "./MdChipSet.vue";
import MdEmpty from "./MdEmpty.vue";
import MdIconButton from "./MdIconButton.vue";
import MdSelect from "./MdSelect.vue";
import MdTextField from "./MdTextField.vue";
import ServiceOptionsForm from "./ServiceOptionsForm.vue";

/** 服务配置：左侧按分类分组的配置文件目录，右侧配置正文与表单。 */
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();

const documentOptions = computed(() =>
  props.desktop.state.documents.map((item) => ({ label: item.name, value: item.name })),
);

// 拖动中的子进程与目标分类只用于界面反馈，落盘由分类状态负责。
const dragging = ref("");
const dragTarget = ref("");
const syntaxOpen = ref<"none" | "options" | "health" | "syntax">("none");

/** 记录拖动中的子进程。参数：name 为子进程名称，event 为拖动事件。返回：无。 */
function startDrag(name: string, event: DragEvent): void {
  if (props.desktop.locked.value) return;
  dragging.value = name;
  dragTarget.value = "";
  event.dataTransfer?.setData("text/plain", name);
  if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
}

/** 高亮当前经过的分类。参数：name 为分类名称。返回：无。 */
function dragOver(name: string): void {
  if (dragging.value) dragTarget.value = name;
}

/** 离开分类时清除高亮，经过子元素时不闪烁。参数：event 为拖动事件，name 为分类名称。返回：无。 */
function dragLeave(event: DragEvent, name: string): void {
  const next = event.relatedTarget as Node | null;
  const area = event.currentTarget as HTMLElement | null;
  if (dragTarget.value === name && (!next || !area?.contains(next))) dragTarget.value = "";
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

/** 处理“移动到分类”选择。参数：unit 为子进程名称，name 为分类名称。返回：无。 */
function moveTo(unit: string, name: string): void {
  dragging.value = name;
  void props.desktop.assignCategory(unit, name).finally(endDrag);
}

/** 生成某个子进程可移动的目标分类。参数：unit 为子进程名称。返回：不含当前分类的选项。 */
function moveOptions(unit: string) {
  const current = props.desktop.unitCategory(unit);
  return props.desktop.categoryGroups.value
    .filter((group) => group.name !== current)
    .map((group) => ({ label: group.name, value: group.name }));
}

/** 切换折叠面板。参数：name 为面板标识。返回：无。 */
function togglePanel(name: "options" | "health" | "syntax"): void {
  syntaxOpen.value = syntaxOpen.value === name ? "none" : name;
}

/** 折叠面板的图标。参数：name 为面板标识。返回：箭头图标名。 */
function panelIcon(name: "options" | "health" | "syntax"): IconName {
  return syntaxOpen.value === name ? "chevron-down" : "chevron-right";
}

const example = '[Unit]\nDescription=应用说明\nRequires=database.service\nAfter=database.service\n\n[Service]\nType=simple\nExecStart="C:/Apps/app.exe" --port 8080\nWorkingDirectory=C:/Apps\nEnvironment="PORT=8080"\nRestart=on-failure\nRestartSec=2s\nTimeoutStopSec=30s\nHealthType=http\nHealthUrl=http://127.0.0.1:8080/health\nHealthTimeoutSec=1s\nHealthIntervalSec=10s\n\n[Install]\nWantedBy=multi-user.target';
</script>

<template>
  <div class="config-workspace">
    <!-- 配置目录 -->
    <aside class="panel config-tree">
      <div class="panel__head">
        <div>
          <h2 class="panel__title">配置目录</h2>
          <p class="panel__subtitle">{{ desktop.state.statuses.length }} 个服务 · 按分类整理</p>
        </div>
        <MdIconButton
          name="category-add"
          label="新增分类"
          :disabled="desktop.locked.value"
          @click="desktop.state.categoryVisible = true"
        />
      </div>

      <div v-if="desktop.state.statuses.length" class="config-tree__groups">
        <section
          v-for="group in desktop.categoryGroups.value"
          :key="group.name"
          class="config-group"
          :class="{ 'is-drop-target': dragging && dragTarget === group.name }"
          @dragover.prevent="dragOver(group.name)"
          @dragleave="dragLeave($event, group.name)"
          @drop.prevent="dropCategory(group.name)"
        >
          <div class="config-group__head">
            <button
              type="button"
              class="config-group__toggle"
              :aria-expanded="desktop.categoryExpanded(group.name)"
              @click="desktop.toggleCategory(group.name)"
            >
              <AppIcon
                :name="desktop.categoryExpanded(group.name) ? 'chevron-down' : 'chevron-right'"
                :size="18"
              />
              <span class="config-group__name">{{ group.name }}</span>
              <span class="config-group__count">{{ group.units.length }}</span>
            </button>
            <MdIconButton
              v-if="group.name !== DEFAULT_CATEGORY"
              name="remove"
              :label="'删除分类 ' + group.name"
              :disabled="desktop.locked.value"
              :size="16"
              @click="desktop.removeCategory(group.name)"
            />
          </div>
          <div v-show="desktop.categoryExpanded(group.name)" class="config-group__body">
            <div v-for="unit in group.units" :key="unit" class="config-unit">
              <button
                type="button"
                class="config-unit__main"
                :class="{
                  'is-selected': unit === desktop.state.selected,
                  'is-dragging': dragging === unit,
                }"
                :disabled="desktop.locked.value"
                :draggable="!desktop.locked.value"
                :title="'拖动 ' + unit + ' 到其他分类'"
                @click="desktop.selectProcess(unit)"
                @dragstart="startDrag(unit, $event)"
                @dragend="endDrag"
              >
                <AppIcon name="file" :size="18" />
                <span>{{ unit }}</span>
              </button>
              <MdSelect
                v-if="desktop.categoryGroups.value.length > 1"
                :model-value="null"
                :options="moveOptions(unit)"
                :label="'移动 ' + unit + ' 到其他分类'"
                placeholder="移动"
                width="96px"
                :disabled="desktop.locked.value"
                @update:model-value="moveTo(unit, $event)"
              />
            </div>
            <p v-if="!group.units.length" class="config-group__empty">拖动子进程到此分类</p>
          </div>
        </section>
      </div>
      <MdEmpty v-else compact title="暂无服务配置" description="点击顶部“新建子进程”开始。" />

      <div class="config-tree__io">
        <h3 class="section-title">导入与导出</h3>
        <MdChipSet
          v-model="desktop.state.exportUnits"
          :options="desktop.state.statuses.map((item) => ({ value: item.name, label: item.name }))"
          label="选择要导出的服务"
          :disabled="desktop.locked.value"
          empty-text="还没有可导出的服务"
        />
        <div class="config-tree__io-actions">
          <MdButton
            variant="tonal"
            icon="download"
            :disabled="desktop.locked.value || !desktop.state.exportUnits.length"
            @click="desktop.exportConfigs()"
            >导出所选</MdButton
          >
          <MdButton
            variant="outlined"
            icon="folder-open"
            :disabled="desktop.locked.value"
            @click="desktop.importConfigs()"
            >导入配置包</MdButton
          >
        </div>
        <MdCheckbox v-model="desktop.state.importOverwrite" label="导入时替换同名服务" />
        <p class="form-note">
          JSON 配置包包含主配置、覆盖文件和启用状态，导入前校验整包；关联服务请一并选择。
        </p>
      </div>
    </aside>

    <!-- 编辑器 -->
    <section class="panel editor">
      <div class="panel__head">
        <div>
          <h2 class="panel__title">{{ desktop.state.selected || "选择子进程" }}</h2>
          <p class="panel__subtitle">
            配置文件 / {{ desktop.state.document ? "编辑中" : "等待选择" }}
          </p>
        </div>
        <MdButton
          variant="tonal"
          icon="add"
          :disabled="desktop.locked.value || desktop.state.editorLoading || !desktop.state.selected"
          @click="desktop.state.dropinVisible = true"
          >覆盖文件</MdButton
        >
      </div>

      <div class="editor__toolbar">
        <MdSelect
          :model-value="desktop.state.document || null"
          :options="documentOptions"
          label="配置文件"
          placeholder="选择配置文件"
          width="min(380px, 100%)"
          prefix-icon="code"
          :disabled="desktop.locked.value || desktop.state.editorLoading || !documentOptions.length"
          @update:model-value="desktop.changeDocument($event)"
        />
        <span class="editor__meta">UTF-8 · systemd 配置子集</span>
      </div>

      <div class="editor__surface" :class="{ 'is-loading': desktop.state.editorLoading }">
        <textarea
          v-model="desktop.state.text"
          class="editor__code"
          spellcheck="false"
          aria-label="服务配置正文"
          placeholder="选择一个子进程以编辑配置"
          :readonly="desktop.locked.value || desktop.state.editorLoading || !desktop.state.document"
        ></textarea>
        <span v-if="desktop.state.editorLoading" class="editor__loading">正在读取配置…</span>
      </div>

      <div class="editor__footer">
        <span class="editor__sync" :class="{ 'is-dirty': desktop.dirty.value }">
          <AppIcon
            :name="desktop.dirty.value ? 'dot' : 'success'"
            :size="16"
          />
          {{
            desktop.dirty.value
              ? "有未保存的修改"
              : desktop.state.document
                ? "已与磁盘同步"
                : "等待选择配置"
          }}
        </span>
        <div class="editor__actions">
          <MdButton
            variant="text"
            icon="remove"
            class="is-destructive-text"
            :disabled="desktop.locked.value || desktop.state.editorLoading || !desktop.state.selected"
            @click="desktop.deleteService()"
            >删除服务</MdButton
          >
          <MdButton
            variant="outlined"
            icon="save"
            :disabled="desktop.locked.value || desktop.state.editorLoading || !desktop.state.document"
            @click="desktop.saveConfig(false)"
            >保存配置</MdButton
          >
          <MdButton
            variant="filled"
            icon="restart"
            :disabled="desktop.locked.value || desktop.state.editorLoading || !desktop.state.document"
            @click="desktop.saveConfig(true)"
            >保存并重启</MdButton
          >
        </div>
      </div>

      <div class="editor__panels">
        <section class="accordion" :class="{ 'is-open': syntaxOpen === 'options' }">
          <button type="button" class="accordion__head" @click="togglePanel('options')">
            <AppIcon :name="panelIcon('options')" :size="18" />
            <span>启动关联、标准输出与资源上限</span>
          </button>
          <div v-if="syntaxOpen === 'options'" class="accordion__body">
            <ServiceOptionsForm
              :draft="desktop.state.serviceDraft"
              :services="desktop.state.statuses.map((item) => item.name)"
              :current="desktop.state.selected"
              :disabled="desktop.locked.value || desktop.state.editorLoading || !desktop.state.document"
            />
            <p class="form-note">
              表单读取当前文档。覆盖文件不能删除主配置或其他覆盖文件中继承的 Requires/After/Before/Wants；移除关联时请编辑定义该关联的文件。
            </p>
            <MdButton
              variant="tonal"
              icon="check"
              :disabled="desktop.locked.value || desktop.state.editorLoading || !desktop.state.document"
              @click="desktop.applyOptions()"
              >写入当前草稿</MdButton
            >
          </div>
        </section>
        <section class="accordion" :class="{ 'is-open': syntaxOpen === 'health' }">
          <button type="button" class="accordion__head" @click="togglePanel('health')">
            <AppIcon :name="panelIcon('health')" :size="18" />
            <span>健康监控配置</span>
          </button>
          <div v-if="syntaxOpen === 'health'" class="accordion__body">
            <HealthConfigForm :desktop="desktop" />
          </div>
        </section>
        <section class="accordion" :class="{ 'is-open': syntaxOpen === 'syntax' }">
          <button type="button" class="accordion__head" @click="togglePanel('syntax')">
            <AppIcon :name="panelIcon('syntax')" :size="18" />
            <span>支持的配置指令与示例</span>
          </button>
          <div v-if="syntaxOpen === 'syntax'" class="accordion__body">
            <pre class="code-block">{{ example }}</pre>
            <p class="form-note">
              命令不经过 shell。路径需使用 Windows 绝对路径，推荐正斜杠。PowerShell 自身的 $ 变量写为 $$。
            </p>
          </div>
        </section>
      </div>
    </section>
  </div>
</template>