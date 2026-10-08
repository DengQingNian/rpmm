<script setup lang="ts">
import { computed, ref } from "vue";
import { NAlert, NButton, NInput, NModal } from "naive-ui";
import appIcon from "../assets/icon.svg";
import { useDesktop } from "../composables/useDesktop";
import MonitorPage from "./MonitorPage.vue";
import DashboardPage from "./DashboardPage.vue";
import ConfigPage from "./ConfigPage.vue";
import SettingsPage from "./SettingsPage.vue";
import ProcessForm from "./ProcessForm.vue";
import type { Page } from "../types";
import AppIcon from "./AppIcon.vue";
import type { IconName } from "../icons";
const desktop = useDesktop();
const filename = ref("20-local.conf");
const dropinError = ref(false);
const pages: {
  key: Page;
  title: string;
  description: string;
  icon: IconName;
  number: string;
}[] = [
  {
    key: "dashboard",
    title: "Dashboard",
    description: "宿主机资源、磁盘容量与托管进程概览。",
    icon: "dashboard",
    number: "01",
  },
  {
    key: "monitor",
    title: "进程监控",
    description: "管理进程启停、运行日志、资源使用与健康检查。",
    icon: "monitor",
    number: "02",
  },
  {
    key: "config",
    title: "子进程配置",
    description: "编辑启动命令、环境、依赖与自动重启策略。",
    icon: "config",
    number: "03",
  },
  {
    key: "settings",
    title: "应用设置",
    description: "配置登录启动、托盘行为与监控刷新间隔。",
    icon: "settings",
    number: "04",
  },
];
const currentPage = computed(() =>
  pages.find((page) => page.key === desktop.state.page)!,
);
/** 创建覆盖文件并触发表单错误反馈。参数：无。返回：无。 */
async function submitDropin(): Promise<void> {
  dropinError.value = false;
  if (!(await desktop.createDropin(filename.value.trim())))
    requestAnimationFrame(() => {
      dropinError.value = true;
    });
}
</script>
<template>
  <div class="desktop-shell">
    <aside class="sidebar">
      <div class="brand">
        <img :src="appIcon" alt="rpmm 图标" />
        <div>
          <strong>rpmm<span class="brand-dot">.</span></strong
          ><small>本地进程工作台</small>
        </div>
      </div>
      <div class="notebook-caption">
        工作空间 <AppIcon name="arrow" :size="22" />
      </div>
      <nav aria-label="主导航">
        <button
          v-for="page in pages"
          :key="page.key"
          class="nav-item"
          :class="{ active: desktop.state.page === page.key }"
          :aria-current="desktop.state.page === page.key ? 'page' : undefined"
          @click="desktop.navigate(page.key)"
        >
          <AppIcon class="nav-icon" :name="page.icon" :size="21" />
          <span class="underline-draw">{{ page.title }}</span
          ><small>{{ page.number }}</small>
        </button>
      </nav>
      <div class="sidebar-bottom">
        <div class="connection">
          <span
            class="status-mark"
            :class="{ connected: desktop.state.connected }"
          ></span
          >{{
            desktop.preview
              ? "浏览器预览"
              : desktop.state.connected
                ? "管理器已连接"
                : "等待连接管理器"
          }}
        </div>
        <p>
          {{
            desktop.state.quitting
              ? "正在停止子进程并退出…"
              : desktop.state.busy
                ? "正在执行操作…"
                : "当前用户会话 · 本机托管"
          }}
        </p>
        <NButton block :disabled="desktop.locked.value" @click="desktop.hide"
          ><template #icon><AppIcon name="tray" /></template>收起到托盘</NButton
        ><small>关闭窗口后继续在托盘运行</small>
      </div>
    </aside>
    <main>
      <header class="page-header">
        <div>
          <div class="eyebrow">
            RPMM / 控制台 <span>{{ currentPage.number }}</span>
          </div>
          <h1>{{ currentPage.title }}<span class="title-stroke"></span></h1>
          <p>{{ currentPage.description }}</p>
        </div>
        <div class="header-actions">
          <span class="live">{{
            desktop.state.updated
              ? `已同步 ${desktop.state.updated}`
              : "等待同步"
          }}</span>
          <div>
            <NButton :disabled="desktop.locked.value" @click="desktop.reload"
              ><template #icon><AppIcon name="reload" /></template
              >重载配置</NButton
            ><NButton
              type="primary"
              :disabled="desktop.locked.value"
              @click="desktop.state.createVisible = true"
              ><template #icon><AppIcon name="add" /></template
              >新建子进程</NButton
            >
          </div>
        </div>
      </header>
      <NAlert
        v-if="desktop.preview"
        type="info"
        :show-icon="false"
        class="preview-note"
        >浏览器预览 · 进程操作需要在 Tauri 桌面应用中执行。</NAlert
      >
      <NAlert v-if="desktop.state.quitting" type="warning" class="preview-note"
        >正在按依赖顺序停止所有子进程，请等待应用自动退出。</NAlert
      >
      <div :key="desktop.state.page" class="page-content fade-in-up">
        <DashboardPage
          v-if="desktop.state.page === 'dashboard'"
          :desktop="desktop"
        /><MonitorPage
          v-else-if="desktop.state.page === 'monitor'"
          :desktop="desktop"
        /><ConfigPage
          v-else-if="desktop.state.page === 'config'"
          :desktop="desktop"
        /><SettingsPage v-else :desktop="desktop" />
      </div>
      <footer>
        <span>当前用户会话</span><span>独立托管 / 依赖编排 / 自动重启</span>
      </footer>
    </main>
  </div>
  <NModal
    v-model:show="desktop.state.createVisible"
    preset="card"
    title="新建子进程"
    class="sketch-modal"
    :mask-closable="!desktop.locked.value"
    :closable="!desktop.locked.value"
    :close-on-esc="!desktop.locked.value"
    ><p class="modal-intro">配置可执行文件、参数和重启策略。</p>
    <ProcessForm
      :busy="desktop.locked.value"
      :visible="desktop.state.createVisible"
      @create="desktop.createProcess"
      @cancel="desktop.state.createVisible = false"
  /></NModal>
  <NModal
    v-model:show="desktop.state.dropinVisible"
    preset="card"
    title="新建覆盖文件"
    class="sketch-modal small-modal"
    :mask-closable="!desktop.locked.value"
    :closable="!desktop.locked.value"
    :close-on-esc="!desktop.locked.value"
    ><form
      :class="{ shake: dropinError }"
      @submit.prevent="submitDropin"
      @animationend="dropinError = false"
    >
      <label class="field-label">文件名</label
      ><NInput
        v-model:value="filename"
        :disabled="desktop.locked.value"
        placeholder="例如 20-local.conf"
        :input-props="{ 'aria-label': '覆盖文件名' }"
      />
      <p class="form-note">覆盖文件与主配置合并。保存前进行完整校验。</p>
      <div class="dialog-actions">
        <NButton
          :disabled="desktop.locked.value"
          @click="desktop.state.dropinVisible = false"
          >取消</NButton
        ><NButton
          type="primary"
          attr-type="submit"
          :disabled="desktop.locked.value"
          >开始编辑</NButton
        >
      </div>
    </form></NModal
  >
</template>
