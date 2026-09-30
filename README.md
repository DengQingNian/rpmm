# rpmm

使用 systemd 配置子集托管普通 Windows 程序的 Rust 服务管理器。SCM 只运行一个 `rpmm` 服务，内部管理多个 `.service`，提供依赖编排、自动重启、文件日志及本机 CLI。

支持 Windows 10/11、Windows Server 2019 及以上的 x64 系统。目标程序应在前台运行，不应自行守护化或依赖登录桌面；全部 unit 继承管理器账户。

构建需要 Rust 1.88 或更新版本及 MSVC Windows 工具链。

## 构建与验证

```powershell
cargo build --release
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo test --all-features
```

生产程序位于 `target/release/rpmm.exe`。`test-fixtures` 仅为 Windows 集成测试构建辅助程序；生产构建不需要此 feature。普通测试不注册系统服务。所有测试数据、调试日志保存在 `temp/`，便于检查失败现场。

## 前台运行

默认数据目录为 `%ProgramData%/rpmm`，可以通过 `--root` 指定独立目录。目录包含 `units/`（期望配置）、`state/`（启用状态）、`logs/`（运行日志）；更改 root 不会搬迁已有配置。

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

`demo.service` 依赖 `prepare.service`，显式停止后者会传播到前者。Ctrl+C 关闭前台管理器并清理所有托管进程树。`manager start/stop/status` 针对 SCM 注册；前台管理器通过原窗口的 Ctrl+C 关闭。

## 安装为 Windows 服务

使用管理员 PowerShell，将生产二进制放到稳定路径并准备好配置。安装要求指定带密码的普通 Windows 用户账户；所有 unit 共用它。服务账户还必须有权读取被托管程序、其脚本与依赖文件，并写入应用自己的数据目录。本工具只配置 rpmm 数据目录权限，不修改应用目录权限。

```powershell
C:/Tools/rpmm/rpmm.exe --root C:/ProgramData/rpmm verify
C:/Tools/rpmm/rpmm.exe --root C:/ProgramData/rpmm manager install --account '.\serviceuser'
C:/Tools/rpmm/rpmm.exe --root C:/ProgramData/rpmm manager start
C:/Tools/rpmm/rpmm.exe --root C:/ProgramData/rpmm enable demo.service
C:/Tools/rpmm/rpmm.exe --root C:/ProgramData/rpmm start demo.service
```

安装交互式读取密码，不在命令行、unit 文件或日志里保存密码。自动化可用 `--password-stdin` 通过标准输入提供一行密码。工具解析账户 SID、授予 `SeServiceLogonRight`、验证服务登录，并注册自动启动服务；密码交由 Windows 服务配置处理。安装失败恢复本次改动的项目 DACL、服务注册和新增登录授权。

安装将数据目录权限设置为 SYSTEM/Administrators 完全控制，服务账户读取配置、修改 state/logs。目录里不应放置与 rpmm 无关的资料，也不能包含链接。用户、组或域策略中的拒绝服务登录权限不能通过允许权限覆盖，安装会返回 Windows 登录错误。

SCM 服务名固定为 `rpmm`，同一台机器只安装一个 SCM 管理器；不同 root 可以运行各自的前台管理器。本机命名管道按规范 root 隔离，同一 root 不能并行运行两个管理器；管道只授权当前管理器账户、Administrators 和 SYSTEM，拒绝远程客户端。

```powershell
C:/Tools/rpmm/rpmm.exe --root C:/ProgramData/rpmm manager stop
C:/Tools/rpmm/rpmm.exe --root C:/ProgramData/rpmm manager uninstall
```

卸载保留 unit、enabled 状态和日志，并移除该次安装新增的账户登录权限。已有登录权限不移除；数据目录 ACL 保留。卸载后更改账户名/SID导致无法恢复授权时，会给出明确诊断。

## 配置及操作语义

unit 名称大小写必须与依赖引用一致；名称采用普通 ASCII 文件名，Windows 保留名称、模板、别名和其他 unit 类型均拒绝。配置支持 UTF-8（允许 BOM）、CRLF、整行 `#`/`;` 注释、重复节、续行、重复指令和同名 `unit.service.d/*.conf` 按文件名排序合并。值里的 `#`/`;` 不作为行内注释。

命令首先按 systemd 引号和转义规则生成 argv，再按 Windows 标准命令行规则编码。建议使用 `C:/...` 路径；反斜杠必须按 systemd 规则写成 `\\`。不自动经过 shell，PowerShell/Python/批处理必须显式调用解释器。支持 `${VAR}`（一个参数内展开）、独立 `$VAR`（分词展开）、`$$`（字面美元符）、`%n`、`%%`；`$MAINPID` 仅在停止命令中提供。PowerShell 自己的变量需写成 `$$变量`，例如 `$$true` 和 `$$env:MESSAGE`。

`Environment=` 保留继承环境并覆盖指定变量；Windows 环境变量名合并和查询时不区分大小写，同名覆盖按最后一条指令生效。环境值不再做 `$` 展开。独立 `$VAR` 使用 systemd 的分词、去引号和普通反斜杠处理；传递 Windows 路径时优先使用 `${VAR}`，保留空格及反斜杠。未指定 `WorkingDirectory` 时使用 Windows 系统目录；所有可执行文件和显式工作目录必须为 Windows 绝对路径。

`Requires`/`Wants` 拉入启动任务；`After`/`Before` 只排序同一事务内已有任务。仅写 `After` 不会启动引用对象。强依赖启动失败只有在同时排序在前时阻止依赖者；弱依赖失败不阻止它。需求闭包去重，排序环路拒绝；未成功启动的根返回失败，但成功依赖不回滚。显式停止/重启 required unit 会向依赖者传播；依赖自己退出不会触发传播。

`enable`/`disable` 只保存 `WantedBy=multi-user.target` 成员关系；不会立即启停。管理器启动后激活 enabled 集合。`daemon-reload` 校验候选图后原子替换，失败保留旧配置；运行实例保持启动时的配置和依赖快照，新定义在下一次启动时生效。运行中的 unit 不能从配置目录删除。

重启主进程前先清理其残留进程树。默认重启策略为 `no`，延迟为 100 ms，10 秒内最多启动 5 次（首次、手动和自动启动都计入）。限流进入 `failed/start-limit-hit`，可用 `reset-failed` 清除。`StartLimitIntervalSec=0` 或 `StartLimitBurst=0` 禁用限流。

启动、停止和重启通过同一事务锁协调，独立启动层可以并行执行；状态查询和日志读取单独处理。停止请求与正在启动的事务相交时，会取消该事务尚在激活的实例，再按逆序停止目标闭包；已成功启动的无关依赖保留。主动停止和管理器关闭不触发 unit 自动重启。

`Type=simple/exec` 在 Windows 创建及恢复主进程后认为启动成功，**不表示端口、数据库或业务已就绪**。`oneshot` 顺序执行命令，只有全成功才完成激活；`RemainAfterExit=yes` 成功后保持 active/exited。非 oneshot 默认启动超时为 90 秒，oneshot 默认为无限，默认停止超时为 90 秒。

成功启动的实例结束时执行 `ExecStop`；启动失败时跳过。停止命令应同步完成应用关闭请求；每条停止命令受 `TimeoutStopSec` 约束，完成或超时后清理残留进程树。**没有 `ExecStop` 时直接终止 Job，不模拟 SIGTERM。** SCM 只恢复管理器故障（5 秒后重启），unit 重启由管理器执行，正常关闭不会触发恢复。

## 日志和协议

每个 unit 的 stdout/stderr 和生命周期事件写入 `logs/<unit>.jsonl`，带 UTC 时间、unit、实例编号、来源和内容。输出按有界块读取，长行可能分成多条记录；Windows 非 UTF-8 输出使用替代字符显示。建议应用主动输出 UTF-8。管理器整体事件在 `logs/manager.jsonl`，可直接通过 `Get-Content -Wait` 查看。

单文件达到 10 MiB 时轮转，保留 5 个历史文件。`logs --lines N` 返回最近记录（上限 10000），`--follow` 接续实时记录；不输出整份环境或密码。磁盘错误被诊断，输出管道仍继续排空；慢客户端超过 256 条实时记录缓冲时得到 `log-stream-lagged`，重新查询历史即可。

`--json` 输出机器可读状态或逐条 JSON 日志。IPC 协议版本为 1，每行一个 JSON 对象，单帧上限 1 MiB；日志逐条发送。响应包含 `version/ok/code/data`，错误码包括 `invalid-config`、`operation-failed`、`io-error`、`invalid-protocol`、`unsupported-version`、`log-stream-lagged`。状态字段包含 state/substate、pid、instance、exit_code、reason、restart_count、config_version 和 enabled。

示例请求：`{"version":1,"action":"status","unit":"demo.service"}`。兼容指令及已知差异见 [兼容性矩阵](docs/compatibility.md)。

## 管理员验收

`scripts/scm-acceptance.ps1` 仅在管理员明确执行时注册服务。它要求当前不存在 `rpmm` 服务，可使用临时账户或指定凭据；程序及全部测试资料放在项目 `temp/`。覆盖安装、指定账户启动、两 unit 的依赖、日志、enabled 恢复、失败重启/限流、reset 和卸载。

```powershell
./scripts/scm-acceptance.ps1 -CreateTestAccount
# 或使用现有账户：
./scripts/scm-acceptance.ps1 -Credential (Get-Credential)
```

脚本清理自己创建的 SCM 注册和临时账户；清理失败时保留工作目录供排障。真实管理员账户验收与普通测试分开，普通测试不会改动系统服务或账户权限。
