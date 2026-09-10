$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$expected = @{
  "brand/voxely-icon.png" = "e2709443acf2348f68dd8bde6e6c07fb95677046172ffc0cafea8aefe6caeb2a"
  "src-tauri/icons/icon.png" = "c2ee43a8000dcaab96d643caf9cf2e45467cee94de81c0e32745a630a4ea1597"
  "src-tauri/icons/icon.ico" = "baebfcba353e03ec1147881e064a6aefcb083aa3c8045131029d93621c1ecac0"
  "src-tauri/icons/32x32.png" = "f3a749af233364476fca3a86bdcb1565dc6ae25befa54b8be075afa19e739b34"
  "src-tauri/icons/128x128.png" = "63bbd8ca4eedce22c78fe44ad8335758898c49baedbd9f10b96f008e2165e3c3"
}

foreach ($rel in $expected.Keys) {
  $path = Join-Path $root $rel
  if (-not (Test-Path $path)) {
    throw "Missing icon asset $rel"
  }
  $hash = (Get-FileHash $path -Algorithm SHA256).Hash.ToLower()
  if ($hash -ne $expected[$rel]) {
    throw "Icon hash mismatch for ${rel}: $hash"
  }
}

$required = @(
  "src-tauri/icons/128x128@2x.png",
  "src-tauri/icons/icon.icns",
  "public/favicon.png"
)
foreach ($rel in $required) {
  if (-not (Test-Path (Join-Path $root $rel))) {
    throw "Missing icon asset $rel"
  }
}

Write-Host "Icon SSOT hashes match"
