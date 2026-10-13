# rpmm

使用 systemd 配置子集托管 Windows 子进程的 Tauri 2 桌面应用，内置 Rust 管理器，提供进程监控、配置编辑、运行日志、健康检查、依赖编排、自动重启和进程树清理；支持 Windows 10/11 x64，Windows Server 同样可以运行。

管理器有两种运行方式：默认在当前登录用户的会话中由桌面程序直接托管，不需要 Windows 服务、服务账户或管理员权限；也可以注册为开机自启的 Windows 服务，由系统在启动时拉起守护进程，不需要任何用户登录，适合无人值守的服务器。

配置沿用 systemd 的 `.service` 主文件与 `.service.d/*.conf` 覆盖文件，支持 `Requires`/`Wants`/`After`/`Before`/`HealthAfter` 依赖、`Restart` 重启策略、`StandardOutputDirectory` 输出目录、`MemoryMax`/`CPUQuota` 资源上限和 `HealthType` 健康探测；命令不经过 shell，路径统一归一化为 Windows 绝对路径。目标程序应在前台运行，不应自行守护化。由桌面程序托管时全部子进程继承当前登录用户的账户、环境和权限；注册开机自启后由守护进程在会话 0 派生，默认使用 LocalSystem 账户。

## 安装

从 [Releases](https://github.com/DengQingNian/rpmm/releases) 下载最新版安装包，按当前用户安装，不需要管理员权限：

- `rpmm_<版本>_x64-setup.exe`：桌面程序，含托盘、监控和配置界面。
- `rpmm-cli_<版本>_x64.zip`：命令行程序，解压后含 rpmm.exe 与示例配置。
- `SHA256SUMS.txt`：全部产物的 SHA-256 校验和。

注册开机自启需要管理员权限，桌面程序与命令行程序都能完成登记：界面开关以 `rpmm-desktop.exe --service` 作为服务入口，命令行用 `rpmm.exe autostart run`。默认按当前用户安装的程序位于用户配置目录，服务虽然可以读取，但删除用户配置文件或卸载后登记会失效；服务器上建议把 `rpmm.exe` 放到 `C:\Program Files\rpmm` 之类的机器级目录后再注册。

界面依赖 Microsoft Edge WebView2 Runtime。Windows 11 和较新的 Windows 10 通常已随系统或 Edge 提供；**老旧 Windows 10 或 Windows Server 如果缺少该运行时，界面会无法创建窗口**，请先安装仓库内的 x64 离线运行时：

```powershell
# 图形安装
docs\MicrosoftEdgeWebView2RuntimeInstallerX64.exe
# 无交互安装（适合脚本部署）
docs\MicrosoftEdgeWebView2RuntimeInstallerX64.exe /silent /install
```

离线运行时约 200 MiB，由 Git LFS 托管，克隆或拉取仓库后需执行 `git lfs pull` 才能取得完整文件（未安装 Git LFS 时它只是一个指针文件）。NSIS 安装包默认在安装时联网下载 WebView2 引导程序，内网或离线机器请先手工安装该运行时。

从源码构建需要 Rust 1.90 或更新版本、MSVC Windows 工具链和 Node.js 22.12 或更新版本：

```powershell
npm ci
npm run tauri build                       # 桌面程序与当前用户安装包
cargo build -p rpmm --release             # 仅命令行程序
npm run build                             # 前端类型检查与打包
```

桌面程序位于 `target/release/rpmm-desktop.exe`，NSIS 安装包位于 `target/release/bundle/nsis/`。应用标识为 `com.dengqn.app.rpmm`。

## 使用说明

### 桌面应用

- **Dashboard**：显示托管、运行、失败和随应用启动的服务数量，以及宿主机 CPU、物理内存、磁盘读写速率和各卷容量，ECharts 展示最近 120 次采样的趋势。
- **进程监控**：显示服务状态、PID、实例编号、重启次数、退出码、错误原因和启用状态，可启动、停止、重启、重置失败、启用或取消随应用启动；详情标签展示运行日志、资源使用（CPU、内存、进程 I/O、句柄、环境变量、TCP/UDP 连接）和健康检查历史。
- **运行日志**：默认最近 200 条，可选 500/1000/10000 条；支持自动滚动和 0.5/1/2/5/10/30 秒刷新、来源筛选、内容搜索、正则、忽略大小写，以及类似 grep `-C` 的前后上下文（0～100 条）。可通过系统保存对话框导出当前搜索结果，全屏按钮铺满窗口、Esc 退出。
- **服务配置**：通过表单新建服务，编辑 `.service` 主配置及 `.service.d/*.conf` 覆盖文件，并用表单写入前置/后置/强弱依赖、等待健康的前置服务、标准输出目录、资源上限和健康检查。配置目录按分类分组，把服务拖到分类标题即可改变分类，行尾按钮提供同样的移动菜单；分类只影响界面组织，保存在 `state/categories.json`，不写入 `.service` 文件。可多选服务导出 JSON 配置包，或导入配置包（整包校验后提交，默认拒绝同名服务，可选择替换）。保存前校验完整依赖图与磁盘冲突，运行实例保留启动时的配置，重启后生效。
- **删除服务**：可删除当前服务的主配置、全部覆盖文件和启用状态。运行中的实例需先停止，仍被其他服务引用时会拒绝删除并列出引用者。
- **应用设置**：分别设置开机自启（注册 Windows 服务，无人登录也能运行，需要管理员授权）、登录后打开控制台、手动启动时隐藏窗口、0.5～10 秒的状态与资源监控刷新间隔，以及数据目录（接受 `C:\...`、`C:/...`、`/c/...` 等写法，下次启动生效）。
- **托盘运行**：关闭和最小化窗口都收起到托盘，服务继续运行；左键点击托盘恢复窗口，右键菜单可打开控制台或退出。自己托管时退出会按依赖逆序停止全部服务并清理进程树；连接守护进程时退出只关闭控制台。

自启动有两套彼此独立的机制，首次使用都默认不开启：**开机自启**把管理器注册为自动启动的 Windows 服务，由系统在启动时拉起守护进程，不需要任何用户登录；**登录后打开控制台**只在当前用户登录 Windows 后启动图形界面。服务是否随 rpmm 启动只由各自的启用状态决定，与这两种自启动如何设置无关；详细的注册方式、账户与会话差异见下面的“开机自启（无需登录）”。

### 开机自启（无需登录）

服务器重启后无人登录也要把服务拉起来时，把管理器注册为自动启动的 Windows 服务：

```powershell
# 需要管理员权限：把当前程序与当前数据目录注册为开机自启
rpmm.exe --root C:\ProgramData\rpmm autostart install
rpmm.exe autostart status                 # 注册、运行状态与账户
rpmm.exe autostart start                  # 立即启动，不必等重启
rpmm.exe autostart stop                   # 按依赖逆序停止全部子进程后退出
rpmm.exe --root C:\ProgramData\rpmm autostart uninstall
# 需要用户配置文件或网络凭据时指定账户
rpmm.exe --root C:\ProgramData\rpmm autostart install --account "DOMAIN\svc-rpmm" --password "密码"
```

服务由服务控制管理器在系统启动时拉起，不依赖任何用户登录；守护进程启动后激活 enabled 集合，所以服务是否随 rpmm 启动仍然只由各自的启用状态决定。失败恢复策略在前三次崩溃或非零退出后分别等待 5、10、30 秒重启，一天后重置计数；也可以用 services.msc 或 sc.exe 管理。重复执行 `install` 会按当前程序路径与数据目录更新登记，不会产生第二个服务。

默认以 **LocalSystem** 账户运行在会话 0：没有交互桌面，子进程不再继承登录用户的环境、用户配置文件和映射驱动器；需要这些时用 `--account` 指定账户。数据目录建议放在 `C:\ProgramData\rpmm` 之类的机器级位置，指向当前用户配置文件时会给出提醒。注册、删除、启动和停止都需要管理员权限，未提升时命令会直接报告而不会留下半成品登记。

桌面程序与守护进程共用同一数据目录时只允许一个管理器。桌面程序启动时先探测命名管道：发现守护进程就以客户端连接，设置页会提示“当前控制台连接的是该守护进程”，此时退出控制台只关闭窗口；没有守护进程时桌面程序仍然自己托管，保持不需要管理员的便携用法。在本机自行托管时打开开机自启开关，守护进程要等控制台退出或下次系统启动才会接管。

### 前台运行

桌面和命令行程序首次运行都使用默认数据目录 `%LOCALAPPDATA%/rpmm`，可以用 `--root` 指定独立目录；`C:\...`、`C:/...`、`/c/...`、`/cygdrive/c/...` 都按 Windows 绝对路径归一化。目录包含 `units/`（配置）、`state/`（启用状态、界面设置和分类）和 `logs/`（运行日志）。

```powershell
New-Item -ItemType Directory -Force temp/demo/units | Out-Null
Copy-Item examples/units/* temp/demo/units -Recurse
target/release/rpmm.exe --root temp/demo verify
target/release/rpmm.exe --root temp/demo manager run
```

示例使用 `C:/Windows` 下的 PowerShell，Windows 安装在其他驱动器时请先修改路径。管理器窗口保持运行，另开一个 PowerShell：

```powershell
target/release/rpmm.exe --root temp/demo start demo.service
target/release/rpmm.exe --root temp/demo status
target/release/rpmm.exe --root temp/demo logs demo.service --follow --source stdout
target/release/rpmm.exe --root temp/demo enable demo.service
target/release/rpmm.exe --root temp/demo stop prepare.service
# 删除服务配置，运行中的实例需要显式停止：
target/release/rpmm.exe --root temp/demo delete demo.service --stop
```

`demo.service` 依赖 `prepare.service`，显式停止后者会传播到前者。Ctrl+C 关闭前台管理器并清理所有托管进程树；`--json` 输出机器可读状态或逐条 JSON 日志。桌面应用本身已运行管理器，CLI 可连接到相同 root，但同一 root 不要再启动 `manager run`。

桌面程序也接受相同的 `--root`，还可以用 `--hidden` 隐藏到托盘启动；开发时在隔离目录里运行：

```powershell
target/release/rpmm-desktop.exe --root C:/Users/yourname/AppData/Local/rpmm
target/release/rpmm-desktop.exe --hidden
npm run tauri dev -- -- --root F:/proj/rpmm/temp/demo
```

全部支持的配置指令及与 systemd 的已知差异见 [兼容性矩阵](docs/compatibility.md)。
