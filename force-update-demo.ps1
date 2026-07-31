#!/usr/bin/env pwsh
# Script para forzar actualización completa del demo

Write-Host "🔄 FORZANDO ACTUALIZACIÓN COMPLETA DEL DEMO..." -ForegroundColor Cyan
Write-Host ""

# 1. Detener cualquier servidor corriendo
Write-Host "🛑 Deteniendo servidores..." -ForegroundColor Yellow
Get-Process | Where-Object {$_.ProcessName -like "*node*"} | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

# 2. Limpiar cache de npm
Write-Host "🧹 Limpiando cache..." -ForegroundColor Yellow
Set-Location ../runbox-demo
npm cache clean --force 2>$null

# 3. Recompilar WASM
Write-Host "🔨 Recompilando WASM..." -ForegroundColor Yellow
Set-Location ../runbox
wasm-pack build --target web --release --out-dir pkg

# 4. Eliminar completamente node_modules/runboxjs
Write-Host "🗑️ Eliminando archivos antiguos..." -ForegroundColor Yellow
Remove-Item ../runbox-demo/node_modules/runboxjs -Recurse -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

# 5. Recrear directorio y copiar archivos
Write-Host "📦 Copiando archivos nuevos..." -ForegroundColor Yellow
New-Item -ItemType Directory -Path ../runbox-demo/node_modules/runboxjs -Force | Out-Null
Copy-Item pkg/* ../runbox-demo/node_modules/runboxjs/ -Force

# 6. Agregar timestamp para cache busting
$timestamp = Get-Date -Format "yyyyMMddHHmmss"
Write-Host "⏰ Timestamp para cache busting: $timestamp" -ForegroundColor Green

# 7. Crear archivo de versión
"$timestamp" | Out-File -FilePath ../runbox-demo/node_modules/runboxjs/version.txt -Encoding utf8

Write-Host ""
Write-Host "✅ ACTUALIZACIÓN COMPLETA TERMINADA" -ForegroundColor Green
Write-Host "🚀 Ahora inicia el servidor manualmente:" -ForegroundColor Cyan
Write-Host "   cd runbox-demo" -ForegroundColor White
Write-Host "   npm run dev" -ForegroundColor White
Write-Host ""
Write-Host "💡 IMPORTANTE: Refresca el navegador con Ctrl+F5 para limpiar cache" -ForegroundColor Yellow