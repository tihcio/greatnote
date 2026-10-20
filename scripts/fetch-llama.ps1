# Scarica il motore llama.cpp distribuito con Greatnote (ADR 0020) e lo
# prepara in src-tauri/resources/llama, da cui il bundle Windows lo copia
# accanto all'app. Versione e hash sono fissati: un archivio diverso da
# quello previsto viene rifiutato.
#
# Uso (dalla radice del repository, prima di `npx tauri build` su Windows):
#   powershell -ExecutionPolicy Bypass -File scripts/fetch-llama.ps1

$ErrorActionPreference = "Stop"

$Build = "b11397"
$Archive = "llama-$Build-bin-win-vulkan-x64.zip"
$Sha256 = "128ba6b5a3a89c40ba4a4a7b23f666bb61432c865163e8e8531e654b875f2bee"
$ArchiveUrl = "https://github.com/ggml-org/llama.cpp/releases/download/$Build/$Archive"
$LicenseUrl = "https://raw.githubusercontent.com/ggml-org/llama.cpp/$Build/LICENSE"

$Root = Split-Path -Parent $PSScriptRoot
$Target = Join-Path $Root "src-tauri/resources/llama"
$Marker = Join-Path $Target "VERSION.txt"

if ((Test-Path $Marker) -and ((Get-Content $Marker -Raw).Trim() -eq $Build)) {
    Write-Host "llama.cpp $Build presente in $Target"
    exit 0
}

$Temp = Join-Path ([System.IO.Path]::GetTempPath()) ("greatnote-llama-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $Temp | Out-Null
try {
    $ZipPath = Join-Path $Temp $Archive
    Write-Host "Scarico $ArchiveUrl"
    Invoke-WebRequest -Uri $ArchiveUrl -OutFile $ZipPath -UseBasicParsing
    $Actual = (Get-FileHash -Algorithm SHA256 $ZipPath).Hash.ToLowerInvariant()
    if ($Actual -ne $Sha256) {
        throw "Hash dell'archivio diverso dal previsto: $Actual"
    }

    $Extracted = Join-Path $Temp "extracted"
    Expand-Archive -Path $ZipPath -DestinationPath $Extracted

    if (Test-Path $Target) {
        Remove-Item -Recurse -Force $Target
    }
    New-Item -ItemType Directory -Path $Target | Out-Null
    # Solo il server, le sue librerie e le licenze: gli altri strumenti a
    # riga di comando non servono all'app.
    Get-ChildItem -Path $Extracted -File | Where-Object {
        $_.Extension -ne ".exe" -or $_.Name -eq "llama-server.exe"
    } | Copy-Item -Destination $Target
    if (-not (Test-Path (Join-Path $Target "llama-server.exe"))) {
        throw "llama-server.exe non trovato nell'archivio"
    }

    Invoke-WebRequest -Uri $LicenseUrl -OutFile (Join-Path $Target "LICENSE-llama.cpp.txt") -UseBasicParsing
    Set-Content -Path $Marker -Value $Build -Encoding ascii
    Write-Host "llama.cpp $Build pronto in $Target"
}
finally {
    Remove-Item -Recurse -Force $Temp -ErrorAction SilentlyContinue
}
