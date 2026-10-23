# Prepara gli aggiornamenti in-app per una release Windows (ADR 0023).
#
# - verifica che la versione del tag coincida con quella dell'app;
# - controlla che le firme degli installer NSIS e MSI siano legate alla
#   versione (campo "version:" del commento firmato, richiesto da
#   requireSignedVersion) e, se manca, rifirma l'installer con la stessa chiave;
# - scrive latest.json, letto dall'app per scoprire la nuova versione. Il
#   plugin cerca prima windows-x86_64-<nsis|msi>, secondo il tipo di
#   installazione: chi ha installato dal .msi deve ricevere il .msi, perché
#   l'installer NSIS installerebbe una seconda copia in un'altra cartella.
#
# Richiede TAURI_SIGNING_PRIVATE_KEY e TAURI_SIGNING_PRIVATE_KEY_PASSWORD
# nell'ambiente (segreti del repository). Uso:
#   ./scripts/updater-manifest.ps1 -Tag v0.2.0 -Repository tihcio/greatnote
param(
    [Parameter(Mandatory = $true)][string]$Tag,
    [Parameter(Mandatory = $true)][string]$Repository,
    [string]$BundleDir = "target/release/bundle/nsis",
    [string]$MsiDir = "target/release/bundle/msi",
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
# WiX crea un .msi per lingua: gli aggiornamenti usano quello italiano (la
# lingua dell'installer non cambia quella dell'app).
$msis = @(Get-ChildItem -Path $MsiDir -Filter "*_it-IT.msi")
if ($msis.Count -ne 1) {
    throw "Atteso un solo installer MSI italiano in $MsiDir, trovati $($msis.Count)."
}

function Get-SignedVersion([string]$path) {
    $decoded = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String((Get-Content $path -Raw).Trim()))
    $comment = ($decoded -split "`n" | Where-Object { $_ -like "trusted comment:*" } | Select-Object -First 1)
    foreach ($field in ($comment -replace "^trusted comment:\s*", "") -split "`t") {
        if ($field.StartsWith("version:")) { return $field.Substring(8).Trim() }
    }
    return $null
}

# Firma dell'installer legata alla versione; restituisce la voce di latest.json.
function Get-PlatformEntry($installer) {
    $signaturePath = "$($installer.FullName).sig"
    if (-not (Test-Path $signaturePath)) {
        throw "Firma mancante ($signaturePath): la build deve usare src-tauri/tauri.updater.conf.json."
    }
    $signed = Get-SignedVersion $signaturePath
    if ($signed -ne $version) {
        Write-Host "La firma di $($installer.Name) non indica la versione ($signed): rifirmo per $version."
        npx tauri signer sign --app-version $version $installer.FullName
        if ($LASTEXITCODE -ne 0) { throw "Firma non riuscita." }
        $signed = Get-SignedVersion $signaturePath
        if ($signed -ne $version) { throw "La firma rigenerata non contiene la versione $version." }
    }
    # GitHub sostituisce gli spazi dei nomi dei file allegati con punti.
    $assetName = $installer.Name -replace " ", "."
    return [ordered]@{
        signature = (Get-Content $signaturePath -Raw).Trim()
        url       = "https://github.com/$Repository/releases/download/$Tag/$assetName"
    }
}

$nsis = Get-PlatformEntry $installers[0]
$msi = Get-PlatformEntry $msis[0]

$notes = "Greatnote $version"
if ($NotesFile -and (Test-Path $NotesFile)) {
    $notes = (Get-Content $NotesFile -Raw).Trim()
}
$manifest = [ordered]@{
    version   = $version
    notes     = $notes
    pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = [ordered]@{
        # Voce generica per le versioni che non distinguono l'installer.
        "windows-x86_64"      = $nsis
        "windows-x86_64-nsis" = $nsis
        "windows-x86_64-msi"  = $msi
    }
}
# UTF-8 senza BOM, anche con Windows PowerShell 5.1.
$json = $manifest | ConvertTo-Json -Depth 5
[IO.File]::WriteAllText([IO.Path]::GetFullPath($Output), $json, (New-Object Text.UTF8Encoding $false))
Write-Host "Scritto $Output per la versione $version ($($installers[0].Name), $($msis[0].Name))."
