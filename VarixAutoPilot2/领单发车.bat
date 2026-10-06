@echo off
chcp 65001 >nul
title VarixAutoPilot 领单产线塔（18 路 · VTaskBoard 版）
cd /d "%~dp0"

echo ════════════════════════════════════════════════════
echo  领单产线塔：建 18 个全新对话 → 派领单协议 → 静置 5 分钟体检
echo  没正常起来的工人：归档旧对话 → 新建对话重来；之后每 10s 巡检
echo  工人 AI 自动到 VTaskBoard(端口 8767) 领单，READY 自动续跑
echo  收口条件：任务板全部完成；否则一直运行，Ctrl+C 停塔不停工
echo ════════════════════════════════════════════════════
echo.

set "PY=C:\Users\varia\.workbuddy\binaries\python\versions\3.13.12\python.exe"
if not exist "%PY%" (
  echo [错误] python.exe not found:
  echo        %PY%
  pause
  exit /b 1
)

"%PY%" tools\claim_tower.py --start 18 --watch --interval 10 --settle-min 5 %*
echo.
echo 塔已退出（工人仍在自循环）。按任意键关闭窗口。
pause >nul
