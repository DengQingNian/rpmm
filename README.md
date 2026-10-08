# rpmm

使用 systemd 配置子集托管 Windows 子进程的 Tauri 2 桌面应用。桌面进程直接运行 Rust 管理器，提供进程监控、配置编辑、日志、依赖编排、自动重启和进程树清理。运行于当前登录用户的会话，不依赖 Windows 服务、服务账户或管理员权限。

支持 Windows 10/11 x64，桌面界面需要 Microsoft Edge WebView2 Runtime。目标程序应在前台运行，不应自行守护化；全部 unit 继承当前登录用户的账户、环境和权限。

构建需要 Rust 1.90 或更新版本、MSVC Windows 工具链和 Node.js 22.12 或更新版本。

桌面 UI 使用 Vue 3、TypeScript 和 Naive UI，通过 Vue 单文件组件组织监控、配置和设置页面，使用 `vue-tsc` 检查模板及脚本类型。共享后端操作集中在 `ui/composables/useDesktop.ts`，Naive UI 主题位于 `ui/theme.ts`，全局手绘样式位于 `ui/style.css`。切换页面不会丢失配置草稿，切换进程或文件前会确认未保存的修改；界面卸载时释放轮询和 Tauri 事件订阅。

界面图标使用 Lucide 的 Vue 图标库 `@lucide/vue`。`ui/icons.ts` 显式导入使用的图标，`ui/components/AppIcon.vue` 统一 SVG 尺寸、描边和无障碍属性。导航、统计卡片、进程列表及操作按钮均使用 SVG 图标，颜色继承文字或状态色，默认尺寸 18 px、描边 1.8；统计图标为 34 px、描边 1.5。图标由本地依赖打包，无需网络或图标字体。

界面采用纸张底色、铅笔灰、虚线边框和轻微倾斜。动效从 StyleKit legacy 动画集合适配：页面切换使用淡入上滑（220 ms、8 px、`cubic-bezier(0.16, 1, 0.3, 1)`），导航悬停及键盘焦点使用下划线绘制（180 ms、`cubic-bezier(0.4, 0, 0.2, 1)`），表单校验失败使用轻微抖动（240 ms、最大 4 px、`ease-in-out`）。使用 CSS 实现，无额外动画库；系统开启减少动态效果时禁用动画及组件过渡，周期刷新不重播入场动画。

Naive UI 在运行时生成组件样式，因此 Tauri 的 CSP 允许内联样式；脚本仍限制为本地资源。浏览器开发预览不连接真实管理器，进程操作需要通过桌面应用执行。

## 构建与验证

```powershell
npm ci
npm run tauri dev
# 构建桌面程序及当前用户安装包：
npm run tauri build
# 验证：
npm run build
npm run test:logs
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p rpmm --all-features
```

桌面程序位于 `target/release/rpmm-desktop.exe`，NSIS 安装包位于 `target/release/bundle/nsis/`。安装包按当前用户安装。单独构建 CLI 使用 `cargo build -p rpmm --release`，生成 `target/release/rpmm.exe`。`test-fixtures` 仅为 Windows 集成测试构建辅助程序；生产构建不需要此 feature。所有测试数据、调试日志保存在 `temp/`。

应用标识为 `com.dengqn.app.rpmm`。图标采用透明背景、蓝色圆角六边形核心与三个青色服务节点，体现统一编排多个服务。界面标志引用 `ui/assets/icon.svg`；页签和托盘使用同轮廓的单色加粗版本 `ui/assets/icon-small.svg`。修改后执行 `npm run icons` 更新 `src-tauri/icons/` 中的 PNG、ICO、ICNS 和商店资源，再执行 `npm run tauri build` 重新打包。ICO 包含 16/24/32/48/64/128/256 七档尺寸，托盘直接嵌入 32 px 单色 PNG。独立 CLI 的 `build.rs` 使用 `embed-resource` 把同一 ICO 嵌入 `rpmm.exe`，执行 `cargo build -p rpmm --release` 更新。生成过程的中间文件位于 `temp/icon-build/`，设计来源和生成提示词见 [图标说明](docs/icons.md)。

## 桌面使用

- **Dashboard**：显示托管、运行、失败和随应用启动的进程数量，以及宿主机 CPU、物理内存、磁盘读写速率和各卷容量。ECharts 显示最近 120 次采样趋势；首次采样不显示未经差分的速率。
- **进程监控**：显示子进程状态、PID、实例编号、重启次数、退出码、错误原因和启用状态。可启动、停止、重启、重置失败、启用或取消随应用启动；托管列表可折叠，详情随之扩展，并可通过选择框切换进程。
- **运行日志**：默认最近 200 条，可选 500/1000/10000 条；默认自动滚动、每秒刷新，可关闭滚动或选择暂停、0.5/1/2/5/10/30 秒刷新。支持来源筛选、内容搜索、可选正则和忽略大小写，以及类似 grep `-C` 的前后上下文（0～100 条）。相邻上下文范围自动合并，断组显示分隔符。搜索在所选最近记录内执行，行号代表该记录窗口的位置。虚拟列表支持 10000 条显示；全屏按钮铺满窗口，Esc 退出。下载通过系统保存对话框导出当前搜索及上下文结果，保留时间、来源和记录行号。
- **资源使用**：采集托管主进程 CPU、物理/虚拟内存、启动时间、进程 I/O 速率及累计量、打开句柄数、环境变量和 TCP/UDP 连接。Windows 句柄计数包含文件、线程等对象，当前不枚举文件路径；进程 I/O 可能包含网络和设备操作。环境优先读取运行进程，无法读取时显示启动配置与继承环境，并标明来源。切换进程或实例时清空趋势，无法获取的指标显示缺失或诊断。
- **健康检查**：后台按运行配置快照探测本机 TCP 端口或完整 HTTP/HTTPS URL；HTTP 仅直接响应 200 成功，不跟随重定向。默认超时 1 秒、间隔 10 秒，分别可设置为 (0,60] 秒和 1～3600 秒。每次探测持久化到 unit 的轮转日志，健康标签展示当前实例状态及最近 100 次历史记录。探测失败不触发自动重启；停止后不显示旧实例为当前健康状态。
- **子进程配置**：通过表单创建子进程，编辑 `.service` 主配置以及 `.service.d/*.conf` 覆盖文件。命令、环境变量、工作目录、依赖和重启策略均沿用原解析规则。
- **保存配置**：保存前校验完整候选图，包括仍在运行的实例依赖快照；校验失败不修改原文件和运行定义。编辑期间若文件被其他操作修改，保存会拒绝覆盖。配置编辑限制为不含 NUL 的 UTF-8 文本，最大 1 MiB；拒绝路径穿越、链接和重解析点。
- **保存并重启**：先保存并重载，再执行所选子进程的重启事务；如果重启失败，界面会说明配置已保存，同时保留失败诊断。重启 required unit 会传播到运行中的依赖者。
- **应用设置**：启用/取消登录自启动、设置手动启动时隐藏窗口、调整 0.5～10 秒的状态和资源监控刷新间隔；日志面板使用独立刷新设置。登录自启动始终隐藏到托盘，并保留当前 `--root` 目录。
- **托盘运行**：关闭和最小化窗口都收起到托盘，子进程继续运行。左键点击托盘恢复窗口；右键菜单可打开控制台或退出。应用设置中也可明确退出，退出时等待子进程按依赖逆序停止并清理进程树。

自启动指**当前用户登录 Windows 后启动**，无人登录时不运行。首次使用默认不开启系统自启动；子进程是否随 rpmm 启动由各自的启用状态决定，与 rpmm 是否随登录启动分别设置。主窗口启动和重复打开由单实例插件协调；桌面管理器与 CLI 前台管理器通过按 root 隔离的独占管道避免重复托管。

```powershell
# 指定现有配置目录：
target/release/rpmm-desktop.exe --root C:/Users/yourname/AppData/Local/rpmm
# 隐藏到托盘启动：
target/release/rpmm-desktop.exe --hidden
# 在隔离目录里开发：
npm run tauri dev -- -- --root F:/proj/rpmm/temp/demo
```

## 前台运行

桌面和 CLI 默认数据目录均为 `%LOCALAPPDATA%/rpmm`，可以通过 `--root` 指定独立目录。目录包含 `units/`（期望配置）、`state/enabled.json`（子进程启用状态）、`state/desktop.json`（界面设置）、`logs/`（运行日志）；更改 root 不会搬迁已有配置。

在项目目录中运行：

```powershell
New-Item -ItemType Directory -Force temp/demo/units | Out-Null
Copy-Item examples/units/* temp/demo/units -Recurse
target/release/rpmm.exe --root temp/demo verify
target/release/rpmm.exe --root temp/demo manager run
```

示例使用 `C:/Windows` 下的 PowerShell；Windows 安装在其他驱动器时请先修改路径。管理器窗口保持运行，另开一个 PowerShell：

```powershell
target/release/rpmm.exe --root temp/demo start demo.service
target/release/rpmm.exe --root temp/demo status
target/release/rpmm.exe --root temp/demo logs demo.service --follow --source stdout
target/release/rpmm.exe --root temp/demo enable demo.service
target/release/rpmm.exe --root temp/demo stop prepare.service
```

`demo.service` 依赖 `prepare.service`，显式停止后者会传播到前者。Ctrl+C 关闭前台管理器并清理所有托管进程树。桌面应用本身已运行管理器，CLI 可连接到相同 root；同一 root 不要再启动 `manager run`。`manager stop` 通过本机管道请求退出该 root 的管理器（包括桌面应用），由管理器完成子进程清理；命令返回表示请求已受理。

## 从服务版迁移

此版本移除了 Windows 服务安装、卸载、控制和 `service-host` 入口，同时移除了服务账户密码、服务登录权限和系统服务依赖。

若机器已安装旧版服务，应先使用**旧版二进制**执行 `manager stop` 和 `manager uninstall`，再运行桌面版。新版本不会修改已有系统服务。将旧数据目录中的 `units/` 和 `state/enabled.json` 复制到用户数据目录，或者使用 `--root` 指向当前用户有读写权限的旧目录。旧版服务专用 ACL 可能使普通用户无法写入，需要先由目录所有者授予权限或复制到用户目录。

迁移后所有子进程使用当前用户账户；原账户的环境、文件权限和网络权限不会自动迁移。保留 `.service` 文件及原来的依赖、自动重启和日志格式。单实例桌面应用一次只使用一个 root；重复打开会恢复已有窗口，切换 root 需要先退出。命名管道授权当前账户、Administrators 和 SYSTEM，拒绝远程客户端。

## 配置及操作语义

unit 名称大小写必须与依赖引用一致；名称采用普通 ASCII 文件名，Windows 保留名称、模板、别名和其他 unit 类型均拒绝。配置支持 UTF-8（允许 BOM）、CRLF、整行 `#`/`;` 注释、重复节、续行、重复指令和同名 `unit.service.d/*.conf` 按文件名排序合并。值里的 `#`/`;` 不作为行内注释。

命令首先按 systemd 引号和转义规则生成 argv，再按 Windows 标准命令行规则编码。建议使用 `C:/...` 路径；反斜杠必须按 systemd 规则写成 `\\`。不自动经过 shell，PowerShell/Python/批处理必须显式调用解释器。支持 `${VAR}`（一个参数内展开）、独立 `$VAR`（分词展开）、`$$`（字面美元符）、`%n`、`%%`；`$MAINPID` 仅在停止命令中提供。PowerShell 自己的变量需写成 `$$变量`，例如 `$$true` 和 `$$env:MESSAGE`。

`Environment=` 保留继承环境并覆盖指定变量；Windows 环境变量名合并和查询时不区分大小写，同名覆盖按最后一条指令生效。环境值不再做 `$` 展开。独立 `$VAR` 使用 systemd 的分词、去引号和普通反斜杠处理；传递 Windows 路径时优先使用 `${VAR}`，保留空格及反斜杠。未指定 `WorkingDirectory` 时使用 Windows 系统目录；所有可执行文件和显式工作目录必须为 Windows 绝对路径。

`Requires`/`Wants` 拉入启动任务；`After`/`Before` 只排序同一事务内已有任务。仅写 `After` 不会启动引用对象。强依赖启动失败只有在同时排序在前时阻止依赖者；弱依赖失败不阻止它。需求闭包去重，排序环路拒绝；未成功启动的根返回失败，但成功依赖不回滚。显式停止/重启 required unit 会向依赖者传播；依赖自己退出不会触发传播。

`enable`/`disable` 只保存 `WantedBy=multi-user.target` 成员关系；不会立即启停。管理器启动后激活 enabled 集合。`daemon-reload` 校验候选图后原子替换，失败保留旧配置；运行实例保持启动时的配置和依赖快照，新定义在下一次启动时生效。运行中的 unit 不能从配置目录删除。

重启主进程前先清理其残留进程树。默认重启策略为 `no`，延迟为 100 ms，10 秒内最多启动 5 次（首次、手动和自动启动都计入）。限流进入 `failed/start-limit-hit`，可用 `reset-failed` 清除。`StartLimitIntervalSec=0` 或 `StartLimitBurst=0` 禁用限流。

启动、停止和重启通过同一事务锁协调，独立启动层可以并行执行；状态查询和日志读取单独处理。停止请求与正在启动的事务相交时，会取消该事务尚在激活的实例，再按逆序停止目标闭包；已成功启动的无关依赖保留。主动停止和管理器关闭不触发 unit 自动重启。

`Type=simple/exec` 在 Windows 创建及恢复主进程后认为启动成功，**不表示端口、数据库或业务已就绪**。`oneshot` 顺序执行命令，只有全成功才完成激活；`RemainAfterExit=yes` 成功后保持 active/exited。非 oneshot 默认启动超时为 90 秒，oneshot 默认为无限，默认停止超时为 90 秒。

成功启动的实例结束时执行 `ExecStop`；启动失败时跳过。停止命令应同步完成应用关闭请求；每条停止命令受 `TimeoutStopSec` 约束，完成或超时后清理残留进程树。**没有 `ExecStop` 时直接终止 Job，不模拟 SIGTERM。** unit 重启由管理器执行，正常退出不触发重启。桌面管理器自身故障时由 Job 句柄关闭清理进程树，不自动重启桌面应用。

## 日志和协议

健康检查是 rpmm 的扩展配置，在 `[Service]` 中使用 `HealthType=none/tcp/http`、`HealthPort`、`HealthUrl`、`HealthTimeoutSec` 和 `HealthIntervalSec`。配置页提供表单将指令写入当前文档草稿；保存时统一校验，运行进程重启后应用新设置。例如：

```ini
[Service]
HealthType=http
HealthUrl=http://127.0.0.1:8080/health
HealthTimeoutSec=1s
HealthIntervalSec=10s
```

TCP 配置改为 `HealthType=tcp`、`HealthPort=8080`，目标固定为 `127.0.0.1`。`HealthType=none` 禁用探测。默认不启用；oneshot 无主进程时不探测。每次探测以 `health` 来源写入 JSONL，可使用 `logs <unit> --source health` 查询，遵循相同轮转策略。

每个 unit 的 stdout/stderr 和生命周期事件写入 `logs/<unit>.jsonl`，带 UTC 时间、unit、实例编号、来源和内容。输出按有界块读取，长行可能分成多条记录；Windows 非 UTF-8 输出使用替代字符显示。建议应用主动输出 UTF-8。管理器整体事件在 `logs/manager.jsonl`，可直接通过 `Get-Content -Wait` 查看。

单文件达到 10 MiB 时轮转，保留 5 个历史文件。`logs --lines N` 返回最近记录（上限 10000），`--follow` 接续实时记录；不输出整份环境或密码。磁盘错误被诊断，输出管道仍继续排空；慢客户端超过 256 条实时记录缓冲时得到 `log-stream-lagged`，重新查询历史即可。

`--json` 输出机器可读状态或逐条 JSON 日志。IPC 协议版本为 1，每行一个 JSON 对象，单帧上限 1 MiB；日志逐条发送。响应包含 `version/ok/code/data`，错误码包括 `invalid-config`、`operation-failed`、`io-error`、`invalid-protocol`、`unsupported-version`、`log-stream-lagged`。状态字段包含 state/substate、pid、instance、exit_code、reason、restart_count、config_version 和 enabled。

示例请求：`{"version":1,"action":"status","unit":"demo.service"}`。兼容指令及已知差异见 [兼容性矩阵](docs/compatibility.md)。

## 桌面验收

在 `temp/demo` 中使用示例配置进行验收：启动/停止/重启依赖进程、筛选日志、保存主文件和覆盖文件、确认错误配置不落盘、确认编辑冲突不会覆盖文件。窗口关闭或最小化后应继续托管；托盘恢复窗口应显示相同实例；明确退出后所有进程树应停止。

在安装到稳定路径的桌面应用中开启登录自启动，重新登录后确认应用隐藏到托盘且 enabled 集合被激活；关闭自启动后再次登录确认应用不再自动启动。开发目录和临时二进制不适合持久自启动。完整登录/注销验收需要用户实际重新登录，不由普通测试改变系统会话。
