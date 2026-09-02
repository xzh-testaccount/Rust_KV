[CmdletBinding()]
param(
    [ValidateRange(1, 1000000)]
    [int]$Operations = 2000,

    [ValidateRange(1, 1000000)]
    [int]$LiveKeys = 100,

    [ValidateRange(3, 15)]
    [int]$Runs = 5,

    [string]$BasicRepo = 'D:\rustwork\Rust_KV_basic_bench',

    [string]$OutputDir = ''
)

$ErrorActionPreference = 'Stop'
$toolchain = 'stable-x86_64-pc-windows-gnu'
$advancedRepo = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))

if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = Join-Path $advancedRepo 'docs\results'
}

function Assert-Repository([string]$Path, [string]$Name) {
    if (-not (Test-Path -LiteralPath (Join-Path $Path 'Cargo.toml'))) {
        throw "$Name仓库不存在或缺少Cargo.toml：$Path"
    }
}

function Invoke-StorageProbe([string]$RepoPath) {
    Push-Location $RepoPath
    try {
        $output = & cargo "+$toolchain" run --quiet --release --example storage_benchmark -- $Operations $LiveKeys 2>&1
        if ($LASTEXITCODE -ne 0) {
            throw "基准程序运行失败：`n$($output -join "`n")"
        }
    }
    finally {
        Pop-Location
    }

    $metrics = @{}
    foreach ($outputLine in $output) {
        $text = [string]$outputLine
        if ($text -match '^(?<key>[a-z_]+)=(?<value>.+)$') {
            $metrics[$Matches.key] = $Matches.value.Trim()
        }
    }
    return $metrics
}

function Get-RequiredNumber([hashtable]$Metrics, [string]$Name, [string]$Variant) {
    if (-not $Metrics.ContainsKey($Name)) {
        throw "$Variant输出缺少指标：$Name"
    }
    return [double]::Parse(
        $Metrics[$Name],
        [System.Globalization.CultureInfo]::InvariantCulture
    )
}

function Get-Median([double[]]$Values) {
    if ($Values.Count -eq 0) {
        throw '无法计算空数据的中位数'
    }
    $sorted = @($Values | Sort-Object)
    $middle = [int][Math]::Floor($sorted.Count / 2)
    if ($sorted.Count % 2 -eq 1) {
        return [double]$sorted[$middle]
    }
    return ([double]$sorted[$middle - 1] + [double]$sorted[$middle]) / 2.0
}

function Format-OneDecimal([double]$Value) {
    return $Value.ToString('N1', [System.Globalization.CultureInfo]::InvariantCulture)
}

function New-BarPanel(
    [double]$X,
    [double]$Y,
    [double]$Width,
    [double]$Height,
    [string]$Title,
    [string]$Subtitle,
    [object[]]$Items
) {
    $builder = [System.Text.StringBuilder]::new()
    [void]$builder.AppendLine("<g>")
    [void]$builder.AppendLine("<rect x='$X' y='$Y' width='$Width' height='$Height' rx='24' fill='#FFFFFF' filter='url(#shadow)'/>")
    [void]$builder.AppendLine("<text x='$($X + 28)' y='$($Y + 42)' class='panel-title'>$Title</text>")
    [void]$builder.AppendLine("<text x='$($X + 28)' y='$($Y + 68)' class='panel-subtitle'>$Subtitle</text>")

    $labelWidth = 145.0
    $barX = $X + $labelWidth + 28
    $barWidth = $Width - $labelWidth - 82
    $contentTop = $Y + 92
    $contentHeight = $Height - 116
    $rowHeight = $contentHeight / [Math]::Max(1, $Items.Count)
    $maximum = [double](($Items | Measure-Object -Property Value -Maximum).Maximum)
    if ($maximum -le 0) {
        $maximum = 1
    }

    for ($index = 0; $index -lt $Items.Count; $index++) {
        $item = $Items[$index]
        $centerY = $contentTop + ($index * $rowHeight) + ($rowHeight / 2)
        $barY = $centerY - 11
        $actualWidth = [Math]::Max(5, $barWidth * ([double]$item.Value / $maximum))
        $labelY = $centerY + 5
        $valueX = [Math]::Min($barX + $actualWidth + 10, $X + $Width - 72)

        [void]$builder.AppendLine("<text x='$($barX - 12)' y='$labelY' text-anchor='end' class='bar-label'>$($item.Label)</text>")
        [void]$builder.AppendLine("<rect x='$barX' y='$barY' width='$barWidth' height='22' rx='11' fill='#E8EEF6'/>")
        [void]$builder.AppendLine("<rect x='$barX' y='$barY' width='$actualWidth' height='22' rx='11' fill='url(#$($item.Gradient))'/>")
        [void]$builder.AppendLine("<text x='$valueX' y='$labelY' class='bar-value'>$($item.Display)</text>")
    }

    [void]$builder.AppendLine('</g>')
    return $builder.ToString()
}

Assert-Repository $BasicRepo '基础版'
Assert-Repository $advancedRepo '创新版'
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

$basicSamples = @()
$advancedSamples = @()

for ($run = 1; $run -le $Runs; $run++) {
    Write-Host "[$run/$Runs] 运行基础版与创新版..." -ForegroundColor Cyan

    # 奇偶轮次交换运行顺序，降低缓存和机器温度造成的固定偏差。
    if ($run % 2 -eq 1) {
        $basic = Invoke-StorageProbe $BasicRepo
        $advanced = Invoke-StorageProbe $advancedRepo
    }
    else {
        $advanced = Invoke-StorageProbe $advancedRepo
        $basic = Invoke-StorageProbe $BasicRepo
    }

    $basicSamples += [pscustomobject]@{
        run = $run
        write_us = Get-RequiredNumber $basic 'write_us' '基础版'
        disk_bytes = Get-RequiredNumber $basic 'wal_bytes' '基础版'
        recovery_us = Get-RequiredNumber $basic 'recovery_median_us' '基础版'
    }

    $advancedSamples += [pscustomobject]@{
        run = $run
        write_us = Get-RequiredNumber $advanced 'write_us' '创新版'
        disk_before_bytes = Get-RequiredNumber $advanced 'wal_bytes_before' '创新版'
        disk_after_bytes = Get-RequiredNumber $advanced 'disk_bytes_after' '创新版'
        compact_us = Get-RequiredNumber $advanced 'compact_us' '创新版'
        recovery_before_us = Get-RequiredNumber $advanced 'recovery_before_compact_median_us' '创新版'
        recovery_after_us = Get-RequiredNumber $advanced 'recovery_after_compact_median_us' '创新版'
    }
}

$basicWriteUs = Get-Median ([double[]]$basicSamples.write_us)
$advancedWriteUs = Get-Median ([double[]]$advancedSamples.write_us)
$basicDiskBytes = Get-Median ([double[]]$basicSamples.disk_bytes)
$advancedDiskBeforeBytes = Get-Median ([double[]]$advancedSamples.disk_before_bytes)
$advancedDiskAfterBytes = Get-Median ([double[]]$advancedSamples.disk_after_bytes)
$compactUs = Get-Median ([double[]]$advancedSamples.compact_us)
$basicRecoveryUs = Get-Median ([double[]]$basicSamples.recovery_us)
$advancedRecoveryBeforeUs = Get-Median ([double[]]$advancedSamples.recovery_before_us)
$advancedRecoveryAfterUs = Get-Median ([double[]]$advancedSamples.recovery_after_us)

$compressionRatio = (1.0 - ($advancedDiskAfterBytes / $advancedDiskBeforeBytes)) * 100.0
$overallDiskReduction = (1.0 - ($advancedDiskAfterBytes / $basicDiskBytes)) * 100.0
$recoveryImprovement = (1.0 - ($advancedRecoveryAfterUs / $basicRecoveryUs)) * 100.0
$compactionRecoveryImprovement = (1.0 - ($advancedRecoveryAfterUs / $advancedRecoveryBeforeUs)) * 100.0
$writeOverhead = (($advancedWriteUs / $basicWriteUs) - 1.0) * 100.0
$walMetadataOverhead = (($advancedDiskBeforeBytes / $basicDiskBytes) - 1.0) * 100.0

$basicCommit = (& git -C $BasicRepo rev-parse --short HEAD).Trim()
$advancedCommit = (& git -C $advancedRepo rev-parse --short HEAD).Trim()
$rustVersion = (& rustc "+$toolchain" --version).Trim()

$result = [ordered]@{
    generated_at = (Get-Date).ToString('o')
    input = [ordered]@{
        operations = $Operations
        live_keys = $LiveKeys
        outer_runs = $Runs
        recovery_repeats_per_run = 7
        profile = 'release'
        toolchain = $toolchain
    }
    versions = [ordered]@{
        basic_commit = $basicCommit
        advanced_commit = $advancedCommit
        rustc = $rustVersion
    }
    medians = [ordered]@{
        basic_write_us = $basicWriteUs
        advanced_write_us = $advancedWriteUs
        basic_disk_bytes = $basicDiskBytes
        advanced_disk_before_bytes = $advancedDiskBeforeBytes
        advanced_disk_after_bytes = $advancedDiskAfterBytes
        compact_us = $compactUs
        basic_recovery_us = $basicRecoveryUs
        advanced_recovery_before_us = $advancedRecoveryBeforeUs
        advanced_recovery_after_us = $advancedRecoveryAfterUs
    }
    derived_percent = [ordered]@{
        compaction_ratio = $compressionRatio
        disk_reduction_vs_basic = $overallDiskReduction
        recovery_improvement_vs_basic = $recoveryImprovement
        recovery_improvement_before_after = $compactionRecoveryImprovement
        write_overhead = $writeOverhead
        wal_metadata_overhead = $walMetadataOverhead
    }
    samples = [ordered]@{
        basic = $basicSamples
        advanced = $advancedSamples
    }
}

$jsonPath = Join-Path $OutputDir 'b_compaction_metrics.json'
$csvPath = Join-Path $OutputDir 'b_compaction_samples.csv'
$svgPath = Join-Path $OutputDir 'b_compaction_comparison.svg'
$pngPath = Join-Path $OutputDir 'b_compaction_comparison.png'
$reportPath = Join-Path $OutputDir 'b_compaction_result.md'
$utf8 = [System.Text.UTF8Encoding]::new($false)

[System.IO.File]::WriteAllText($jsonPath, ($result | ConvertTo-Json -Depth 8), $utf8)

$csvRows = for ($index = 0; $index -lt $Runs; $index++) {
    [pscustomobject]@{
        run = $index + 1
        basic_write_ms = $basicSamples[$index].write_us / 1000.0
        advanced_write_ms = $advancedSamples[$index].write_us / 1000.0
        compact_ms = $advancedSamples[$index].compact_us / 1000.0
        basic_recovery_ms = $basicSamples[$index].recovery_us / 1000.0
        advanced_recovery_before_ms = $advancedSamples[$index].recovery_before_us / 1000.0
        advanced_recovery_after_ms = $advancedSamples[$index].recovery_after_us / 1000.0
        basic_disk_bytes = $basicSamples[$index].disk_bytes
        advanced_disk_before_bytes = $advancedSamples[$index].disk_before_bytes
        advanced_disk_after_bytes = $advancedSamples[$index].disk_after_bytes
    }
}
$csvText = ($csvRows | ConvertTo-Csv -NoTypeInformation) -join "`r`n"
[System.IO.File]::WriteAllText($csvPath, $csvText, $utf8)

$diskItems = @(
    [pscustomobject]@{ Label = '基础版 WAL'; Value = $basicDiskBytes; Display = "$(Format-OneDecimal ($basicDiskBytes / 1024.0)) KiB"; Gradient = 'basicGradient' },
    [pscustomobject]@{ Label = '创新版压缩前'; Value = $advancedDiskBeforeBytes; Display = "$(Format-OneDecimal ($advancedDiskBeforeBytes / 1024.0)) KiB"; Gradient = 'beforeGradient' },
    [pscustomobject]@{ Label = '创新版压缩后'; Value = $advancedDiskAfterBytes; Display = "$(Format-OneDecimal ($advancedDiskAfterBytes / 1024.0)) KiB"; Gradient = 'afterGradient' }
)

$recoveryItems = @(
    [pscustomobject]@{ Label = '基础版 WAL'; Value = $basicRecoveryUs; Display = "$(Format-OneDecimal ($basicRecoveryUs / 1000.0)) ms"; Gradient = 'basicGradient' },
    [pscustomobject]@{ Label = '创新版压缩前'; Value = $advancedRecoveryBeforeUs; Display = "$(Format-OneDecimal ($advancedRecoveryBeforeUs / 1000.0)) ms"; Gradient = 'beforeGradient' },
    [pscustomobject]@{ Label = 'Snapshot恢复'; Value = $advancedRecoveryAfterUs; Display = "$(Format-OneDecimal ($advancedRecoveryAfterUs / 1000.0)) ms"; Gradient = 'afterGradient' }
)

$compactItems = @()
foreach ($sample in $advancedSamples) {
    $compactItems += [pscustomobject]@{
        Label = "第$($sample.run)轮"
        Value = $sample.compact_us
        Display = "$(Format-OneDecimal ($sample.compact_us / 1000.0)) ms"
        Gradient = 'runGradient'
    }
}
$compactItems += [pscustomobject]@{
    Label = '中位数'
    Value = $compactUs
    Display = "$(Format-OneDecimal ($compactUs / 1000.0)) ms"
    Gradient = 'compactGradient'
}

$writeItems = @(
    [pscustomobject]@{ Label = '基础版写入'; Value = $basicWriteUs; Display = "$(Format-OneDecimal ($basicWriteUs / 1000.0)) ms"; Gradient = 'basicGradient' },
    [pscustomobject]@{ Label = '创新版写入'; Value = $advancedWriteUs; Display = "$(Format-OneDecimal ($advancedWriteUs / 1000.0)) ms"; Gradient = 'advancedGradient' }
)

$diskPanel = New-BarPanel 55 178 650 310 '① 持久化文件大小' "越小越好｜压缩率 $(Format-OneDecimal $compressionRatio)%" $diskItems
$recoveryPanel = New-BarPanel 735 178 650 310 '② 启动恢复时间' "越小越好｜相对基础版改善 $(Format-OneDecimal $recoveryImprovement)%" $recoveryItems
$compactPanel = New-BarPanel 55 518 650 350 '③ compact() 暂停时间' '越小越好｜展示每轮波动与中位数' $compactItems
$writePanel = New-BarPanel 735 518 650 350 '④ 2000次正常写入' "越小越好｜创新版变化 $(Format-OneDecimal $writeOverhead)%" $writeItems

$summary = "压缩率 $(Format-OneDecimal $compressionRatio)%  ·  恢复变化 $(Format-OneDecimal $recoveryImprovement)%  ·  压缩中位数 $(Format-OneDecimal ($compactUs / 1000.0)) ms  ·  写入变化 $(Format-OneDecimal $writeOverhead)%"
$svg = @"
<svg xmlns="http://www.w3.org/2000/svg" width="1440" height="920" viewBox="0 0 1440 920">
  <defs>
    <linearGradient id="pageGradient" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#EEF2FF"/><stop offset="0.55" stop-color="#F8FAFC"/><stop offset="1" stop-color="#ECFDF5"/></linearGradient>
    <linearGradient id="basicGradient" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#6366F1"/><stop offset="1" stop-color="#3B82F6"/></linearGradient>
    <linearGradient id="beforeGradient" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#FB923C"/><stop offset="1" stop-color="#F59E0B"/></linearGradient>
    <linearGradient id="afterGradient" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#34D399"/><stop offset="1" stop-color="#059669"/></linearGradient>
    <linearGradient id="advancedGradient" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#A855F7"/><stop offset="1" stop-color="#EC4899"/></linearGradient>
    <linearGradient id="compactGradient" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#F59E0B"/><stop offset="1" stop-color="#EF4444"/></linearGradient>
    <linearGradient id="runGradient" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#FDE68A"/><stop offset="1" stop-color="#FB923C"/></linearGradient>
    <filter id="shadow" x="-20%" y="-20%" width="140%" height="150%"><feDropShadow dx="0" dy="9" stdDeviation="12" flood-color="#64748B" flood-opacity="0.16"/></filter>
    <style>
      text { font-family: "Microsoft YaHei", "Segoe UI", sans-serif; fill: #0F172A; }
      .title { font-size: 34px; font-weight: 700; }
      .subtitle { font-size: 16px; fill: #475569; }
      .meta { font-size: 14px; fill: #64748B; }
      .panel-title { font-size: 21px; font-weight: 700; }
      .panel-subtitle { font-size: 13px; fill: #64748B; }
      .bar-label { font-size: 13px; fill: #334155; }
      .bar-value { font-size: 13px; font-weight: 700; fill: #0F172A; }
    </style>
  </defs>
  <rect width="1440" height="920" fill="url(#pageGradient)"/>
  <circle cx="1330" cy="55" r="115" fill="#C7D2FE" opacity="0.42"/>
  <circle cx="90" cy="885" r="145" fill="#A7F3D0" opacity="0.32"/>
  <text x="55" y="62" class="title">B模块 · WAL日志压缩性能测评</text>
  <text x="55" y="96" class="subtitle">$Operations 次同步写入 · $LiveKeys 个最终键 · $Runs 轮取中位数 · recovery每轮7次</text>
  <text x="55" y="128" class="meta">$summary</text>
  <text x="1385" y="128" text-anchor="end" class="meta">基础 $basicCommit · 创新 $advancedCommit</text>
  $diskPanel
  $recoveryPanel
  $compactPanel
  $writePanel
  <text x="720" y="902" text-anchor="middle" class="meta">release · $toolchain · 数值越小越好（压缩率除外）</text>
</svg>
"@
[System.IO.File]::WriteAllText($svgPath, $svg, $utf8)

$browserCandidates = @(
    'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe',
    'C:\Program Files\Microsoft\Edge\Application\msedge.exe',
    'C:\Program Files\Google\Chrome\Application\chrome.exe',
    'C:\Program Files (x86)\Google\Chrome\Application\chrome.exe'
)
$browser = $browserCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if ($browser) {
    $browserProfile = Join-Path $advancedRepo 'target\benchmark-edge-profile'
    New-Item -ItemType Directory -Force -Path $browserProfile | Out-Null
    if (Test-Path -LiteralPath $pngPath) {
        Remove-Item -LiteralPath $pngPath -Force
    }
    $svgUri = ([System.Uri]$svgPath).AbsoluteUri
    $browserOutput = & $browser --headless=new --no-first-run --disable-gpu --hide-scrollbars "--user-data-dir=$browserProfile" --window-size=1440,920 --force-device-scale-factor=1 "--screenshot=$pngPath" $svgUri 2>&1
    for ($attempt = 0; $attempt -lt 50 -and -not (Test-Path -LiteralPath $pngPath); $attempt++) {
        Start-Sleep -Milliseconds 100
    }
    if (-not (Test-Path -LiteralPath $pngPath)) {
        Write-Warning "PNG预览生成失败，SVG仍可正常使用：$($browserOutput -join ' ')"
    }
}

$chartFileName = if (Test-Path -LiteralPath $pngPath) {
    'b_compaction_comparison.png'
}
else {
    'b_compaction_comparison.svg'
}

$report = @"
# B模块WAL日志压缩测评结果

![彩色柱状图]($chartFileName)

## 测试输入

| 参数 | 数值 |
| --- | ---: |
| 同步SET次数 | $Operations |
| 最终有效键 | $LiveKeys |
| 完整运行轮次 | $Runs |
| 每轮恢复重复 | 7 |
| 编译模式 | release |
| 工具链 | $toolchain |

写入采用固定的循环覆盖负载：``key:0000``到``key:$('{0:D4}' -f ($LiveKeys - 1))``，值为递增的``value:XXXXXXXX``。每轮使用新的临时目录，写入后和每次恢复后都验证全部最终键值。

## 四项实验中位数

| 指标 | 基础版 | 创新版压缩前 | 创新版压缩后 |
| --- | ---: | ---: | ---: |
| 持久化文件大小 | $(Format-OneDecimal ($basicDiskBytes / 1024.0)) KiB | $(Format-OneDecimal ($advancedDiskBeforeBytes / 1024.0)) KiB | $(Format-OneDecimal ($advancedDiskAfterBytes / 1024.0)) KiB |
| 启动恢复 | $(Format-OneDecimal ($basicRecoveryUs / 1000.0)) ms | $(Format-OneDecimal ($advancedRecoveryBeforeUs / 1000.0)) ms | $(Format-OneDecimal ($advancedRecoveryAfterUs / 1000.0)) ms |
| ``compact()``暂停 | — | — | $(Format-OneDecimal ($compactUs / 1000.0)) ms |
| $Operations 次正常写入 | $(Format-OneDecimal ($basicWriteUs / 1000.0)) ms | $(Format-OneDecimal ($advancedWriteUs / 1000.0)) ms | 与压缩前相同 |

## 结论

- 创新版自身压缩前后空间减少 **$(Format-OneDecimal $compressionRatio)%**。
- 创新版压缩后相对基础版空间减少 **$(Format-OneDecimal $overallDiskReduction)%**。
- Snapshot恢复相对基础版变化 **$(Format-OneDecimal $recoveryImprovement)%**；相对创新版压缩前变化 **$(Format-OneDecimal $compactionRecoveryImprovement)%**。
- 单次``compact()``暂停中位数为 **$(Format-OneDecimal ($compactUs / 1000.0)) ms**。
- 创新版正常写入相对基础版变化 **$(Format-OneDecimal $writeOverhead)%**。数值接近0时应解释为没有观察到明显退化，正负波动不代表写入优化。
- 新版WAL元数据使压缩前文件相对基础版变化 **$(Format-OneDecimal $walMetadataOverhead)%**。

原始数据见``b_compaction_metrics.json``和``b_compaction_samples.csv``。耗时受磁盘缓存和系统负载影响，文件大小及最终数据校验是确定结果。
"@
[System.IO.File]::WriteAllText($reportPath, $report, $utf8)

Write-Host ''
Write-Host '测评完成' -ForegroundColor Green
Write-Host "压缩率：$(Format-OneDecimal $compressionRatio)%" -ForegroundColor Green
Write-Host "恢复变化：$(Format-OneDecimal $recoveryImprovement)%" -ForegroundColor Green
Write-Host "compact中位数：$(Format-OneDecimal ($compactUs / 1000.0)) ms" -ForegroundColor Yellow
Write-Host "写入变化：$(Format-OneDecimal $writeOverhead)%" -ForegroundColor Magenta
Write-Host "图表：$svgPath" -ForegroundColor Cyan
if (Test-Path -LiteralPath $pngPath) {
    Write-Host "PNG：$pngPath" -ForegroundColor Cyan
}
Write-Host "报告：$reportPath" -ForegroundColor Cyan
Write-Host "原始JSON：$jsonPath" -ForegroundColor DarkCyan
