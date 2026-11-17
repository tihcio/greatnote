# Crea il pacchetto MSIX di Greatnote per il Microsoft Store (ADR 0029).
#
# Tauri genera solo NSIS e MSI: questo script impacchetta l'eseguibile già
# compilato (target/release/greatnote-desktop.exe), il motore llama.cpp e i loghi
# con gli strumenti del Windows SDK (makepri, makeappx).
#
# Il pacchetto per lo Store non va firmato: lo firma Microsoft dopo la
# certificazione. Per provarlo in locale usa -TestCertificate, che lo firma
# con un certificato autofirmato da rendere attendibile a mano.
#
# Uso (dopo `npx tauri build`):
#   ./scripts/build-msix.ps1
# L'identità del prodotto nello Store è indicata più sotto.
param(
    [string]$ExePath = "target/release/greatnote-desktop.exe",
    [string]$LlamaDir = "src-tauri/resources/llama",
    [string]$OutDir = "target/release/bundle/msix",
    [string]$IdentityName = $env:MSIX_IDENTITY_NAME,
    [string]$Publisher = $env:MSIX_PUBLISHER,
    [string]$PublisherDisplayName = $env:MSIX_PUBLISHER_DISPLAY_NAME,
    [string]$Version = "",
    [switch]$TestCertificate
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

# Identità assegnata da Partner Center (Gestione prodotto > Identità
# prodotto; Store ID 9P7KJXLWGP5P). Non è un segreto; le variabili
# d'ambiente servono solo a sovrascriverla.
if (-not $IdentityName) { $IdentityName = "TizianoAngeli.Greatnote" }
if (-not $Publisher) { $Publisher = "CN=F1EEA09B-E60D-40CC-8D0B-AF312F4E74C3" }
if (-not $PublisherDisplayName) { $PublisherDisplayName = "TizianoAngeli" }
if (-not $Version) {
    $appVersion = (Get-Content "src-tauri/tauri.conf.json" -Raw | ConvertFrom-Json).version
    # Lo Store richiede quattro numeri con l'ultimo a 0.
    $Version = "$appVersion.0"
}
if ($Version -notmatch '^\d+\.\d+\.\d+\.0$') {
    throw "Versione MSIX non valida: $Version (formato X.Y.Z.0)."
}
if (-not (Test-Path $ExePath)) {
    throw "Eseguibile non trovato: $ExePath. Compila prima l'app (npx tauri build)."
}

# Strumenti del Windows SDK più recente.
$kits = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
$sdkBin = Get-ChildItem $kits -Directory -Filter "10.*" -ErrorAction SilentlyContinue |
    Sort-Object { [version]$_.Name } -Descending |
    ForEach-Object { Join-Path $_.FullName "x64" } |
    Where-Object { Test-Path (Join-Path $_ "makeappx.exe") } |
    Select-Object -First 1
if (-not $sdkBin) {
    throw "Windows SDK non trovato (makeappx.exe in $kits): installa il Windows SDK."
}
$makeappx = Join-Path $sdkBin "makeappx.exe"
$makepri = Join-Path $sdkBin "makepri.exe"
$signtool = Join-Path $sdkBin "signtool.exe"

function Invoke-Tool([string]$tool, [string[]]$arguments) {
    & $tool @arguments
    if ($LASTEXITCODE -ne 0) { throw "$(Split-Path -Leaf $tool) non riuscito (codice $LASTEXITCODE)." }
}

$stage = Join-Path ([IO.Path]::GetTempPath()) ("greatnote-msix-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $stage | Out-Null
try {
    Copy-Item $ExePath (Join-Path $stage "greatnote-desktop.exe")
    if (Test-Path (Join-Path $LlamaDir "llama-server.exe")) {
        # Stessa posizione dell'installazione NSIS: cartella llama accanto all'app.
        Copy-Item -Recurse $LlamaDir (Join-Path $stage "llama")
    } else {
        Write-Warning "Motore llama.cpp assente in ${LlamaDir}: pacchetto senza AI integrata (scripts/fetch-llama.ps1)."
    }
    Copy-Item -Recurse "src-tauri/msix/Assets" (Join-Path $stage "Assets")

    $manifest = Get-Content "src-tauri/msix/AppxManifest.xml" -Raw -Encoding UTF8
    $manifest = $manifest.Replace("{{IDENTITY_NAME}}", [Security.SecurityElement]::Escape($IdentityName))
    $manifest = $manifest.Replace("{{PUBLISHER}}", [Security.SecurityElement]::Escape($Publisher))
    $manifest = $manifest.Replace("{{PUBLISHER_DISPLAY_NAME}}", [Security.SecurityElement]::Escape($PublisherDisplayName))
    $manifest = $manifest.Replace("{{VERSION}}", $Version)
    if ($manifest -match "\{\{[A-Z_]+\}\}") { throw "Segnaposto non sostituito nel manifest." }
    [IO.File]::WriteAllText((Join-Path $stage "AppxManifest.xml"), $manifest, (New-Object Text.UTF8Encoding $false))

    # resources.pri: varianti dei loghi per scala e dimensione (barra delle
    # applicazioni senza riquadro colorato).
    $priConfig = Join-Path ([IO.Path]::GetTempPath()) ("greatnote-priconfig-" + [guid]::NewGuid() + ".xml")
    Invoke-Tool $makepri @("createconfig", "/cf", $priConfig, "/dq", "it-IT_en-US", "/pv", "10.0.0", "/o")
    Invoke-Tool $makepri @("new", "/pr", $stage, "/cf", $priConfig, "/mn", (Join-Path $stage "AppxManifest.xml"), "/of", (Join-Path $stage "resources.pri"), "/o")
    Remove-Item $priConfig -ErrorAction SilentlyContinue

    New-Item -ItemType Directory -Force $OutDir | Out-Null
    $package = Join-Path (Resolve-Path $OutDir) ("Greatnote_" + $Version + "_x64.msix")
    Invoke-Tool $makeappx @("pack", "/d", $stage, "/p", $package, "/o", "/h", "SHA256")
    Write-Host "Creato $package ($IdentityName, $Publisher)."

    if ($TestCertificate) {
        # Solo per prove locali: il certificato va aggiunto a "Persone
        # attendibili" del computer per installare il pacchetto.
        $cert = New-SelfSignedCertificate -Type Custom -Subject $Publisher -KeyUsage DigitalSignature `
            -FriendlyName "Greatnote MSIX test" -CertStoreLocation "Cert:\CurrentUser\My" `
            -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
        $cerPath = Join-Path (Resolve-Path $OutDir) "Greatnote-test.cer"
        Export-Certificate -Cert $cert -FilePath $cerPath | Out-Null
        Invoke-Tool $signtool @("sign", "/fd", "SHA256", "/sha1", $cert.Thumbprint, "/s", "My", $package)
        Write-Host "Firmato con un certificato di prova: $cerPath"
    }
}
finally {
    Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
}
