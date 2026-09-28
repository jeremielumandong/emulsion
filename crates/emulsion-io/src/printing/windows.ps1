param([Parameter(Mandatory=$true)][string]$Request)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
Add-Type -AssemblyName System.Drawing
$r = Get-Content -LiteralPath $Request -Raw -Encoding UTF8 | ConvertFrom-Json
function PaperId($paper) { return ('{0}:{1}:{2}:{3}' -f $paper.RawKind,$paper.PaperName,$paper.Width,$paper.Height) }
function PaperInfo($settings, $paper) {
    $page = New-Object System.Drawing.Printing.PageSettings($settings)
    $page.PaperSize = $paper
    $area = $page.PrintableArea
    return @{id=(PaperId $paper); name=$paper.PaperName; width=$paper.Width * .254; height=$paper.Height * .254;
        margins=@([Math]::Max(0,$area.Top*.254), [Math]::Max(0,($paper.Width-$area.Right)*.254), [Math]::Max(0,($paper.Height-$area.Bottom)*.254), [Math]::Max(0,$area.Left*.254))}
}
try {
    if ($r.action -eq 'discover') {
        $default = New-Object System.Drawing.Printing.PrinterSettings
        $items = @([System.Drawing.Printing.PrinterSettings]::InstalledPrinters | ForEach-Object {
            @{id=$_;name=$_;default=($_ -eq $default.PrinterName);status='System queue'}
        })
        ConvertTo-Json -InputObject $items -Depth 6 -Compress
        exit 0
    }
    $settings = New-Object System.Drawing.Printing.PrinterSettings
    $settings.PrinterName = $r.printer
    if (-not $settings.IsValid) {throw 'This printer is no longer available. Refresh the printer list.'}
    if ($r.action -eq 'capabilities') {
        $papers = @($settings.PaperSizes | ForEach-Object {PaperInfo $settings $_})
        $trays = @($settings.PaperSources | ForEach-Object {@{id=[string]$_.RawKind;name=$_.SourceName}})
        $quality = @($settings.PrinterResolutions | ForEach-Object {@{id=('{0},{1},{2}' -f [int]$_.Kind,$_.X,$_.Y);name=('{0} ({1} × {2})' -f $_.Kind,$_.X,$_.Y)}})
        $sides = @(@{id='one-sided';name='One-sided'})
        if ($settings.CanDuplex) {$sides += @{id='two-sided-long-edge';name='Duplex · long edge'}; $sides += @{id='two-sided-short-edge';name='Duplex · short edge'}}
        @{papers=$papers;default_paper=(PaperId $settings.DefaultPageSettings.PaperSize);media=@();trays=$trays;quality=$quality;sides=$sides;color=$settings.SupportsColor} | ConvertTo-Json -Depth 8 -Compress
        exit 0
    }
    if ($r.action -ne 'submit') {throw 'Unknown print operation'}
    $paper = $settings.PaperSizes | Where-Object {(PaperId $_) -eq $r.paper} | Select-Object -First 1
    if ($null -eq $paper) {throw 'The selected paper is no longer supported.'}
    $live = PaperInfo $settings $paper
    if ([Math]::Abs($live.width-$r.width) -gt .01 -or [Math]::Abs($live.height-$r.height) -gt .01) {throw 'Paper dimensions changed. Refresh the printer settings.'}
    for ($i=0; $i -lt 4; $i++) {if ([Math]::Abs($live.margins[$i]-$r.margins[$i]) -gt .01) {throw 'Printer margins changed. Refresh the printer settings.'}}
    $settings.Copies = [int16]$r.copies
    $settings.Collate = $true
    if ($r.sides -and $r.sides -ne 'one-sided') {
        if (-not $settings.CanDuplex) {throw 'This printer does not support duplex.'}
        $settings.Duplex = if ($r.sides -eq 'two-sided-long-edge') {[System.Drawing.Printing.Duplex]::Vertical} else {[System.Drawing.Printing.Duplex]::Horizontal}
    } else {$settings.Duplex = [System.Drawing.Printing.Duplex]::Simplex}
    $document = New-Object System.Drawing.Printing.PrintDocument
    try {
        $document.PrinterSettings = $settings
        $document.DocumentName = $r.title
        $document.DefaultPageSettings.PaperSize = $paper
        $document.DefaultPageSettings.Landscape = [bool]$r.landscape
        $document.DefaultPageSettings.Color = (-not [bool]$r.grayscale) -and $settings.SupportsColor
        $document.DefaultPageSettings.Margins = New-Object System.Drawing.Printing.Margins(0,0,0,0)
        if ($r.tray) {
            $tray = $settings.PaperSources | Where-Object {[string]$_.RawKind -eq $r.tray} | Select-Object -First 1
            if ($null -eq $tray) {throw 'Paper source is no longer available.'}
            $document.DefaultPageSettings.PaperSource = $tray
        }
        if ($r.quality) {
            $quality = $settings.PrinterResolutions | Where-Object {('{0},{1},{2}' -f [int]$_.Kind,$_.X,$_.Y) -eq $r.quality} | Select-Object -First 1
            if ($null -eq $quality) {throw 'Print quality is no longer supported.'}
            $document.DefaultPageSettings.PrinterResolution = $quality
        }
        $document.OriginAtMargins = $false
        $document.PrintController = New-Object System.Drawing.Printing.StandardPrintController
        $script:pageIndex = 0
        $document.add_PrintPage({
            param($sender,$event)
            $image = [System.Drawing.Image]::FromFile($r.files[$script:pageIndex])
            try {
                $event.Graphics.PageUnit = [System.Drawing.GraphicsUnit]::Display
                $event.Graphics.TranslateTransform(-$event.PageSettings.HardMarginX, -$event.PageSettings.HardMarginY)
                $event.Graphics.DrawImage($image, [System.Drawing.RectangleF]::new(0,0,$event.PageBounds.Width,$event.PageBounds.Height))
            } finally {$image.Dispose()}
            $script:pageIndex++
            $event.HasMorePages = $script:pageIndex -lt $r.files.Count
        })
        $document.Print()
    } finally {$document.Dispose()}
    @{message=('Sent to ' + $r.printer + '. Check the Windows print queue for progress.')} | ConvertTo-Json -Compress
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
