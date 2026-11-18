<script setup lang="ts">
import { computed, ref } from "vue";
import appIcon from "../assets/icon.svg";
import { MAX_CATEGORY_CHARS } from "../categories";
import { useDesktop } from "../composables/useDesktop";
import type { IconName } from "../icons";
import { useTheme } from "../theme";
import type { Page } from "../types";
import AppIcon from "./AppIcon.vue";
import ConfigPage from "./ConfigPage.vue";
import DashboardPage from "./DashboardPage.vue";
import MdAlert from "./MdAlert.vue";
import MdButton from "./MdButton.vue";
import MdDialog from "./MdDialog.vue";
import MdIconButton from "./MdIconButton.vue";
import MdTextField from "./MdTextField.vue";
import MonitorPage from "./MonitorPage.vue";
import ProcessForm from "./ProcessForm.vue";
import SettingsPage from "./SettingsPage.vue";

const desktop = useDesktop();
const theme = useTheme();

/** 主导航目标；宽屏用展开式导航抽屉，窄屏用图标导航栏。 */
const pages: { key: Page; title: string; description: string; icon: IconName }[] = [
  {
    key: "dashboard",
    title: "Dashboard",
    description: "宿主机资源、磁盘容量与托管进程概览。",
    icon: "dashboard",
  },
  {
    key: "monitor",
    title: "进程监控",
    description: "管理进程启停、运行日志、资源使用与健康检查。",
    icon: "monitor",
  },
  {
    key: "config",
    title: "服务配置",
    description: "编辑启动命令、环境、依赖与自动重启策略。",
    icon: "config",
  },
  {
    key: "settings",
    title: "应用设置",
    description: "配置登录启动、托盘行为与监控刷新间隔。",
    icon: "settings",
  },
];

const currentPage = computed(() => pages.find((page) => page.key === desktop.state.page)!);
const collapsed = computed(() => desktop.state.navCollapsed);

/** 主题按钮的无障碍名称。参数：无。返回：下一步切换到的主题说明。 */
const themeLabel = computed(() =>
  theme.mode.value === "auto"
    ? "跟随系统（点击切换为浅色）"
    : theme.mode.value === "light"
      ? "浅色模式（点击切换为深色）"
      : "深色模式（点击切换为跟随系统）",
);

/** 连接状态文案。参数：无。返回：当前连接说明。 */
const connectionText = computed(() => {
  if (desktop.preview) return "浏览器预览";
  return desktop.state.connected ? "管理器已连接" : "等待连接管理器";
});

const createForm = ref<InstanceType<typeof ProcessForm> | null>(null);
const filename = ref("20-local.conf");
const dropinError = ref(false);
const categoryName = ref("");
const categoryError = ref(false);

/** 由对话框底部按钮提交新建表单。参数：无。返回：无。 */
function submitCreate(): void {
  void createForm.value?.submit();
}

/** 新建覆盖文件；失败时短暂抖动提示。参数：无。返回：无。 */
async function submitDropin(): Promise<void> {
  dropinError.value = false;
  if (await desktop.createDropin(filename.value.trim())) return;
  requestAnimationFrame(() => {
    dropinError.value = true;
  });
}

/** 新建分类；失败时抖动并保留输入以便修改。参数：无。返回：无。 */
async function submitCategory(): Promise<void> {
  categoryError.value = false;
  if (!(await desktop.addCategory(categoryName.value))) {
    requestAnimationFrame(() => {
      categoryError.value = true;
    });
    return;
  }
  categoryName.value = "";
}
</script>

<template>
  <div class="app-shell" :class="{ 'is-rail': collapsed }">
    <!-- 导航抽屉：宽屏常驻，窄屏为带遮罩的模态抽屉。 -->
    <div
      v-if="desktop.state.navOpen"
      class="app-shell__scrim"
      @click="desktop.state.navOpen = false"
    />
    <nav
      class="navigation"
      :class="{ 'is-open': desktop.state.navOpen }"
      aria-label="主导航"
    >
      <div class="navigation__head">
        <img class="navigation__logo" :src="appIcon" alt="" aria-hidden="true" />
        <div v-if="!collapsed" class="navigation__brand">
          <strong>rpmm</strong>
          <span>本地进程工作台</span>
        </div>
      </div>

      <div class="navigation__items">
        <button
          v-for="page in pages"
          :key="page.key"
          type="button"
          class="navigation__item"
          :class="{ 'is-active': desktop.state.page === page.key }"
          :aria-current="desktop.state.page === page.key ? 'page' : undefined"
          :title="collapsed ? page.title : undefined"
          @click="desktop.navigate(page.key)"
        >
          <span class="navigation__indicator">
            <AppIcon :name="page.icon" :size="22" />
          </span>
          <span v-if="!collapsed" class="navigation__label">{{ page.title }}</span>
        </button>
      </div>

      <div class="navigation__foot">
        <button
          v-if="!collapsed"
          type="button"
          class="navigation__status"
          :title="connectionText"
        >
          <span
            class="status-dot"
            :class="desktop.state.connected ? 'is-online' : 'is-offline'"
            aria-hidden="true"
          />
          <span>{{ connectionText }}</span>
        </button>
        <span
          v-else
          class="status-dot status-dot--big"
          :class="desktop.state.connected ? 'is-online' : 'is-offline'"
          :title="connectionText"
        />
        <MdButton
          v-if="!collapsed"
          variant="tonal"
          icon="tray"
          :disabled="desktop.locked.value"
          @click="desktop.hide()"
          >收起到托盘</MdButton
        >
        <MdIconButton
          v-else
          name="tray"
          variant="tonal"
          label="收起到托盘"
          :disabled="desktop.locked.value"
          @click="desktop.hide()"
        />
        <p v-if="!collapsed" class="navigation__hint">
          {{
            desktop.state.quitting
              ? "正在停止子进程并退出…"
              : desktop.state.busy
                ? "正在执行操作…"
                : "管理器已就绪 · 本机托管"
          }}
        </p>
      </div>
    </nav>

    <div class="app-shell__body">
      <header class="top-app-bar">
        <MdIconButton
          class="top-app-bar__menu"
          name="menu"
          label="打开导航"
          @click="desktop.state.navOpen = true"
        />
        <MdIconButton
          class="top-app-bar__toggle"
          :name="collapsed ? 'expand' : 'collapse'"
          :label="collapsed ? '展开导航栏' : '收起导航栏'"
          @click="desktop.toggleNav()"
        />
        <div class="top-app-bar__titles">
          <h1 class="top-app-bar__title">{{ currentPage.title }}</h1>
          <p class="top-app-bar__subtitle">{{ currentPage.description }}</p>
        </div>
        <div class="top-app-bar__actions">
          <span class="sync-chip" :title="'最近一次同步时间'">
            <span
              class="status-dot"
              :class="desktop.state.connected ? 'is-online' : 'is-offline'"
              aria-hidden="true"
            />
            {{ desktop.state.updated ? desktop.state.updated : "等待同步" }}
          </span>
          <MdIconButton
            :name="(theme.resolved.value === 'dark' ? 'theme-dark' : 'theme') as IconName"
            :label="themeLabel"
            @click="theme.cycle()"
          />
          <MdButton
            variant="tonal"
            icon="reload"
            :disabled="desktop.locked.value"
            @click="desktop.reload()"
            >重载配置</MdButton
          >
          <MdButton
            variant="filled"
            icon="add"
            :disabled="desktop.locked.value"
            @click="desktop.state.createVisible = true"
            >新建子进程</MdButton
          >
        </div>
      </header>

      <main class="app-main">
        <MdAlert v-if="desktop.preview" kind="info" class="app-main__banner">
          浏览器预览 · 进程操作需要在 Tauri 桌面应用中执行。
        </MdAlert>
        <MdAlert v-if="desktop.state.quitting" kind="warning" class="app-main__banner">
          正在按依赖顺序停止所有子进程，请等待应用自动退出。
        </MdAlert>
        <Transition name="page" mode="out-in">
          <div :key="desktop.state.page" class="app-main__page">
            <DashboardPage
              v-if="desktop.state.page === 'dashboard'"
              :desktop="desktop"
            />
            <MonitorPage
              v-else-if="desktop.state.page === 'monitor'"
              :desktop="desktop"
            />
            <ConfigPage
              v-else-if="desktop.state.page === 'config'"
              :desktop="desktop"
            />
            <SettingsPage v-else :desktop="desktop" />
          </div>
        </Transition>
      </main>
    </div>

    <!-- 新建子进程：大型对话框，表单可滚动。 -->
    <MdDialog
      :open="desktop.state.createVisible"
      title="新建子进程"
      :busy="desktop.locked.value"
      :closable="!desktop.locked.value"
      width="min(680px, calc(100vw - 48px))"
      tall
      confirm-label="创建配置"
      @close="desktop.state.createVisible = false"
      @confirm="submitCreate"
    >
      <ProcessForm
        ref="createForm"
        :busy="desktop.locked.value"
        :visible="desktop.state.createVisible"
        :services="desktop.state.statuses.map((item) => item.name)"
        @create="desktop.createProcess"
        @cancel="desktop.state.createVisible = false"
      />
    </MdDialog>

    <MdDialog
      :open="desktop.state.dropinVisible"
      title="新建覆盖文件"
      :busy="desktop.locked.value"
      :closable="!desktop.locked.value"
      width="min(460px, calc(100vw - 48px))"
      confirm-label="开始编辑"
      @close="desktop.state.dropinVisible = false"
      @confirm="submitDropin"
    >
      <form :class="{ shake: dropinError }" @submit.prevent="submitDropin" @animationend="dropinError = false">
        <MdTextField
          v-model="filename"
          label="文件名"
          placeholder="例如 20-local.conf"
          supporting="覆盖文件与主配置合并，保存前进行完整校验。"
          :disabled="desktop.locked.value"
          @enter="submitDropin"
        />
      </form>
    </MdDialog>

    <MdDialog
      :open="desktop.state.categoryVisible"
      title="新增分类"
      :busy="desktop.locked.value"
      :closable="!desktop.locked.value"
      width="min(460px, calc(100vw - 48px))"
      confirm-label="创建分类"
      @close="desktop.state.categoryVisible = false"
      @confirm="submitCategory"
    >
      <form :class="{ shake: categoryError }" @submit.prevent="submitCategory" @animationend="categoryError = false">
        <MdTextField
          v-model="categoryName"
          label="分类名称"
          placeholder="例如 数据库"
          :maxlength="MAX_CATEGORY_CHARS"
          supporting="分类只用于整理配置目录，不会写入 .service 文件；可拖动子进程或使用移动菜单更换分类。"
          :disabled="desktop.locked.value"
          @enter="submitCategory"
        />
      </form>
    </MdDialog>
  </div>
</template>