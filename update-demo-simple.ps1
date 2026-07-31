# Script simple para actualizar archivos WASM

Write-Host "Compilando WASM..." -ForegroundColor Yellow
wasm-pack build --target web --release --out-dir pkg

Write-Host "Creando directorio temporal..." -ForegroundColor Yellow
$tempDir = "temp-update-$(Get-Random)"
New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

Write-Host "Copiando archivos..." -ForegroundColor Yellow
Copy-Item "pkg/runbox.js" "$tempDir/" -Force
Copy-Item "pkg/runbox_bg.wasm" "$tempDir/" -Force  
Copy-Item "pkg/runbox.d.ts" "$tempDir/" -Force

Write-Host "Eliminando archivos destino..." -ForegroundColor Yellow
$destDir = "../runbox-demo/node_modules/runboxjs"
Remove-Item "$destDir/runbox.js" -Force -ErrorAction SilentlyContinue
Remove-Item "$destDir/runbox_bg.wasm" -Force -ErrorAction SilentlyContinue
Remove-Item "$destDir/runbox.d.ts" -Force -ErrorAction SilentlyContinue

Start-Sleep -Seconds 1

Write-Host "Moviendo archivos..." -ForegroundColor Yellow
Move-Item "$tempDir/runbox.js" "$destDir/" -Force
Move-Item "$tempDir/runbox_bg.wasm" "$destDir/" -Force
Move-Item "$tempDir/runbox.d.ts" "$destDir/" -Force

Remove-Item $tempDir -Recurse -Force

Write-Host "Archivos WASM actualizados exitosamente" -ForegroundColor Green