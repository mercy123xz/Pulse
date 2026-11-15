
# Pulse - Windows EXE Code Signing
# Creates or reuses a local Pulse code-signing certificate,
# exports the public certificate, signs the EXE, and verifies it.

param(
    [Parameter(Mandatory = $true)]
    [string]$ExePath,

    [string]$CertDir = (Join-Path $env:LOCALAPPDATA "Pulse\Certificates"),

    [string]$PublicCerOut,

    # Optional: lets the build sign without a prompt. Leave it out to be asked.
    [SecureString]$Password
)

$ErrorActionPreference = "Stop"

try {
    Write-Host "`n========== PULSE CODE SIGNING ==========" -ForegroundColor Cyan

    # 1. Validate executable
    if (-not (Test-Path -LiteralPath $ExePath -PathType Leaf)) {
        throw "Executable not found: $ExePath"
    }

    $ExePath = (Resolve-Path -LiteralPath $ExePath).Path

    if ([System.IO.Path]::GetExtension($ExePath) -ne ".exe") {
        throw "The specified file must be an EXE."
    }

    # 2. Configure certificate paths
    New-Item -ItemType Directory -Force -Path $CertDir | Out-Null

    $pfxPath = Join-Path $CertDir "pulse-codesign.pfx"

    if ([string]::IsNullOrWhiteSpace($PublicCerOut)) {
        $PublicCerOut = Join-Path (Split-Path $ExePath -Parent) "pulse-public.cer"
    }

    $publicDir = Split-Path $PublicCerOut -Parent

    if ($publicDir) {
        New-Item -ItemType Directory -Force -Path $publicDir | Out-Null
    }

    # 3. Request PFX password securely
    if ($Password) {
        $securePassword = $Password
    }
    else {
        Write-Host "`nEnter the password for Pulse's signing certificate." -ForegroundColor Yellow
        $securePassword = Read-Host "PFX Password" -AsSecureString
    }

    # 4. Create or reuse certificate
    if (-not (Test-Path -LiteralPath $pfxPath)) {

        Write-Host "`nCreating Pulse code-signing certificate..." -ForegroundColor Cyan

        $cert = New-SelfSignedCertificate `
            -Type CodeSigningCert `
            -Subject "CN=Pulse, O=Pulse, C=US" `
            -KeyAlgorithm RSA `
            -KeyLength 2048 `
            -HashAlgorithm SHA256 `
            -CertStoreLocation "Cert:\CurrentUser\My" `
            -KeyExportPolicy Exportable `
            -NotAfter (Get-Date).AddYears(5)

        Export-PfxCertificate `
            -Cert $cert `
            -FilePath $pfxPath `
            -Password $securePassword | Out-Null

        Write-Host "Pulse certificate created and exported." -ForegroundColor Green

        $thumbprint = $cert.Thumbprint
    }
    else {

        Write-Host "`nReusing existing Pulse certificate..." -ForegroundColor Cyan

        $imported = Import-PfxCertificate `
            -FilePath $pfxPath `
            -CertStoreLocation "Cert:\CurrentUser\My" `
            -Password $securePassword `
            -Exportable

        $thumbprint = $imported.Thumbprint
    }

    # 5. Retrieve certificate from Windows certificate store
    $signCert = Get-ChildItem "Cert:\CurrentUser\My" |
        Where-Object {
            $_.Thumbprint -eq $thumbprint -and $_.HasPrivateKey
        } |
        Select-Object -First 1

    if (-not $signCert) {
        throw "Could not retrieve Pulse's certificate with its private key."
    }

    # 6. Validate certificate
    if ($signCert.NotAfter -le (Get-Date)) {
        throw "Pulse's signing certificate has expired."
    }

    $ekuExtension = $signCert.Extensions |
        Where-Object { $_.Oid.Value -eq "2.5.29.37" } |
        Select-Object -First 1

    if (-not $ekuExtension) {
        throw "Certificate is missing its Enhanced Key Usage extension."
    }

    $eku = [System.Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]$ekuExtension

    $codeSigningAllowed = $eku.EnhancedKeyUsages |
        Where-Object { $_.Value -eq "1.3.6.1.5.5.7.3.3" }

    if (-not $codeSigningAllowed) {
        throw "Certificate does not have Code Signing permissions."
    }

    Write-Host "Certificate validation passed." -ForegroundColor Green
    Write-Host "Publisher: $($signCert.Subject)"
    Write-Host "Thumbprint: $($signCert.Thumbprint)"
    Write-Host "Expires: $($signCert.NotAfter)"

    # 7. Export public certificate
    if (Test-Path -LiteralPath $PublicCerOut) {
        Remove-Item -LiteralPath $PublicCerOut -Force
    }

    Export-Certificate `
        -Cert $signCert `
        -FilePath $PublicCerOut `
        -Type CERT | Out-Null

    Write-Host "`nPublic certificate exported." -ForegroundColor Green

    # 8. Sign Pulse executable
    Write-Host "`nSigning Pulse executable..." -ForegroundColor Cyan

    $signResult = Set-AuthenticodeSignature `
        -FilePath $ExePath `
        -Certificate $signCert `
        -TimestampServer "http://timestamp.digicert.com" `
        -HashAlgorithm SHA256

    if ($signResult.Status -ne "Valid") {
        throw "Signing failed: $($signResult.Status) - $($signResult.StatusMessage)"
    }

    # 9. Independently verify final signature
    Write-Host "`nVerifying final signature..." -ForegroundColor Cyan

    $verification = Get-AuthenticodeSignature -FilePath $ExePath

    if ($verification.Status -ne "Valid") {
        throw "Signature verification failed: $($verification.Status) - $($verification.StatusMessage)"
    }

    # 10. Report results
    Write-Host "`n========== SIGNING COMPLETE ==========" -ForegroundColor Green

    Write-Host "Executable: $ExePath"
    Write-Host "Publisher: $($verification.SignerCertificate.Subject)"
    Write-Host "Signature: $($verification.Status)"
    Write-Host "Certificate: $PublicCerOut"
    Write-Host "Private PFX: $pfxPath"

    if ($verification.TimeStamperCertificate) {
        Write-Host "Timestamp: Present" -ForegroundColor Green
    }
    else {
        Write-Host "Timestamp: Not detected" -ForegroundColor Yellow
    }

    Write-Host "`nPulse executable successfully signed and verified." -ForegroundColor Green
}
catch {
    Write-Host "`n========== SIGNING FAILED ==========" -ForegroundColor Red
    Write-Host $_.Exception.Message -ForegroundColor Red
    exit 1
}