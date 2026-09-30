#Requires -RunAsAdministrator
<#
.SYNOPSIS
显式执行 Windows SCM 完整验收，并清理本次创建的服务和临时账户。
.PARAMETER CreateTestAccount
创建独立的临时普通本地账户，结束时删除。
.PARAMETER Credential
使用现有服务账户凭据；不会删除此账户，卸载仅恢复本次新增登录授权。
.PARAMETER Binary
待验收的生产 rpmm.exe，默认项目 target/release/rpmm.exe。
#>
[CmdletBinding(DefaultParameterSetName = 'Create')]
param(
    [Parameter(Mandatory, ParameterSetName = 'Create')][switch]$CreateTestAccount,
    [Parameter(Mandatory, ParameterSetName = 'Existing')][pscredential]$Credential,
    [string]$Binary = (Join-Path $PSScriptRoot '../target/release/rpmm.exe')
)
$ErrorActionPreference = 'Stop'
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$temporaryRoot = [IO.Path]::GetFullPath((Join-Path $workspace 'temp'))
$testRoot = [IO.Path]::GetFullPath((Join-Path $temporaryRoot ('scm-' + [guid]::NewGuid().ToString('N'))))
if (Get-Service -Name rpmm -ErrorAction SilentlyContinue) { throw '已存在 rpmm 服务，验收拒绝覆盖。' }
if (-not (Test-Path -LiteralPath $Binary -PathType Leaf)) { throw '请先运行 cargo build --release。' }
$testUser = $null
$cleanedService = $true

# 调用 CLI 并检查退出码。参数：Arguments 为原生参数表。返回：输出文本。
function Invoke-Rpmm {
    param([string[]]$Arguments)
    $output = & $script:testBinary --root $script:testRoot @Arguments
    if ($LASTEXITCODE -ne 0) { throw "CLI 失败：$($Arguments -join ' ')" }
    return $output
}
# 等待 unit 达到目标状态。参数：Name、State、Substate 为条件。返回：最终状态。
function Wait-Unit {
    param([string]$Name, [string]$State, [string]$Substate)
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
        $statuses = (Invoke-Rpmm -Arguments @('--json', 'status', $Name)) | ConvertFrom-Json
        $status = @($statuses)[0]
        if ($status.state -eq $State -and (-not $Substate -or $status.substate -eq $Substate)) { return $status }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "等待 $Name/$State 超时。"
}

try {
    New-Item -ItemType Directory -Force -Path (Join-Path $testRoot 'bin'), (Join-Path $testRoot 'units') | Out-Null
    $testBinary = Join-Path $testRoot 'bin/rpmm.exe'
    Copy-Item -LiteralPath $Binary -Destination $testBinary
    $shell = (Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe').Replace('\', '/')
    $prepare = @"
[Service]
Type=oneshot
ExecStart="$shell" -NoProfile -NonInteractive -Command "Write-Output 'SCM prepare'"
RemainAfterExit=yes
"@
    # 单引号 here-string 保留 systemd 需要的 $$，不让 PowerShell 提前展开。
    $app = @'
[Unit]
Requires=prepare.service
After=prepare.service
[Service]
ExecStart="__SHELL__" -NoProfile -NonInteractive -Command "while ($$true) { Write-Output 'SCM app'; Start-Sleep -Milliseconds 100 }"
Restart=on-failure
RestartSec=100ms
[Install]
WantedBy=multi-user.target
'@.Replace('__SHELL__', $shell)
    [IO.File]::WriteAllText((Join-Path $testRoot 'units/prepare.service'), $prepare)
    [IO.File]::WriteAllText((Join-Path $testRoot 'units/app.service'), $app)
    Invoke-Rpmm -Arguments @('verify') | Out-Host

    if ($CreateTestAccount) {
        $testUser = 'rpmm-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
        $secret = 'R!9a' + [guid]::NewGuid().ToString('N')
        $secure = ConvertTo-SecureString -String $secret -AsPlainText -Force
        New-LocalUser -Name $testUser -Password $secure -Description 'rpmm 临时验收账户' | Out-Null
        $Credential = [pscredential]::new("$env:COMPUTERNAME\$testUser", $secure)
        $secret = $null
    }
    $pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($Credential.Password)
    try {
        $plain = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer)
        $plain | & $testBinary --root $testRoot manager install --account $Credential.UserName --password-stdin
        if ($LASTEXITCODE -ne 0) { throw '指定账户安装失败。' }
    } finally {
        $plain = $null
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer)
    }
    $cleanedService = $false
    Invoke-Rpmm -Arguments @('manager', 'start') | Out-Host
    Invoke-Rpmm -Arguments @('enable', 'app.service') | Out-Host
    Invoke-Rpmm -Arguments @('start', 'app.service') | Out-Host
    Wait-Unit 'prepare.service' 'active' | Out-Host
    Wait-Unit 'app.service' 'active' | Out-Host
    Start-Sleep -Milliseconds 500
    $logs = Invoke-Rpmm -Arguments @('logs', 'app.service', '--source', 'stdout')
    if (-not ($logs -match 'SCM app')) { throw '未取得应用 stdout。' }
    Invoke-Rpmm -Arguments @('manager', 'stop') | Out-Host
    Invoke-Rpmm -Arguments @('manager', 'start') | Out-Host
    Wait-Unit 'app.service' 'active' | Out-Host
    Invoke-Rpmm -Arguments @('stop', 'prepare.service') | Out-Host
    Wait-Unit 'app.service' 'inactive' | Out-Host

    $failure = @"
[Unit]
StartLimitBurst=2
[Service]
ExecStart="$shell" -NoProfile -NonInteractive -Command "exit 1"
Restart=on-failure
RestartSec=50ms
[Install]
WantedBy=multi-user.target
"@
    [IO.File]::WriteAllText((Join-Path $testRoot 'units/app.service'), $failure)
    Invoke-Rpmm -Arguments @('daemon-reload') | Out-Host
    Invoke-Rpmm -Arguments @('reset-failed', 'app.service') | Out-Host
    Invoke-Rpmm -Arguments @('start', 'app.service') | Out-Host
    $failed = Wait-Unit 'app.service' 'failed' 'start-limit-hit'
    if ($failed.restart_count -lt 1) { throw '失败重启未发生。' }
    Invoke-Rpmm -Arguments @('reset-failed', 'app.service') | Out-Host
    [IO.File]::WriteAllText((Join-Path $testRoot 'units/app.service'), $app)
    Invoke-Rpmm -Arguments @('daemon-reload') | Out-Host
    Invoke-Rpmm -Arguments @('start', 'app.service') | Out-Host
    Wait-Unit 'app.service' 'active' | Out-Host
    Invoke-Rpmm -Arguments @('manager', 'uninstall') | Out-Host
    $cleanedService = $true
    Write-Host 'SCM 验收通过：安装、依赖、日志、enabled 恢复、失败重启及卸载。'
} finally {
    # 仅本次 preflight 确认不存在服务后才进入此清理块；CLI 再验证注册 root。
    if (Get-Service -Name rpmm -ErrorAction SilentlyContinue) {
        try { Invoke-Rpmm -Arguments @('manager', 'uninstall') | Out-Host; $cleanedService = $true }
        catch { $cleanedService = $false; Write-Warning "服务清理失败，保留目录：$testRoot；$_" }
    }
    if ($testUser -and $cleanedService) { Remove-LocalUser -Name $testUser -ErrorAction SilentlyContinue }
    if ($cleanedService -and (Test-Path -LiteralPath $testRoot)) {
        # 删除前核实实际绝对路径必须仍处于该项目 temp 内；全程只用 PowerShell。
        $resolved = (Resolve-Path -LiteralPath $testRoot).ProviderPath
        if (-not $resolved.StartsWith($temporaryRoot.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw '清理目标越过项目 temp，拒绝删除。' }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
