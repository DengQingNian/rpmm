# systemd 兼容性矩阵

本项目实现明确的配置子集，不承诺完整 systemd 运行环境。依据 [systemd.syntax](https://github.com/systemd/systemd/blob/main/man/systemd.syntax.xml)、[systemd.unit](https://github.com/systemd/systemd/blob/main/man/systemd.unit.xml)、[systemd.service](https://github.com/systemd/systemd/blob/main/man/systemd.service.xml) 的相关指令语义。

| 能力 | 状态 | 首版行为 |
|---|---|---|
| UTF-8、CRLF、整行注释、重复节、续行 | 兼容 | 保留顺序、诊断带来源位置；续行间注释跳过 |
| 引号、C 转义、布尔值、组合时间跨度 | 兼容子集 | 不允许未知转义/NUL；时间支持秒、微秒、毫秒、分、时、日、周、月、年 |
| 同名 `.service.d/*.conf` | 兼容 | 文件名排序合并；不搜索模板、前缀级或类型级 drop-in |
| 重复指令与空值 | 兼容子集 | Exec 列表及 Environment 可清空；依赖空值不删除已追加依赖；标量恢复默认 |
| `X-` 节/指令 | 兼容 | 忽略自定义元数据 |
| 普通未知/未支持指令 | 有意更严格 | 配置失败；systemd 通常警告后忽略 |
| Description | 兼容子集 | 支持 `%n`、`%%` |
| Requires、Wants、After、Before | 兼容子集 | rpmm 内部需求与排序分开；不引用 SCM 外部服务或 Linux target |
| StartLimitIntervalSec、StartLimitBurst | 兼容子集 | 滑动窗口；Interval 为有限时间，任一设 0 禁用 |
| Type=simple/exec | Windows 近似 | 创建并恢复 Windows 进程即激活；两者在此平台使用相同边界 |
| Type=oneshot、RemainAfterExit | 兼容子集 | 顺序多命令；成功后可保留 active/exited；首版仍要求合理的启动/停止组合 |
| ExecStart、ExecStop | 兼容子集/Windows 近似 | systemd 分词及变量展开；Windows 绝对可执行路径；标准 Windows argv 编码 |
| 命令环境变量、specifier | 兼容子集 | `${VAR}`、独立 `$VAR`、`$$`、停止时 MAINPID、`%n`、`%%` |
| Environment、WorkingDirectory | Windows 近似 | 继承管理器环境，覆盖 Windows 变量；未指定目录使用 SystemRoot/System32 |
| Restart | 兼容子集 | no、always、on-failure、on-success；非零退出、启动错误、超时视为失败；不实现 POSIX 信号分类 |
| RestartSec、TimeoutStartSec、TimeoutStopSec | 兼容子集 | RestartSec 必须有限；启动/停止超时可 infinity；停止超时逐条应用 |
| HealthType、HealthPort、HealthUrl、HealthTimeoutSec、HealthIntervalSec | rpmm 扩展 | TCP 探测本机端口；HTTP/HTTPS 直接响应 200 成功；默认超时 1 秒、间隔 10 秒，记录历史但不自动重启 |
| WantedBy=multi-user.target | Windows 近似 | 保存启用状态；管理器启动时拉起成员；不创建 Linux symlink |
| 进程组清理 | Windows 近似 | 挂起创建后加入 Job；禁止 breakaway；关闭 Job 清理进程树 |
| 停止信号 | Windows 差异 | 使用 ExecStop 或终止 Job；没有 SIGTERM、SIGKILL、KillSignal 映射 |
| 配置重载 | 兼容意图/首版限制 | 原子校验；运行实例完整保留旧配置及依赖快照；不自动重启应用 |
| Linux 用户/组、User、Group | 暂不支持 | 所有 unit 继承当前登录用户账户；不静默忽略 |
| EnvironmentFile、ExecStartPre/Post、ExecStopPost、ExecReload | 暂不支持 | 明确拒绝，防止部分执行被误认为完整兼容 |
| 命令前缀、其他 specifier、模板和别名 | 暂不支持 | 包括 `-`、`+`、`!`、`@` 前缀及实例模板 |
| network.target/network-online.target | 暂不支持 | 不把网络排序伪装成网络已就绪 |
| notify/forking/dbus/idle 等 Type | 暂不支持 | 没有应用级就绪协议和主进程推断 |
| Socket、Timer、其他自定义 target | 暂不支持 | 后续独立扩展；首版仅普通 .service |
| CPU/内存配额、Linux 沙箱和网络隔离 | 暂不支持 | Job 首版用于生命周期，未实现资源指令或网络安全边界 |
| journald/Event Log | Windows 替代 | JSONL 文件、GUI 和 CLI；不接 Event Log |
| GUI 和交互桌面 | 已支持 | Tauri 2 监控、配置编辑；子进程处于当前用户会话 |
| Windows 服务 | 已移除 | 桌面进程直接监督；不注册服务或授予服务登录权限 |
| 开机启动 | 当前用户登录启动 | 当前用户 Run 登记；隐藏到托盘后激活 enabled 集合 |
| 最小化、窗口关闭 | 常驻托盘 | 仅明确退出时有序停止子进程；托盘点击恢复窗口 |

Windows 机制参考：[Job Object](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)、[命名管道访问控制](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)、[Tauri 托盘](https://v2.tauri.app/learn/system-tray/)、[Run 启动登记](https://learn.microsoft.com/en-us/windows/win32/setupapi/run-and-runonce-registry-keys)。
