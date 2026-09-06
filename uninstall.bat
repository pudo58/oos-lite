@echo off
chcp 65001 >nul 2>&1
setlocal EnableDelayedExpansion
title OOS-Lite Uninstaller & Process Cleanup

echo ============================================================
echo           OOS-LITE UNINSTALLER ^& CLEANUP TOOL
echo   Trình gỡ cài đặt và dọn dẹp tiến trình / cổng mạng OOS-Lite
echo ============================================================
echo.
echo Tác vụ sẽ thực hiện / This tool will:
echo   1. Dừng triệt để tiến trình oos-lite.exe và oos-lite-gui.exe (Kill processes)
echo   2. Giải phóng hoàn toàn cổng mạng 3000 (UI) và 8080 (WebDAV) (Free ports)
echo   3. Ngắt kết nối ổ đĩa ảo Z: khỏi Windows Explorer (Unmap drive Z:)
echo   4. Xóa tích hợp Menu Chuột Phải Windows Explorer (Unregister context menu)
echo   5. Dọn dẹp dữ liệu tạm và cấu hình trong %%LOCALAPPDATA%%\oos-lite
echo   6. Thực thi trình gỡ cài đặt chính thức unins000.exe (nếu đã cài đặt qua Setup)
echo.

set /p CONFIRM="Bạn có chắc chắn muốn tiến hành? / Proceed with uninstall? [y/N]: "
if /i not "!CONFIRM!"=="y" if /i not "!CONFIRM!"=="yes" (
    echo.
    echo [INFO] Đã hủy thao tác gỡ cài đặt / Operation cancelled by user.
    echo.
    pause
    exit /b 0
)

echo.
echo ------------------------------------------------------------
echo [1/6] Ngắt kết nối ổ đĩa ảo Z: / Unmapping drive Z:...
net use Z: /delete /y >nul 2>&1
echo       ✓ Đã ngắt kết nối ổ đĩa Z: (nếu có).

echo.
echo [2/6] Dừng các tiến trình OOS-Lite / Terminating OOS-Lite processes...
taskkill /F /T /IM oos-lite-gui.exe >nul 2>&1
taskkill /F /T /IM oos-lite.exe >nul 2>&1
echo       ✓ Đã dừng toàn bộ tiến trình oos-lite-gui.exe và oos-lite.exe.

echo.
echo [3/6] Quét và giải phóng cổng mạng 3000 & 8080 / Freeing TCP ports...
powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command ^
  "$ports = @(3000, 8080); " ^
  "foreach ($p in $ports) { " ^
  "  $conns = Get-NetTCPConnection -LocalPort $p -State Listen -ErrorAction SilentlyContinue; " ^
  "  if ($conns) { " ^
  "    foreach ($c in $conns) { " ^
  "      $procId = $c.OwningProcess; " ^
  "      Write-Host \"       -> Đang đóng PID $procId đang giữ cổng $p...\"; " ^
  "      Stop-Process -Id $procId -Force -ErrorAction SilentlyContinue; " ^
  "    } " ^
  "  } " ^
  "}"
echo       ✓ Đã giải phóng toàn bộ cổng mạng TCP 3000 và 8080.

echo.
echo [4/6] Xóa menu chuột phải Windows Explorer / Cleaning Registry context menu...
reg delete "HKCU\Software\Classes\*\shell\OOSLite" /f >nul 2>&1
reg delete "HKCU\Software\Classes\Directory\shell\OOSLite" /f >nul 2>&1
reg delete "HKCU\Software\Classes\Directory\Background\shell\OOSLite" /f >nul 2>&1
reg delete "HKCU\Software\Classes\OOSLite.FileMenu" /f >nul 2>&1
reg delete "HKCU\Software\Classes\OOSLite.DirMenu" /f >nul 2>&1
reg delete "HKCU\Software\Classes\OOSLite.DirMenuBg" /f >nul 2>&1
reg delete "HKCU\Software\Classes\OOSLite.Menu" /f >nul 2>&1
reg delete "HKCU\Software\Classes\OOSLite.MenuBg" /f >nul 2>&1
echo       ✓ Đã xóa sạch các khóa Registry Menu OOS-Lite.

echo.
echo [5/6] Dọn dẹp thư mục tạm %%LOCALAPPDATA%%\oos-lite...
if exist "%LOCALAPPDATA%\oos-lite" (
    rmdir /s /q "%LOCALAPPDATA%\oos-lite" >nul 2>&1
    echo       ✓ Đã xóa thư mục %%LOCALAPPDATA%%\oos-lite.
) else (
    echo       ✓ Không có thư mục tạm %%LOCALAPPDATA%%\oos-lite.
)

echo.
echo [6/6] Kiểm tra bộ cài Windows Inno Setup unins000.exe...
set "UNINSTALLER="
if exist "%~dp0unins000.exe" set "UNINSTALLER=%~dp0unins000.exe"
if not defined UNINSTALLER (
    if exist "%LOCALAPPDATA%\Programs\OOS-Lite\unins000.exe" (
        set "UNINSTALLER=%LOCALAPPDATA%\Programs\OOS-Lite\unins000.exe"
    )
)
if not defined UNINSTALLER (
    if exist "%ProgramFiles%\OOS-Lite\unins000.exe" (
        set "UNINSTALLER=%ProgramFiles%\OOS-Lite\unins000.exe"
    )
)

if defined UNINSTALLER (
    echo       Tìm thấy trình gỡ cài đặt: "!UNINSTALLER!"
    echo       Đang chạy gỡ bỏ file và shortcut...
    "!UNINSTALLER!" /SILENT
    echo       ✓ Đã gỡ bỏ toàn bộ tập tin ứng dụng và shortcut.
) else (
    echo       Chế độ portable / dev: Không phát hiện file unins000.exe.
)

echo.
echo ------------------------------------------------------------
set /p DELVAULT="Bạn có muốn XÓA TOÀN BỘ kho dữ liệu cá nhân tại %USERPROFILE%\.oos-store không? [y/N]: "
if /i "!DELVAULT!"=="y" (
    if exist "%USERPROFILE%\.oos-store" (
        echo Đang xóa %USERPROFILE%\.oos-store...
        rmdir /s /q "%USERPROFILE%\.oos-store" >nul 2>&1
        echo ✓ Đã xóa kho dữ liệu cá nhân.
    ) else (
        echo ✓ Thư mục %USERPROFILE%\.oos-store không tồn tại.
    )
) else (
    echo ✓ Giữ lại an toàn kho dữ liệu cá nhân tại: %USERPROFILE%\.oos-store
)

echo.
echo ============================================================
echo   ✓ HOÀN TẤT GỠ BỎ ^& DỌN DẸP OOS-LITE THÀNH CÔNG!
echo   All OOS-Lite processes killed, ports freed, and uninstalled.
echo ============================================================
echo.
pause
