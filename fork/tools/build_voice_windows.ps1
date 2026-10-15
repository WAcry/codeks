# codeks 0004: build the upstream voice runtime on native Windows runners.
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")]
    [string]$Target,

    [Parameter(Mandatory = $true)]
    [string]$OutputDir
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$Arch = if ($Target.StartsWith("x86_64-")) { "x86_64" } else { "aarch64" }

# Upstream setup scripts export to later workflow steps. Import their values
# here as well because this script prepares and builds within the same step.
function Import-WorkflowEnvironment([string[]]$Names) {
    foreach ($Name in $Names) {
        $Line = Get-Content $env:GITHUB_ENV |
            Where-Object { $_.StartsWith("${Name}=") } |
            Select-Object -Last 1
        if (-not $Line) { throw "Missing workflow environment value: $Name" }
        [Environment]::SetEnvironmentVariable($Name, $Line.Substring($Name.Length + 1), "Process")
    }
}

Push-Location $RepoRoot
try {
    # Autoconf expands CC as words; avoid quoting in libffi's temporary wrapper path.
    $env:TMP = "C:/b/t"
    $env:TEMP = $env:TMP
    New-Item -ItemType Directory -Force -Path $env:TMP | Out-Null
    & ./.github/scripts/compute-bazel-windows-path.ps1
    Import-WorkflowEnvironment @("CODEX_BAZEL_WINDOWS_PATH")

    $Manifest = Get-Content -Raw .github/scripts/voice-cygwin-snapshot.json | ConvertFrom-Json
    $Pin = $Manifest.archive
    $Source = $Manifest.sourceArchive
    $Directory = Join-Path $env:RUNNER_TEMP "voice-cygwin-snapshot"
    New-Item -ItemType Directory -Path $Directory | Out-Null
    # These hash-pinned build inputs belong to the upstream release repository.
    gh release download $Pin.tag --repo openai/codex --pattern $Pin.name --pattern $Source.name --dir $Directory
    $SourceFile = Join-Path $Directory $Source.name
    if ((Get-Item $SourceFile).Length -ne $Source.bytes -or
        (Get-FileHash $SourceFile -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Source.sha256) {
        throw "Public Cygwin source archive mismatch"
    }
    & ./.github/scripts/setup-voice-windows.ps1 -Target $Target -SnapshotArchive (Join-Path $Directory $Pin.name)
    Import-WorkflowEnvironment @("VOICE_WINDOWS_BAZEL_REPOSITORY", "VOICE_CYGWIN_ROOT", "VOICE_PKG_CONFIG")

    if (-not $env:SystemRoot) { throw "Windows SystemRoot is required for native audio actions" }
    $HostArch = $env:PROCESSOR_ARCHITEW6432
    if (-not $HostArch) { $HostArch = $env:PROCESSOR_ARCHITECTURE }
    if (-not $HostArch) { throw "Windows host architecture is required" }
    $env:VOICE_ARCH = $Arch
    $env:BUILDBUDDY_API_KEY = ""
    $env:STABLE_GIT_COMMIT = (git rev-parse HEAD).Trim()

    Push-Location codex-rs
    try { cargo update --workspace } finally { Pop-Location }

    # Use Git Bash explicitly; Windows may also provide a WSL bash.exe.
    $Bash = Join-Path $env:ProgramFiles "Git/bin/bash.exe"
    $BazelArgs = @(
        ".github/scripts/watch_voice_bazel.py", $Bash, "./.github/scripts/run-bazel-ci.sh",
        "--remote-download-toplevel", "--print-failed-action-summary", "--windows-msvc-host-platform", "--",
        "test", "--config=release", "--jobs=$env:NUMBER_OF_PROCESSORS",
        "--disk_cache=$env:CODEKS_BAZEL_DISK_CACHE", "--experimental_disk_cache_gc_max_size=2G",
        "--platforms=//:local_windows_msvc",
        "--extra_toolchains=@local_config_cc//:cc-toolchain-x64_windows,@local_config_cc//:cc-toolchain-arm64_windows,//third_party/voice:windows_pkg_config_toolchain,//third_party/voice:windows_cmake_toolchain",
        "--repo_env=BAZEL_DO_NOT_DETECT_CPP_TOOLCHAIN=0",
        "--repo_env=BAZEL_MSVC_RUNTIME_VISUAL_STUDIO_EULA=1",
        "--inject_repository=voice_windows_tools=$env:VOICE_WINDOWS_BAZEL_REPOSITORY",
        "--//third_party/voice:windows_installed_tools=@voice_windows_tools//:tools",
        "--action_env=SystemRoot=$env:SystemRoot", "--host_action_env=SystemRoot=$env:SystemRoot",
        "--action_env=TMP=$env:TMP", "--host_action_env=TMP=$env:TMP",
        "--action_env=TEMP=$env:TEMP", "--host_action_env=TEMP=$env:TEMP",
        "--action_env=PROCESSOR_ARCHITECTURE=$HostArch", "--host_action_env=PROCESSOR_ARCHITECTURE=$HostArch",
        "--output_groups=default,receipt,sdk",
        "--workspace_status_command=./scripts/workspace-status.cmd",
        "--build_metadata=COMMIT_SHA=$env:STABLE_GIT_COMMIT", "--",
        "//third_party/voice:native_runtime_windows_$Arch",
        "@rules_rust//cargo/private/cargo_build_script_runner:test",
        "//codex-rs/voice-host:codex-voice-host"
    )
    python @BazelArgs
    python fork/tools/voice_package.py stage --target $Target `
        --source "bazel-bin/third_party/voice/native_runtime_windows_$Arch" `
        --helper "bazel-bin/codex-rs/voice-host/codex-voice-host.exe" --output $OutputDir
} finally {
    Pop-Location
}
