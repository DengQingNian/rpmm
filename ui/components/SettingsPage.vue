<script setup lang="ts">
import { NButton, NSelect, NSwitch } from "naive-ui";
import { computed } from "vue";
import type { useDesktop } from "../composables/useDesktop";
import AppIcon from "./AppIcon.vue";
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();
const refreshOptions = computed(() =>
  [
    ...new Set([
      500,
      1000,
      2000,
      5000,
      10000,
      props.desktop.state.settingsDraft.refresh_ms,
    ]),
  ]
    .sort((a, b) => a - b)
    .map((value) => ({ label: `${value / 1000} 秒`, value })),
);
</script>
<template>
  <div class="settings-layout">
    <section class="paper settings-panel">
      <div class="panel-heading">
        <div>
          <span class="eyebrow">运行偏好</span>
          <h2>启动与运行</h2>
        </div>
      </div>
      <form @submit.prevent="desktop.saveSettings">
        <label class="setting-row"
          ><div>
            <strong>开机启动</strong>
            <p>当前用户登录 Windows 后自动启动 rpmm，并隐藏到托盘。</p>
          </div>
          <NSwitch
            v-model:value="desktop.state.settingsDraft.autostart"
            :disabled="desktop.locked.value || !desktop.state.settings"
            aria-label="开机启动"
        /></label>
        <label class="setting-row"
          ><div>
            <strong>手动启动时隐藏窗口</strong>
            <p>启动后保持托盘运行，点击托盘图标打开控制台。</p>
          </div>
          <NSwitch
            v-model:value="desktop.state.settingsDraft.start_hidden"
            :disabled="desktop.locked.value || !desktop.state.settings"
            aria-label="手动启动时隐藏窗口"
        /></label>
        <label class="setting-row"
          ><div>
            <strong>监控刷新间隔</strong>
            <p>
              进程状态、健康状态和资源采样的刷新频率；日志刷新在日志面板单独设置。
            </p>
          </div>
          <NSelect
            v-model:value="desktop.state.settingsDraft.refresh_ms"
            :options="refreshOptions"
            :disabled="desktop.locked.value || !desktop.state.settings"
            aria-label="监控刷新间隔"
        /></label>
        <div class="settings-footer">
          <NButton
            type="primary"
            attr-type="submit"
            :loading="desktop.state.busy"
            :disabled="desktop.locked.value || !desktop.state.settings"
            ><template #icon><AppIcon name="save" /></template>保存设置</NButton
          >
        </div>
      </form>
    </section>
    <section class="paper settings-panel">
      <div class="panel-heading">
        <div>
          <span class="eyebrow">本地工作空间</span>
          <h2>数据与托盘</h2>
        </div>
        <AppIcon name="home" />
      </div>
      <div class="setting-block">
        <strong>数据目录</strong
        ><code>{{
          desktop.state.settings?.root || "请使用桌面应用读取数据目录"
        }}</code>
        <p>
          包含 units 配置、state 状态和 logs 日志。使用 --root
          参数可指定其他目录。
        </p>
      </div>
      <div class="setting-block">
        <strong>托盘行为</strong>
        <p>
          关闭或最小化窗口会收起到托盘，子进程持续运行。退出时按依赖顺序停止所有托管进程，清理残留进程树。
        </p>
        <NButton
          type="error"
          :disabled="desktop.locked.value"
          @click="desktop.quit"
          ><template #icon><AppIcon name="quit" /></template
          >退出并停止所有子进程</NButton
        >
      </div>
    </section>
  </div>
</template>
