## Проблема: `Access is denied (0x80070005)` при запуске D2MXLUtils.exe напрямую

### 1. Контекст

- Бэкенд D2MXLUtils на Rust/Tauri 2 инжектится в процесс Diablo II (Median XL) через `OpenProcess` + `ReadProcessMemory`/`WriteProcessMemory` + `CreateRemoteThread`.
- В dev-режиме (`pnpm tauri dev`) инжекция работает стабильно.
- В релизной сборке (`pnpm tauri build` → `src-tauri/target/i686-pc-windows-msvc/release/D2MXLUtils.exe`) наблюдается проблема при запуске "напрямую".

### 2. Симптомы

- При запуске релизного `D2MXLUtils.exe` **двойным кликом из Explorer**:
  - в лог (`d2mxlutils.log`) пишется:
    - `[INFO] Scanner starting...`
    - `[ERROR] Failed to attach to Diablo II: Failed to open process: Access is denied. (0x80070005)`
  - инжекция не происходит.
- При запуске **из PowerShell**:
  - если запустить **один и тот же exe** командой:
    - `& C:\Users\...\D2MXLUtils\src-tauri\target\i686-pc-windows-msvc\release\D2MXLUtils.exe`
  - и запустить Diablo II под тем же пользователем/уровнем прав,
  - инжекция **работает**, ошибки `ACCESS_DENIED` нет.
- Та же ошибка воспроизводится у друга на **нативной Windows x64** (не Parallels), при запуске exe "напрямую" (через Explorer).

Вывод: код инжекции и права, выданные процессу при запуске из PowerShell, достаточны; проблема появляется только при определённых сценариях запуска через Explorer/UAC.

### 3. Что уже пробовали

- Проверяли UAC и уровень прав:
  - процессы игры и D2MXLUtils запускались как с elevation, так и без;
  - сценарий "оба процесса запущены из одного admin-PowerShell" всегда работает.
- Пытались решить через конфиг Tauri:
  - добавляли в `tauri.conf.json`:
    - `bundle.windows.allowElevation = true`
  - **Tauri 2** не знает такой опции → валидация конфигурации падает:
    - `bundle > windows: Additional properties are not allowed ('allowElevation' was unexpected)`.
- Пытались встроить кастомный `app.manifest` с `requestedExecutionLevel="requireAdministrator"` через `winres`:
  - добавили `winres` в `[build-dependencies]` и вызвали `WindowsResource::set_manifest_file("app.manifest")` в `build.rs`;
  - линковка упала с ошибкой:
    - `CVTRES : fatal error CVT1100: duplicate resource. type:VERSION, name:1`
    - `LINK : fatal error LNK1123: failure during conversion to COFF`
  - причина: и Tauri, и winres кладут свой `VERSION`-ресурс → конфликт.
- После этого все изменения по winres/manifest/allowElevation были **откачены**, конфиг снова валиден для Tauri 2.

### 4. Текущее понимание причины

- Сама ошибка `0x80070005` приходит из `OpenProcess`:
  - `Access is denied` означает, что текущий процесс **не обладает достаточными правами** для запрашиваемого access mask к целевому PID.
- Так как:
  - **один и тот же** `D2MXLUtils.exe` из PowerShell может открыть процесс игры и инжектиться;
  - но тот же файл при запуске из Explorer получает отказ,
  - проблема лежит не в Rust-коде и не в таргете (`i686`/`x86_64`), а в **модели прав/токенов**, которые Windows выдаёт процессу в разных сценариях запуска (Explorer/UAC vs консоль).
- В Tauri 2 **нет** официального поля в `tauri.conf.json`, чтобы включить `requireAdministrator` или аналогичный уровень UAC — опция `bundle.windows.allowElevation` принадлежала Tauri 1 и теперь считается невалидной.
- Попытка "насильно" вшить свой manifest через winres приводит к конфликту ресурсов с тем, что генерирует сам Tauri, и требует более сложной кастомизации ресурсо-таблицы.

### 5. Рабочий обход на сейчас

- **Гарантированно рабочий сценарий**:
  - открыть PowerShell **от имени администратора**;
  - запустить из него:
    - сначала Diablo II (`Game.exe` / лаунчер),
    - затем `D2MXLUtils.exe` (релизный exe);
  - в этом случае оба процесса явно наследуют один и тот же elevated-токен → `OpenProcess` работает.
- Для пользователя можно оформить это в виде отдельного **launcher-скрипта** (`.ps1` или `.bat`), который:
  - запускается "От имени администратора";
  - стартует игру и утилиту под одним и тем же аккаунтом/уровнем прав.

### 6. Идеи для будущего решения

- **Аккуратный кастомный manifest/ресурсы**:
  - найти способ добавить `requestedExecutionLevel="requireAdministrator"` без конфликта с генерацией ресурсов Tauri 2;
  - возможно, через замену части ресурсо-таблицы post-link (потребует отдельного исследования).
- **Отдельный launcher.exe**:
  - маленький вспомогательный exe (или инсталлер), который явно требует elevation и уже из себя запускает игру и D2MXLUtils;
  - основной D2MXLUtils.exe может тогда работать с более мягким manifest'ом.
- **Альтернативная модель доступа**:
  - исследовать, можно ли для доступа к процессу Diablo II обойтись более узким набором прав `OpenProcess`, который менее чувствителен к UAC/Integrity Level.


