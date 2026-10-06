# Requires PowerShell 7 and a separately installed Pi. Never uses the owner profile.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$NodePath,
    [Parameter(Mandatory)][string]$PiCliPath,
    [Parameter(Mandatory)][string]$PiAiModulePath,
    [Parameter(Mandatory)][string]$SdkPath,
    [Parameter(Mandatory)][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
foreach ($inputPath in @($NodePath, $PiCliPath, $PiAiModulePath, $SdkPath)) {
    if (-not (Test-Path -LiteralPath $inputPath -PathType Leaf)) { throw "Missing input: $inputPath" }
}
$NodePath = (Resolve-Path -LiteralPath $NodePath).Path
$PiCliPath = (Resolve-Path -LiteralPath $PiCliPath).Path
$PiAiModulePath = (Resolve-Path -LiteralPath $PiAiModulePath).Path
$SdkPath = (Resolve-Path -LiteralPath $SdkPath).Path
$root = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $root) { throw 'OutputDirectory must be new; existing evidence is never overwritten.' }
[void][IO.Directory]::CreateDirectory($root)
$workspace = Join-Path $root "workspace ' dollar `$ semicolon ; unicode é"
$state = Join-Path $root 'state'
$fixtureProfile = Join-Path $root 'profile'
foreach ($directory in @($workspace, $fixtureProfile)) { [void][IO.Directory]::CreateDirectory($directory) }
@{
    retry = @{ enabled = $false }
    enableInstallTelemetry = $false
    enableAnalytics = $false
    extensions = @('-builtin:mcp', '-builtin:llama.cpp', '-builtin:codemode', '-builtin:tool-search')
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $fixtureProfile 'settings.json') -Encoding utf8
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'provider.mjs') -Destination (Join-Path $root 'provider.mjs')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'boundary-probe.mjs') -Destination (Join-Path $root 'boundary-probe.mjs')
$environmentValues = @{
    PI_CODING_AGENT_DIR = $fixtureProfile; PI_OFFLINE = '1'; PI_TELEMETRY = '0'
    AIKIT_PI_AI_MODULE = $PiAiModulePath
    AIKIT_PI_SDK_EXECUTABLE = $SdkPath; AIKIT_PI_SDK_STATE = $state
}
$sequence = 0
$installationId = $null
$installedExtension = $null

function Assert-That([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Invoke-Bounded([string]$Executable, [string[]]$Arguments, [string]$Label, [string]$InputText = '') {
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $Executable
    $info.WorkingDirectory = $workspace
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.StandardInputEncoding = [Text.UTF8Encoding]::new($false)
    $info.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $info.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    foreach ($entry in $environmentValues.GetEnumerator()) { $info.Environment[$entry.Key] = $entry.Value }
    foreach ($argumentValue in $Arguments) { $info.ArgumentList.Add($argumentValue) }
    $process = [Diagnostics.Process]::Start($info)
    try {
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        if ($InputText) { $process.StandardInput.Write($InputText) }
        $process.StandardInput.Close()
        $timedOut = -not $process.WaitForExit(45000)
        if ($timedOut) { $process.Kill($true); $process.WaitForExit() }
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()
        [IO.File]::WriteAllText((Join-Path $root "$Label-stdout.jsonl"), $stdout)
        [IO.File]::WriteAllText((Join-Path $root "$Label-stderr.txt"), $stderr)
        @{ exit_code = $process.ExitCode; timed_out = $timedOut } | ConvertTo-Json |
            Set-Content -LiteralPath (Join-Path $root "$Label-process.json") -Encoding utf8
        Assert-That (-not $timedOut) "$Label exceeded 45 seconds; its process tree was terminated."
        Assert-That ($process.ExitCode -eq 0) "$Label exited $($process.ExitCode); inspect saved stderr."
        return $stdout
    } finally { $process.Dispose() }
}

function Invoke-Sdk([string[]]$Arguments, [string]$InputText = '') {
    $script:sequence++
    Invoke-Bounded $SdkPath $Arguments "sdk-$sequence" $InputText | ConvertFrom-Json
}

function Install-Gate([int]$Blocks, [bool]$DenyTools = $false) {
    $handlerArguments = @('hook-pi', $state, $workspace, "$Blocks")
    if ($DenyTools) { $handlerArguments += 'deny-tools' }
    $spec = @{
        application_id = 'sdk-example'; agent_key = 'pi'; workspace = $workspace
        timeout_seconds = 5
        handler = @{ executable = $SdkPath; arguments = $handlerArguments }
        events = @('session_started', 'input_submitted', 'before_tool', 'after_tool',
            'tool_failed', 'completion_proposed', 'completion_failed', 'session_ended')
    }
    $plan = Invoke-Sdk @('plan', $state) ($spec | ConvertTo-Json -Depth 8)
    $script:installationId = $plan.installation_id
    $script:installedExtension = $plan.config_path
    $environmentValues['AIKIT_PI_INSTALLATION'] = $installationId
    $result = Invoke-Sdk @('apply', $state, $plan.id)
    Assert-That ($result.status -eq 'configured') 'Installation did not become configured.'
}

function Run-Native([string]$Label, [string]$Scenario, [string]$WriteFile = 'probe.txt') {
    $environmentValues['AIKIT_PI_SCENARIO'] = $Scenario
    $environmentValues['AIKIT_PI_WRITE_FILE'] = $WriteFile
    $environmentValues['AIKIT_PI_EVIDENCE'] = Join-Path $root "$Label-events.jsonl"
    $nativeArguments = @($PiCliPath, '--print', '--mode', 'json', '--offline', '--approve',
        '--no-skills', '--no-prompt-templates', '--no-context-files', '--no-themes', '--no-mcp',
        '--extension', (Join-Path $root 'provider.mjs'), '--provider', 'aikit-fixture',
        '--model', 'deterministic', '--session-dir', (Join-Path $root 'sessions'))
    if ($Scenario -in @('abort-after-allow', 'override-block')) {
        # Control ordering without loading the SDK twice through Windows path
        # aliases. These two scenarios explicitly load the owned installed source;
        # the other scenarios still qualify native project auto-discovery.
        $nativeArguments += @('--no-extensions', '--extension', $installedExtension, '--extension', (Join-Path $root 'boundary-probe.mjs'))
    }
    if ($Scenario -eq 'write') { $nativeArguments += @('--tools', 'write') }
    else { $nativeArguments += '--no-tools' }
    $nativeArguments += 'Run the deterministic qualification fixture.'
    $stream = Invoke-Bounded $NodePath $nativeArguments $Label
    $events = @(Get-Content -LiteralPath $environmentValues['AIKIT_PI_EVIDENCE'] | ConvertFrom-Json)
    $starts = @($events | Where-Object event -eq 'session_start')
    Assert-That ($starts.Count -eq 1) "$Label did not start exactly one native session."
    Assert-That (@($events | Where-Object event -eq 'session_shutdown').Count -eq 1) "$Label did not shut down."
    $rows = @()
    if ($installationId) {
        $journal = Invoke-Sdk @('events', $state, $installationId)
        $journal | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $root "$Label-sdk.json") -Encoding utf8
        $rows = @($journal.records | Where-Object { $_.request.session_id -eq $starts[0].session })
        Assert-That (@($rows | Where-Object { $_.request.event -eq 'session_started' }).Count -eq 1) "$Label missing SDK start."
        Assert-That (@($rows | Where-Object { $_.request.event -eq 'session_ended' }).Count -eq 1) "$Label missing SDK end."
        $invocations = @($rows | ForEach-Object { $_.request.invocation_id } | Select-Object -Unique)
        Assert-That ($invocations.Count -eq 1 -and $invocations[0] -match '^[0-9a-f-]{36}$') "$Label missing a consistent bridge invocation scope."
    }
    return @{ events = $events; rows = $rows; stream = @($stream -split "`n" | Where-Object { $_.Trim() } | ConvertFrom-Json) }
}

try {
    $nodeVersion = (Invoke-Bounded $NodePath @('--version') 'node-version').Trim()
    $piVersion = (Invoke-Bounded $NodePath @($PiCliPath, '--version') 'pi-version').Trim()
    $baseline = Run-Native 'write-baseline' 'write' 'baseline.txt'
    Assert-That ([IO.File]::ReadAllText((Join-Path $workspace 'baseline.txt')) -eq "native-write-probe`n") 'Baseline write failed.'
    Assert-That (@($baseline.events | Where-Object { $_.event -eq 'tool_result' -and $_.isError -eq $false }).Count -eq 1) 'Missing baseline success.'

    Install-Gate 3
    $stop = Run-Native 'stop-blocked' 'stop'
    $decisions = @($stop.rows | Where-Object { $_.request.event -eq 'completion_proposed' -and $null -ne $_.decision })
    Assert-That (($decisions.decision.decision -join ',') -eq 'block,block,block,allow') 'Completion sequence differs.'
    Assert-That (@($stop.events | Where-Object type -eq 'fixture_model_call').Count -eq 4) 'Native continuation count differs.'
    Assert-That (@($decisions | Where-Object { $_.request.final_answer -ne 'OK' }).Count -eq 0) 'Final answers were lost.'

    Install-Gate 0
    $allowed = Run-Native 'write-allowed' 'write' 'allowed.txt'
    Assert-That ([IO.File]::ReadAllText((Join-Path $workspace 'allowed.txt')) -eq "native-write-probe`n") 'Installed allow did not write.'
    Assert-That (@($allowed.rows | Where-Object { $_.request.event -eq 'after_tool' }).Count -eq 1) 'Missing successful tool observation.'
    # An existing directory is an invalid file target on the qualified platform.
    $failedTool = Run-Native 'write-failed' 'write' '.'
    Assert-That (@($failedTool.rows | Where-Object { $_.request.event -eq 'tool_failed' }).Count -eq 1) 'Missing failed tool observation.'

    Install-Gate 0 $true
    $denied = Run-Native 'write-denied' 'write' 'denied.txt'
    Assert-That (-not (Test-Path -LiteralPath (Join-Path $workspace 'denied.txt'))) 'Denied write created a file.'
    Assert-That (@($denied.rows | Where-Object { $_.request.event -eq 'before_tool' -and $_.decision.decision -eq 'block' }).Count -eq 1) 'Missing SDK tool denial.'
    $toolResults = @($denied.stream | Where-Object type -eq 'turn_end' | ForEach-Object { $_.toolResults })
    Assert-That (@($toolResults | Where-Object { $_.toolCallId -eq 'native-write' -and $_.isError -eq $true }).Count -eq 1) 'Native denial result missing.'

    $failure = Run-Native 'provider-failed' 'failure'
    Assert-That (@($failure.rows | Where-Object { $_.request.event -eq 'completion_failed' }).Count -eq 1) 'Missing failed completion.'
    Assert-That (@($failure.rows | Where-Object { $_.request.event -eq 'completion_proposed' -or $null -ne $_.request.final_answer }).Count -eq 0) 'Failure became completion evidence.'

    Install-Gate 0
    $aborted = Run-Native 'abort-after-allow' 'abort-after-allow'
    $probes = @($aborted.events | Where-Object type -eq 'boundary_probe')
    Assert-That ($probes.Count -eq 1 -and $probes[0].sdk_decision -eq 'allow') 'Probe did not abort after a recorded Allow.'
    Assert-That ($probes[0].outcome_before_action -eq 'completed') 'Unexpected outcome before abort.'
    Assert-That (@($aborted.events | Where-Object event -eq 'agent_settled').Count -eq 1) 'Abort did not settle.'
    $settled = @($aborted.events | Where-Object event -eq 'agent_settled')[0]
    $normalSettled = @($stop.events | Where-Object event -eq 'agent_settled')[0]
    Assert-That (($settled.event_keys -join ',') -eq 'type' -and ($normalSettled.event_keys -join ',') -eq 'type') 'Settlement now carries additional fields: re-evaluate completion evidence.'
    Assert-That ($probes[0].signal_present -eq $false -and $settled.signal_present -eq $false) 'Abort signal availability changed: revisit native settlement qualification.'
    Assert-That (@($aborted.rows | Where-Object { $_.request.event -eq 'completion_failed' }).Count -eq 0) 'Abort now has a failure signal: revisit the native API qualification.'

    Install-Gate 3
    $overridden = Run-Native 'stop-block-overridden' 'override-block'
    $probes = @($overridden.events | Where-Object type -eq 'boundary_probe')
    Assert-That ($probes.Count -eq 1 -and $probes[0].sdk_decision -eq 'block' -and $probes[0].incoming_continue -eq $true) 'Probe did not observe SDK continuation request.'
    Assert-That (@($overridden.events | Where-Object type -eq 'fixture_model_call').Count -eq 1) 'Override did not stop continuation after one native turn.'
    Assert-That (@($overridden.events | Where-Object event -eq 'agent_settled').Count -eq 1) 'Overridden Block did not settle.'
    Assert-That (@($overridden.rows | Where-Object { $_.decision.decision -eq 'allow' -and $_.request.event -eq 'completion_proposed' }).Count -eq 0) 'Unexpected completion permission.'
    $summary = @{ node = $nodeVersion; pi = $piVersion; platform = [Environment]::OSVersion.ToString()
        sdk_sha256 = (Get-FileHash -LiteralPath $SdkPath -Algorithm SHA256).Hash
        pi_cli_sha256 = (Get-FileHash -LiteralPath $PiCliPath -Algorithm SHA256).Hash
        provider_sha256 = (Get-FileHash -LiteralPath (Join-Path $root 'provider.mjs') -Algorithm SHA256).Hash
        boundary_probe_sha256 = (Get-FileHash -LiteralPath (Join-Path $root 'boundary-probe.mjs') -Algorithm SHA256).Hash
        scenarios_passed = @('write-baseline', 'stop-blocked', 'write-allowed', 'write-failed', 'write-denied', 'provider-failed', 'abort-after-allow', 'stop-block-overridden')
        scope = 'Native print-mode transport and loop with deterministic model; not full provider readiness.' }
} finally {
    if ($installationId) {
        $removal = Invoke-Sdk @('remove-plan', $state, $installationId)
        $removed = Invoke-Sdk @('apply', $state, $removal.id)
        Assert-That ($removed.status -eq 'absent') 'Owned extension cleanup failed.'
    }
}
$summary | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $root 'summary.json') -Encoding utf8
$summary | ConvertTo-Json -Depth 5
