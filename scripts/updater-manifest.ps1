# Prepara gli aggiornamenti in-app per una release Windows (ADR 0023).
#
# - verifica che la versione del tag coincida con quella dell'app;
# - controlla che la firma dell'installer NSIS sia legata alla versione
#   (campo "version:" del commento firmato, richiesto da requireSignedVersion)
#   e, se manca, rifirma l'installer con la stessa chiave;
# - scrive latest.json, letto dall'app per scoprire la nuova versione.
#
# Richiede TAURI_SIGNING_PRIVATE_KEY e TAURI_SIGNING_PRIVATE_KEY_PASSWORD
# nell'ambiente (segreti del repository). Uso:
#   ./scripts/updater-manifest.ps1 -Tag v0.2.0 -Repository tihcio/greatnote-win
param(
    [Parameter(Mandatory = $true)][string]$Tag,
    [Parameter(Mandatory = $true)][string]$Repository,
    [string]$BundleDir = "target/release/bundle/nsis",
    [string]$Output = "target/release/bundle/latest.json",
    [string]$NotesFile = ""
)

$ErrorActionPreference = "Stop"

$config = Get-Content "src-tauri/tauri.conf.json" -Raw | ConvertFrom-Json
$version = $config.version
if ($Tag -ne "v$version") {
    throw "Il tag $Tag non corrisponde alla versione dell'app ($version): aggiorna la versione prima del tag."
}

$installers = @(Get-ChildItem -Path $BundleDir -Filter "*-setup.exe")
if ($installers.Count -ne 1) {
    throw "Atteso un solo installer NSIS in $BundleDir, trovati $($installers.Count)."
}
$installer = $installers[0]
$signaturePath = "$($installer.FullName).sig"
if (-not (Test-Path $signaturePath)) {
    throw "Firma mancante ($signaturePath): la build deve usare src-tauri/tauri.updater.conf.json."
}

function Get-SignedVersion([string]$path) {
    $decoded = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String((Get-Content $path -Raw).Trim()))
    $comment = ($decoded -split "`n" | Where-Object { $_ -like "trusted comment:*" } | Select-Object -First 1)
    foreach ($field in ($comment -replace "^trusted comment:\s*", "") -split "`t") {
        if ($field.StartsWith("version:")) { return $field.Substring(8).Trim() }
    }
    return $null
}

$signed = Get-SignedVersion $signaturePath
if ($signed -ne $version) {
    Write-Host "La firma non indica la versione ($signed): rifirmo l'installer per $version."
    npx tauri signer sign --app-version $version $installer.FullName
    if ($LASTEXITCODE -ne 0) { throw "Firma non riuscita." }
    $signed = Get-SignedVersion $signaturePath
    if ($signed -ne $version) { throw "La firma rigenerata non contiene la versione $version." }
}

$notes = "Greatnote $version"
if ($NotesFile -and (Test-Path $NotesFile)) {
    $notes = (Get-Content $NotesFile -Raw).Trim()
}
# GitHub sostituisce gli spazi dei nomi dei file allegati con punti.
$assetName = $installer.Name -replace " ", "."
$manifest = [ordered]@{
    version   = $version
    notes     = $notes
    pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = [ordered]@{
        "windows-x86_64" = [ordered]@{
            signature = (Get-Content $signaturePath -Raw).Trim()
            url       = "https://github.com/$Repository/releases/download/$Tag/$assetName"
        }
    }
}
# UTF-8 senza BOM, anche con Windows PowerShell 5.1.
$json = $manifest | ConvertTo-Json -Depth 5
[IO.File]::WriteAllText([IO.Path]::GetFullPath($Output), $json, (New-Object Text.UTF8Encoding $false))
Write-Host "Scritto $Output per la versione $version ($assetName)."
