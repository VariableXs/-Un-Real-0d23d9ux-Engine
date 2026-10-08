@echo off
REM ============================================================
REM  VarixAutoPilot - 便携版一键启动
REM
REM  用途：不依赖安装包，exe + 运行时目录一起拷走就能用。
REM  适合：换机器、备份、给别的会话用。
REM
REM  ★ 与「安装版」的区别 ★
REM  安装版走 NSIS（要联网下载工具链，网络断了就打不出来）。
REM  便携版只依赖「运行时」目录 —— 那是 Tauri 打包时自动带出来的，
REM  拷走整个目录即可，不注册任何系统项、不写注册表。
REM
REM  ★ 坑（实测）★
REM  路径里有连字符（"-Un-Real-..."）——
REM  cmd 里必须给 set 赋值加引号，否则 cmd 会把连字符后的文本
REM  当命令解析，直接语法错误。别把引号"简化"掉。
REM ============================================================
setlocal

REM ★ 指向本bat 所在目录（换机器后只改这一行）★
set "AP_HOME=%~dp0"
set "AP_EXE=%AP_HOME%VarixAutoPilot.exe"

if not exist "%AP_EXE%" (
  echo [ERR] 找不到主程序:
  echo        %AP_EXE%
  echo.
  echo  这个 bat 必须和 VarixAutoPilot.exe 放在同一目录。
  pause
  exit /b 1
)

REM --- WorkBuddy 的调试端口是硬依赖 ---
REM 引擎靠 CDP 连WorkBuddy（9222）。端口不通就没有自动化能力。
curl -s -m 3 "http://127.0.0.1:9222/json/version" >nul 2>&1
if not errorlevel 1 (
  echo [OK] 端口 9222 就绪，启动 VarixAutoPilot...
  start "" "%AP_EXE%"
  endlocal
  exit /b 0
)

echo ============================================================
echo  WorkBuddy 调试端口 9222 未就绪 —— 无法自动化。
echo ============================================================
echo.
echo  端口是硬依赖：引擎靠它连 WorkBuddy 的页面，
echo  没有它就点不动、也发不出。
echo.
echo  怎么办：双击桌面的「VarixAutoPilot 开端口.bat」，
echo  它会提示你重启 WorkBuddy（只会重启WorkBuddy，
echo  不会动它的工作区文件）。
echo.
pause
endlocal
