# Apply vendored patches to skia-safe
# Run this once after cloning or after running `cargo vendor`

$vendorDir = "$PSScriptRoot\..\vendor\skia-safe"
$patchesDir = "$PSScriptRoot"

if (-not (Test-Path "$vendorDir\Cargo.toml")) {
    Write-Error "vendor/skia-safe not found. Run cargo vendor first or ensure the directory exists."
    exit 1
}

$patchFiles = Get-ChildItem -Path $patchesDir -Filter "*.patch"
if ($patchFiles.Count -eq 0) {
    Write-Output "No .patch files found in $patchesDir"
    exit 0
}

foreach ($patch in $patchFiles) {
    Write-Output "Applying $($patch.Name) ..."
    # Use git apply if available, otherwise fallback to patch command
    $gitApply = Get-Command "git" -ErrorAction SilentlyContinue
    if ($gitApply) {
        & git -C $vendorDir apply --ignore-whitespace $patch.FullName 2>&1
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "git apply failed for $($patch.Name), trying patch.exe..."
            $patchCmd = Get-Command "patch" -ErrorAction SilentlyContinue
            if (-not $patchCmd) {
                Write-Error "Neither git apply nor patch.exe is available. Install Git for Windows or GnuWin32 patch."
                exit 1
            }
            & patch -d $vendorDir -p1 -i $patch.FullName
        }
    } else {
        $patchCmd = Get-Command "patch" -ErrorAction SilentlyContinue
        if (-not $patchCmd) {
            Write-Error "Neither git nor patch.exe is available. Install Git for Windows or GnuWin32 patch."
            exit 1
        }
        & patch -d $vendorDir -p1 -i $patch.FullName
    }
}

Write-Output "Done. Patches applied to vendor/skia-safe."
