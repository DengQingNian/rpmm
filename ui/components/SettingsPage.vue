<script setup lang="ts">
import { NButton, NInput, NSelect, NSwitch, NTag } from "naive-ui";
import { computed } from "vue";
import type { useDesktop } from "../composables/useDesktop";
import AppIcon from "./AppIcon.vue";
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();
/** 开机自启开关：读后端真实注册状态，写入需要管理员授权。 */
const boot = computed({
  get: () => props.desktop.state.settings?.boot.installed ?? false,
  set: (value: boolean) => {
    void props.desktop.toggleBoot(value);
  },
});
/** 开机自启运行状态的中文说明。 */
const bootState = computed(() => {
  const registration = props.desktop.state.settings?.boot;
  if (!registration?.installed) return "未注册";
  return (
    {
      running: "运行中",
      stopped: "已停止",
      "start-pending": "启动中",
      "stop-pending": "停止中",
      paused: "已暂停",
    }[registration.state] ?? registration.state
  );
});
/** 开机自启账户说明，空账户表示系统账户。 */
const bootAccount = computed(() => {
  const account = props.desktop.state.settings?.boot.account;
  return account ? account : "LocalSystem";
});
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
            <strong
              >开机自启（无需登录）
              <NTag
                :type="
                  desktop.state.settings?.boot.installed ? 'success' : 'default'
                "
                size="small"
                >{{ bootState }}</NTag
              ></strong
            >
            <p>
              注册为 Windows 服务并由系统在启动时拉起，不需要任何用户登录；已启用的服务随守护进程一起启动。
              注册与取消需要管理员授权，被托管的子进程以 {{ bootAccount }}
              账户运行在会话 0，不再继承登录用户的桌面环境。
            </p>
            <p v-if="desktop.state.settings?.daemon" class="setting-hint">
              当前控制台连接的是该守护进程；退出控制台不会停止它。
            </p>
            <p
              v-else-if="desktop.state.settings?.boot.installed"
              class="setting-hint"
            >
              当前控制台仍在本机托管同一数据目录，守护进程将在下次系统启动时接管；立即移交请先退出控制台，再执行
              rpmm autostart start。
            </p>
          </div>
          <NSwitch
            v-model:value="boot"
            :disabled="
              desktop.locked.value ||
              desktop.state.busy ||
              !desktop.state.settings
            "
            aria-label="开机自启"
        /></label>
        <label class="setting-row"
          ><div>
            <strong>登录后打开控制台</strong>
            <p>
              当前用户登录 Windows 后自动启动控制台并隐藏到托盘，只影响登录会话，与开机自启相互独立。
            </p>
          </div>
          <NSwitch
            v-model:value="desktop.state.settingsDraft.autostart"
            :disabled="desktop.locked.value || !desktop.state.settings"
            aria-label="登录后打开控制台"
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
          <h2>数据与托盘</h2>
        </div>
        <AppIcon name="home" />
      </div>
      <div class="setting-block">
        <strong>数据目录</strong
        ><NInput v-model:value="desktop.state.settingsDraft.root" :disabled="desktop.locked.value || !desktop.state.settings" placeholder="例如 C:\rpmm 或 /c/rpmm" />
        <NButton :disabled="desktop.locked.value || !desktop.state.settings" @click="desktop.chooseRoot">选择文件夹</NButton>
        <p>当前使用：{{ desktop.state.settings?.root || "请通过桌面应用读取" }}</p>
        <p>
          包含 units 配置、state 状态和 logs 日志。点击“保存设置”后，下次启动应用时使用新目录。已有数据不自动搬迁，可通过配置导出和导入转移服务。显式 --root 参数优先。
        </p>
      </div>
      <div class="setting-block">
        <strong>托盘行为</strong>
        <p v-if="desktop.state.settings?.daemon">
          关闭或最小化窗口会收起到托盘。当前由开机自启的守护进程托管子进程，退出控制台只关闭窗口，服务继续运行；停止服务请用
          rpmm autostart stop 或以管理员身份在服务管理器中停止。
        </p>
        <p v-else>
          关闭或最小化窗口会收起到托盘，子进程持续运行。退出时按依赖顺序停止所有托管进程，清理残留进程树。
        </p>
        <NButton
          type="error"
          :disabled="desktop.locked.value"
          @click="desktop.quit"
          ><template #icon><AppIcon name="quit" /></template
          >{{
            desktop.state.settings?.daemon
              ? "退出控制台"
              : "退出并停止所有子进程"
          }}</NButton
        >
      </div>
    </section>
  </div>
</template>
