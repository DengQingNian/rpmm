<script setup lang="ts">
import { computed } from "vue";
import type { useDesktop } from "../composables/useDesktop";
import AppIcon from "./AppIcon.vue";
import MdAlert from "./MdAlert.vue";
import MdBadge from "./MdBadge.vue";
import MdButton from "./MdButton.vue";
import MdSelect from "./MdSelect.vue";
import MdSwitch from "./MdSwitch.vue";
import MdTextField from "./MdTextField.vue";

/** 应用设置：启动与运行偏好、数据目录与托盘行为。 */
const props = defineProps<{ desktop: ReturnType<typeof useDesktop> }>();

/** 开机自启开关：读后端真实注册状态，写入需要管理员授权。 */
const boot = computed({
  get: () => props.desktop.state.settings?.boot.installed ?? false,
  set: (value: boolean) => {
    void props.desktop.toggleBoot(value);
  },
});

/** 开机自启运行状态的中文说明。参数：无。返回：状态文案。 */
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

/** 开机自启账户说明，空账户表示系统账户。参数：无。返回：账户名。 */
const bootAccount = computed(() => props.desktop.state.settings?.boot.account || "LocalSystem");

/** 刷新间隔选项。参数：无。返回：下拉选项。 */
const refreshOptions = computed(() =>
  [...new Set([500, 1000, 2000, 5000, 10000, props.desktop.state.settingsDraft.refresh_ms])]
    .sort((a, b) => a - b)
    .map((value) => ({ label: value / 1000 + " 秒", value: String(value) })),
);

/** 二进制单位开关的字符串模型。参数：无。返回：下拉选项。 */
const locked = computed(() => props.desktop.locked.value || !props.desktop.state.settings);
</script>

<template>
  <div class="settings-layout">
    <section class="panel settings-panel">
      <div class="panel__head">
        <div>
          <h2 class="panel__title">启动与运行</h2>
          <p class="panel__subtitle">开机自启、登录启动与监控刷新频率</p>
        </div>
      </div>
      <form class="settings-form" @submit.prevent="desktop.saveSettings()">
        <article class="setting">
          <div class="setting__text">
            <div class="setting__title">
              <span>开机自启（无需登录）</span>
              <MdBadge
                :tone="desktop.state.settings?.boot.installed ? 'success' : 'neutral'"
                >{{ bootState }}</MdBadge
              >
            </div>
            <p>
              注册为 Windows 服务并由系统在启动时拉起，不需要任何用户登录；已启用的服务随守护进程一起启动。
              注册与取消需要管理员授权，被托管的子进程以 {{ bootAccount }} 账户运行在会话 0，不再继承登录用户的桌面环境。
            </p>
            <p v-if="desktop.state.settings?.daemon" class="setting__hint">
              当前控制台连接的是该守护进程；退出控制台不会停止它。
            </p>
            <p v-else-if="desktop.state.settings?.boot.installed" class="setting__hint">
              当前控制台仍在本机托管同一数据目录，守护进程将在下次系统启动时接管；立即移交请先退出控制台，再执行
              rpmm autostart start。
            </p>
          </div>
          <MdSwitch
            v-model="boot"
            label="开机自启"
            :disabled="locked || desktop.state.busy"
          />
        </article>

        <article class="setting">
          <div class="setting__text">
            <div class="setting__title"><span>登录后打开控制台</span></div>
            <p>当前用户登录 Windows 后自动启动控制台并隐藏到托盘，只影响登录会话，与开机自启相互独立。</p>
          </div>
          <MdSwitch
            v-model="desktop.state.settingsDraft.autostart"
            label="登录后打开控制台"
            :disabled="locked"
          />
        </article>

        <article class="setting">
          <div class="setting__text">
            <div class="setting__title"><span>手动启动时隐藏窗口</span></div>
            <p>启动后保持托盘运行，点击托盘图标打开控制台。</p>
          </div>
          <MdSwitch
            v-model="desktop.state.settingsDraft.start_hidden"
            label="手动启动时隐藏窗口"
            :disabled="locked"
          />
        </article>

        <article class="setting">
          <div class="setting__text">
            <div class="setting__title"><span>监控刷新间隔</span></div>
            <p>进程状态、健康状态和资源采样的刷新频率；日志刷新在日志面板单独设置。</p>
          </div>
          <MdSelect
            :model-value="String(desktop.state.settingsDraft.refresh_ms)"
            :options="refreshOptions"
            label="监控刷新间隔"
            width="120px"
            :disabled="locked"
            @update:model-value="desktop.state.settingsDraft.refresh_ms = Number($event)"
          />
        </article>

        <div class="settings-form__footer">
          <MdButton
            variant="filled"
            icon="save"
            type="submit"
            :loading="desktop.state.busy"
            :disabled="locked"
            >保存设置</MdButton
          >
        </div>
      </form>
    </section>

    <section class="panel settings-panel">
      <div class="panel__head">
        <div>
          <h2 class="panel__title">数据与托盘</h2>
          <p class="panel__subtitle">数据目录位置与退出行为</p>
        </div>
        <span class="panel__icon"><AppIcon name="folder-open" :size="22" /></span>
      </div>

      <div class="setting setting--stack">
        <div class="setting__text">
          <div class="setting__title"><span>数据目录</span></div>
          <p>包含 units 配置、state 状态和 logs 日志。保存后下次启动应用时使用新目录。</p>
        </div>
        <MdTextField
          v-model="desktop.state.settingsDraft.root"
          label="数据目录"
          placeholder="例如 C:\rpmm 或 /c/rpmm"
          prefix-icon="folder-open"
          mono
          :disabled="locked"
          :supporting="'当前使用：' + (desktop.state.settings?.root || '请通过桌面应用读取')"
        />
        <div class="setting__actions">
          <MdButton variant="outlined" icon="folder-open" :disabled="locked" @click="desktop.chooseRoot()"
            >选择文件夹</MdButton
          >
        </div>
        <p class="form-note">
          已有数据不自动搬迁，可通过配置导出和导入转移服务。显式 --root 参数优先。
        </p>
      </div>

      <div class="setting setting--stack">
        <div class="setting__text">
          <div class="setting__title"><span>托盘行为</span></div>
        </div>
        <MdAlert v-if="desktop.state.settings?.daemon" kind="info">
          关闭或最小化窗口会收起到托盘。当前由开机自启的守护进程托管子进程，退出控制台只关闭窗口，服务继续运行；停止服务请用
          rpmm autostart stop 或以管理员身份在服务管理器中停止。
        </MdAlert>
        <p v-else class="setting__text form-note">
          关闭或最小化窗口会收起到托盘，子进程持续运行。退出时按依赖顺序停止所有托管进程，清理残留进程树。
        </p>
        <div class="setting__actions">
          <MdButton
            variant="outlined"
            icon="quit"
            class="is-destructive-text"
            :disabled="desktop.locked.value"
            @click="desktop.quit()"
            >{{
              desktop.state.settings?.daemon ? "退出控制台" : "退出并停止所有子进程"
            }}</MdButton
          >
        </div>
      </div>
    </section>
  </div>
</template>
