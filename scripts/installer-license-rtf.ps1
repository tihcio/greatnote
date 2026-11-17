# Genera src-tauri/installer/license-and-privacy.rtf, mostrato dagli
# installer NSIS e MSI (ADR 0028), dal testo in license-and-privacy.txt.
# Il bundler di Tauri convertirebbe da solo il .txt per WiX, ma senza
# codificare le lettere accentate, che risulterebbero illeggibili.
#
# Dopo ogni modifica del .txt:
#   powershell -ExecutionPolicy Bypass -File scripts/installer-license-rtf.ps1

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
$Source = Join-Path $Root "src-tauri/installer/license-and-privacy.txt"
$Output = Join-Path $Root "src-tauri/installer/license-and-privacy.rtf"

$text = [IO.File]::ReadAllText($Source, [Text.Encoding]::UTF8)
$cp1252 = [Text.Encoding]::GetEncoding(1252)
$builder = New-Object Text.StringBuilder
foreach ($char in $text.ToCharArray()) {
    $code = [int]$char
    if ($char -eq "`r") { continue }
    elseif ($char -eq "`n") { [void]$builder.Append("\par`r`n") }
    elseif ($char -eq "\" -or $char -eq "{" -or $char -eq "}") { [void]$builder.Append("\$char") }
    elseif ($code -lt 128) { [void]$builder.Append($char) }
    else {
        $bytes = $cp1252.GetBytes([string]$char)
        if ($bytes.Count -eq 1 -and $bytes[0] -ne 63) {
            [void]$builder.Append(("\'{0:x2}" -f $bytes[0]))
        } else {
            # Fuori da cp1252: carattere Unicode con alternativa "?".
            [void]$builder.Append("\u$code?")
        }
    }
}

$rtf = "{\rtf1\ansi\ansicpg1252\deff0{\fonttbl{\f0\fnil\fcharset0 Segoe UI;}}`r`n\f0\fs18`r`n" + $builder.ToString() + "}`r`n"
[IO.File]::WriteAllText($Output, $rtf, [Text.Encoding]::ASCII)
Write-Host "Scritto $Output"
