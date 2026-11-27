# Scarica whisper.cpp per la trascrizione delle riunioni (ADR 0036) e lo
# prepara in src-tauri/resources/whisper, da cui il bundle Windows lo copia
# accanto all'app. Gemello di scripts/fetch-whisper.sh. La cartella è
# separata da quella di llama.cpp: le due build usano versioni diverse delle
# librerie ggml. Versione e hash sono fissati: un archivio diverso da quello
# previsto viene rifiutato.
#
# Uso (dalla radice del repository, prima di `npx tauri build` su Windows):
#   powershell -ExecutionPolicy Bypass -File scripts/fetch-whisper.ps1

$ErrorActionPreference = "Stop"

$Build = "b5907"
$Archive = "whisper-bin-x64.zip"
$Sha256 = "ecca07b277d943e9b9f4712856a144b857271a6b333104e511e1988e80db582b"
$ArchiveUrl = "https://github.com/ggml-org/whisper.cpp/releases/download/$Build/$Archive"
$LicenseUrl = "https://raw.githubusercontent.com/ggml-org/whisper.cpp/$Build/LICENSE"

$Root = Split-Path -Parent $PSScriptRoot
$Target = Join-Path $Root "src-tauri/resources/whisper"
$Marker = Join-Path $Target "VERSION.txt"

if ((Test-Path $Marker) -and ((Get-Content $Marker -Raw).Trim() -eq $Build)) {
    Write-Host "whisper.cpp $Build presente in $Target"
    exit 0
}

$Temp = Join-Path ([System.IO.Path]::GetTempPath()) ("greatnote-whisper-" + [guid]::NewGuid())
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
    $Release = Join-Path $Extracted "Release"

    if (Test-Path $Target) {
        Remove-Item -Recurse -Force $Target
    }
    New-Item -ItemType Directory -Path $Target | Out-Null
    # Solo whisper-cli e le librerie che carica (whisper e ggml): server,
    # test, SDL2 e Parakeet non servono all'app.
    Get-ChildItem -Path $Release -File | Where-Object {
        $_.Name -eq "whisper-cli.exe" -or $_.Name -eq "whisper.dll" -or $_.Name -like "ggml*.dll"
    } | Copy-Item -Destination $Target
    foreach ($Required in @("whisper-cli.exe", "whisper.dll", "ggml.dll", "ggml-base.dll")) {
        if (-not (Test-Path (Join-Path $Target $Required))) {
            throw "$Required non trovato nell'archivio"
        }
    }

    Invoke-WebRequest -Uri $LicenseUrl -OutFile (Join-Path $Target "LICENSE-whisper.cpp.txt") -UseBasicParsing
    Set-Content -Path $Marker -Value $Build -Encoding ascii
    Write-Host "whisper.cpp $Build pronto in $Target"
}
finally {
    Remove-Item -Recurse -Force $Temp -ErrorAction SilentlyContinue
}
