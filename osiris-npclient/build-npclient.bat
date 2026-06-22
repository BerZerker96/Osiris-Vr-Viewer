@echo off
REM ===========================================================================
REM  Osiris TrackIR bridge - build NPClient64.dll (and optional 32-bit)
REM
REM  Builds the small client DLL that TrackIR/NPClient games load to read the
REM  head pose Osiris writes to FreeTrack shared memory. Completely separate
REM  from the Rust viewer build (build.bat) - it does not touch cargo, the
REM  workspace, or the viewer.
REM
REM  You can just DOUBLE-CLICK this file. It auto-detects the Visual Studio C++
REM  tools (the same ones the viewer build uses) and sets them up itself, so you
REM  do NOT need to open a 'Native Tools Command Prompt' manually anymore.
REM
REM  USAGE:
REM     build-npclient.bat          builds NPClient64.dll  (64-bit, for ES2 and
REM                                                          most modern games)
REM     build-npclient.bat 32       builds NPClient.dll     (32-bit, old games)
REM ===========================================================================

setlocal enableextensions enabledelayedexpansion
cd /d "%~dp0"

REM --- Pick target architecture from the first argument (default x64) ---
set "ARCH=x64"
set "OUT=NPClient64.dll"
set "LINKDEF="
if /I "%~1"=="32" (
    set "ARCH=x86"
    set "OUT=NPClient.dll"
    REM 32-bit __stdcall needs the .def for clean, undecorated export names.
    set "LINKDEF=/link /DEF:NPClient.def"
)

REM --- If cl.exe is not already on PATH, locate VS and initialize its env ---
where cl >nul 2>nul
if errorlevel 1 (
    echo Setting up the Visual Studio C++ build environment ^(!ARCH!^)...
    set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
    if not exist "!VSWHERE!" set "VSWHERE=%ProgramFiles%\Microsoft Visual Studio\Installer\vswhere.exe"
    set "VSPATH="
    if exist "!VSWHERE!" (
        for /f "usebackq tokens=*" %%i in (`"!VSWHERE!" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSPATH=%%i"
    )
    if defined VSPATH (
        if exist "!VSPATH!\VC\Auxiliary\Build\vcvarsall.bat" (
            call "!VSPATH!\VC\Auxiliary\Build\vcvarsall.bat" !ARCH! >nul
        )
    )
)

REM --- Verify the compiler is now available ---
where cl >nul 2>nul
if errorlevel 1 (
    echo.
    echo [ERROR] Could not find the MSVC C compiler ^(cl.exe^).
    echo   Install Visual Studio or the 'Build Tools for Visual Studio' with the
    echo   'Desktop development with C++' workload ^(this is the same toolset the
    echo   viewer build needs^), then run this again.
    echo   Alternatively, run this from the 'x64 Native Tools Command Prompt for VS'.
    echo.
    pause
    exit /b 1
)

echo.
echo Building !OUT!  ^(!ARCH!^) ...
cl /nologo /O2 /LD /DNDEBUG npclient.c /Fe:!OUT! !LINKDEF!
if errorlevel 1 (
    echo.
    echo [FAILED] Compilation failed - see the messages above.
    pause
    exit /b 1
)

REM --- Clean up intermediate files ---
del /q npclient.obj *.exp *.lib 2>nul

echo.
echo [OK] !OUT! built in: %cd%
echo.
echo Next: see README.md in this folder for where to copy the DLL.
echo       ^(Everspace 2: replace the NPClient64.dll inside the ES2 game folder.^)
echo.
pause
endlocal
