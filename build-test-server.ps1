# Збирає Horizon-сервер з модом Duels і створює тестовий сервер у цій папці.
# Запускай САМ: прапорець --trust-native-code означає, що ти довіряєш коду мода
# (нативні моди не в пісочниці). Прочитай mixins\duels\*.rs перед запуском.
#
#   powershell -ExecutionPolicy Bypass -File .\build-test-server.ps1
#
param(
  [string]$Version = "nightly-2026-11-03-2944480",  # встановлена збірка Horizon (список: horizon-launcher versions --installed)
  [int]$Port = 25572,                        # не 25565/25570, щоб не заважати іншим серверам
  [string]$Op = "roman",
  [switch]$NoRun                             # лише зібрати й налаштувати, не запускати
)
$ErrorActionPreference = "Stop"
$here   = Split-Path -Parent $MyInvocation.MyCommand.Path
$engine = Join-Path $here "test-server-engine"
$server = Join-Path $here "test-server"

if (Test-Path $engine) {
  $old = "$engine-old-$(Get-Date -Format yyyyMMdd-HHmmss)"
  Write-Host "Стара збірка -> $old"
  Rename-Item $engine $old
}

Write-Host "1/3 Збираю движок сервера з модом (перший раз довго: качається Rust і код Horizon)..."
horizon-launcher native-build (Join-Path $here "mixins\duels") --server-output $engine --version $Version --trust-native-code
if ($LASTEXITCODE -ne 0) { throw "Збірка не вдалась. Лог: native\build.log у папці даних лаунчера (кнопка Build log)." }

if (-not (Test-Path (Join-Path $server "server.toml"))) {
  Write-Host "2/3 Створюю тестовий сервер у $server (порт $Port, offline, оп $Op)..."
  horizon-server --dir $server init --eula --world duels_test --port $Port --offline --op $Op --yes
}

Write-Host "3/3 Підключаю зібраний движок і запускаю..."
horizon-server --dir $server config set "engine.path=$(Join-Path $engine 'horizon-engine.exe')"
if ($NoRun) { Write-Host "Готово. Запуск: horizon-server --dir `"$server`" run"; return }
horizon-server --dir $server run
