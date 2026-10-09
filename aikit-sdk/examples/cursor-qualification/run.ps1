# Requires PowerShell 7, installed/authenticated Cursor and a built SDK example.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$NodePath,
    [Parameter(Mandatory)][string]$CursorCliPath,
    [Parameter(Mandatory)][string]$SdkPath,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [ValidateRange(10, 180)][int]$TimeoutSeconds = 120,
    [switch]$OrdinaryWorkspace
)
$ErrorActionPreference = 'Stop'
foreach ($inputPath in @($NodePath, $CursorCliPath, $SdkPath)) {
    if (-not (Test-Path -LiteralPath $inputPath -PathType Leaf)) { throw "Missing input: $inputPath" }
}
$NodePath = (Resolve-Path -LiteralPath $NodePath).Path
$CursorCliPath = (Resolve-Path -LiteralPath $CursorCliPath).Path
$SdkPath = (Resolve-Path -LiteralPath $SdkPath).Path
$root = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $root) { throw 'OutputDirectory must be new; evidence is never overwritten.' }
[void][IO.Directory]::CreateDirectory($root)
$workspace = Join-Path $root $(if ($OrdinaryWorkspace) { 'control-workspace' } else { "workspace ' dollar `$ semicolon ; unicode é" })
$state = Join-Path $root 'state'
[void][IO.Directory]::CreateDirectory($workspace)
$gitPath = (Get-Command git -CommandType Application | Select-Object -First 1).Source
& $gitPath -C $workspace init --quiet
if ($LASTEXITCODE -ne 0) { throw 'Fixture Git initialization failed.' }

function Invoke-Native([string]$Label, [string]$FileName) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $NodePath
    $start.WorkingDirectory = $workspace
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $start.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    $start.Environment['CURSOR_INVOKED_AS'] = 'cursor-agent.ps1'
    $prompt = "Use the Write tool once to create $FileName containing native-block-probe. If any tool is denied, do not retry or use alternatives; reply BLOCKED. If the write succeeds reply CREATED."
    foreach ($arg in @($CursorCliPath, '--print', '--trust', '--output-format', 'stream-json', $prompt)) {
        $start.ArgumentList.Add($arg)
    }
    $process = [Diagnostics.Process]::Start($start)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $completed = $process.WaitForExit($TimeoutSeconds * 1000)
        if (-not $completed) { $process.Kill($true); $process.WaitForExit() }
        [IO.File]::WriteAllText((Join-Path $root "$Label-stream.jsonl"), $stdout.GetAwaiter().GetResult())
        [IO.File]::WriteAllText((Join-Path $root "$Label-stderr.txt"), $stderr.GetAwaiter().GetResult())
        $record = @{ exit_code = $process.ExitCode; timed_out = -not $completed; file_exists = (Test-Path -LiteralPath (Join-Path $workspace $FileName)) }
        $record | ConvertTo-Json | Set-Content (Join-Path $root "$Label-process.json")
        return $record
    } finally {
        if (-not $process.HasExited) { $process.Kill($true); $process.WaitForExit() }
        $process.Dispose()
    }
}
function Invoke-Sdk([string[]]$Arguments, [string]$InputJson = '') {
    $output = if ($InputJson) { $InputJson | & $SdkPath @Arguments } else { & $SdkPath @Arguments }
    if ($LASTEXITCODE -ne 0) { throw "SDK operation failed: $($Arguments[0])" }
    return ($output -join "`n") | ConvertFrom-Json
}
$installation = $null
try {
    $baseline = Invoke-Native 'baseline' 'baseline.txt'
    $spec = @{
        application_id = 'sdk-example'; agent_key = 'cursor'; workspace = $workspace; timeout_seconds = 15
        events = @('session_started', 'input_submitted', 'before_tool', 'after_tool', 'tool_failed', 'session_ended')
        handler = @{ executable = $SdkPath; arguments = @('hook-cursor', $state, $workspace, '0', 'deny-tools') }
    }
    $plan = Invoke-Sdk @('plan', $state) ($spec | ConvertTo-Json -Depth 10)
    $plan | ConvertTo-Json -Depth 20 | Set-Content (Join-Path $root 'plan.json')
    $installation = Invoke-Sdk @('apply', $state, $plan.id)
    $gated = Invoke-Native 'gated' 'gated.txt'
    $page = Invoke-Sdk @('events', $state, $plan.installation_id)
    $page | ConvertTo-Json -Depth 30 | Set-Content (Join-Path $root 'events.json')
    $blocks = @($page.records | Where-Object { $_.request.event -eq 'before_tool' -and $_.decision.decision -eq 'block' }).Count
    $report = @{
        baseline = $baseline; gated = $gated
        sdk_records = @($page.records).Count
        sdk_tool_blocks = $blocks
        sdk_sha256 = (Get-FileHash -LiteralPath $SdkPath -Algorithm SHA256).Hash
        cursor_cli_sha256 = (Get-FileHash -LiteralPath $CursorCliPath -Algorithm SHA256).Hash
        context = @{ platform = 'windows'; mode = 'print'; node = $NodePath; cursor_cli = $CursorCliPath }
    }
    $report | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $root 'report.json')
    $report | ConvertTo-Json -Depth 10
    if ($baseline.timed_out -or $baseline.exit_code -ne 0 -or -not $baseline.file_exists) {
        throw 'No-hook native write baseline failed; inspect retained evidence.'
    }
    if ($gated.timed_out -or $gated.exit_code -ne 0 -or $gated.file_exists -or $blocks -lt 1) {
        throw 'Native SDK denial was not qualified; inspect native output and SDK events.'
    }
} finally {
    if ($installation) {
        $removal = Invoke-Sdk @('remove-plan', $state, $plan.installation_id)
        Invoke-Sdk @('apply', $state, $removal.id) | Out-Null
    }
}
