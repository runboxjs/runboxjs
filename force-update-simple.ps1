# Script para forzar actualización completa del demo

Write-Host "FORZANDO ACTUALIZACION COMPLETA DEL DEMO..." -ForegroundColor Cyan

# Detener servidores
Write-Host "Deteniendo servidores..." -ForegroundColor Yellow
Get-Process | Where-Object {$_.ProcessName -like "*node*"} | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

# Recompilar WASM
Write-Host "Recompilando WASM..." -ForegroundColor Yellow
wasm-pack build --target web --release --out-dir pkg

# Eliminar archivos antiguos
Write-Host "Eliminando archivos antiguos..." -ForegroundColor Yellow
Remove-Item ../runbox-demo/node_modules/runboxjs -Recurse -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

# Recrear y copiar
Write-Host "Copiando archivos nuevos..." -ForegroundColor Yellow
New-Item -ItemType Directory -Path ../runbox-demo/node_modules/runboxjs -Force | Out-Null
Copy-Item pkg/* ../runbox-demo/node_modules/runboxjs/ -Force

# Timestamp
$timestamp = Get-Date -Format "yyyyMMddHHmmss"
"$timestamp" | Out-File -FilePath ../runbox-demo/node_modules/runboxjs/version.txt -Encoding utf8

Write-Host "ACTUALIZACION COMPLETA TERMINADA" -ForegroundColor Green
Write-Host "Timestamp: $timestamp" -ForegroundColor Green
Write-Host "Ahora inicia el servidor manualmente: cd runbox-demo && npm run dev" -ForegroundColor Cyan
Write-Host "IMPORTANTE: Refresca el navegador con Ctrl+F5 para limpiar cache" -ForegroundColor Yellow