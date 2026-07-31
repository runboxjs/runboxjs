#!/usr/bin/env pwsh
# Script para forzar actualización de archivos WASM en el demo

Write-Host "🔄 Forzando actualización de archivos WASM..." -ForegroundColor Cyan

# Detener cualquier proceso que pueda estar usando los archivos
Write-Host "🛑 Deteniendo procesos..." -ForegroundColor Yellow
Get-Process | Where-Object {$_.ProcessName -like "*node*" -or $_.ProcessName -like "*npm*" -or $_.ProcessName -like "*vite*"} | Stop-Process -Force -ErrorAction SilentlyContinue

# Esperar un momento
Start-Sleep -Seconds 2

# Compilar WASM
Write-Host "🔨 Compilando WASM..." -ForegroundColor Yellow
wasm-pack build --target web --release --out-dir pkg
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Error al compilar WASM" -ForegroundColor Red
    exit 1
}

# Crear directorio temporal
$tempDir = "temp-update-$(Get-Random)"
New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

# Copiar archivos a temporal
Copy-Item "pkg/runbox.js" "$tempDir/" -Force
Copy-Item "pkg/runbox_bg.wasm" "$tempDir/" -Force  
Copy-Item "pkg/runbox.d.ts" "$tempDir/" -Force

# Eliminar archivos destino
$destDir = "../runbox-demo/node_modules/runboxjs"
Remove-Item "$destDir/runbox.js" -Force -ErrorAction SilentlyContinue
Remove-Item "$destDir/runbox_bg.wasm" -Force -ErrorAction SilentlyContinue
Remove-Item "$destDir/runbox.d.ts" -Force -ErrorAction SilentlyContinue

# Esperar un momento
Start-Sleep -Seconds 1

# Mover archivos desde temporal
Move-Item "$tempDir/runbox.js" "$destDir/" -Force
Move-Item "$tempDir/runbox_bg.wasm" "$destDir/" -Force
Move-Item "$tempDir/runbox.d.ts" "$destDir/" -Force

# Limpiar temporal
Remove-Item $tempDir -Recurse -Force

Write-Host "Archivos WASM actualizados exitosamente" -ForegroundColor Green
Write-Host "Ahora puedes iniciar el servidor: cd ../runbox-demo; npm run dev" -ForegroundColor Cyan