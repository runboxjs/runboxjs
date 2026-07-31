#!/usr/bin/env pwsh
# Script para recompilar RunBox y actualizar el demo

Write-Host "🔨 Compilando RunBox a WASM..." -ForegroundColor Cyan
wasm-pack build --target web --release

if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Error al compilar RunBox" -ForegroundColor Red
    exit 1
}

Write-Host "✅ Compilación exitosa" -ForegroundColor Green
Write-Host ""
Write-Host "📦 Actualizando archivos en runbox-demo..." -ForegroundColor Cyan
Write-Host "⚠️  IMPORTANTE: Asegúrate de que el servidor de desarrollo esté detenido" -ForegroundColor Yellow
Write-Host ""

$demoPath = "../runbox-demo/node_modules/runboxjs"

# Verificar si el directorio existe
if (-not (Test-Path $demoPath)) {
    Write-Host "❌ No se encontró el directorio $demoPath" -ForegroundColor Red
    Write-Host "   Ejecuta 'npm install' en runbox-demo primero" -ForegroundColor Yellow
    exit 1
}

# Intentar copiar los archivos
try {
    Copy-Item -Path "pkg/runbox.js" -Destination "$demoPath/" -Force -ErrorAction Stop
    Copy-Item -Path "pkg/runbox_bg.wasm" -Destination "$demoPath/" -Force -ErrorAction Stop
    Copy-Item -Path "pkg/runbox.d.ts" -Destination "$demoPath/" -Force -ErrorAction Stop
    Copy-Item -Path "pkg/runbox_bg.wasm.d.ts" -Destination "$demoPath/" -Force -ErrorAction Stop
    
    Write-Host "✅ Archivos actualizados correctamente" -ForegroundColor Green
    Write-Host ""
    Write-Host "🚀 Ahora puedes iniciar el demo con:" -ForegroundColor Cyan
    Write-Host "   cd ../runbox-demo" -ForegroundColor White
    Write-Host "   npm run dev" -ForegroundColor White
} catch {
    Write-Host "❌ Error al copiar archivos: $_" -ForegroundColor Red
    Write-Host ""
    Write-Host "💡 Solución:" -ForegroundColor Yellow
    Write-Host "   1. Detén el servidor de desarrollo (Ctrl+C)" -ForegroundColor White
    Write-Host "   2. Ejecuta este script nuevamente" -ForegroundColor White
    exit 1
}
