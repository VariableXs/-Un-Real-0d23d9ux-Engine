@echo off
chcp 65001 >nul
title VarixAutoPilot 领单产线塔（18 路 · VTaskBoard 版）
cd /d "%~dp0"

echo ════════════════════════════════════════════════════
echo  领单产线塔：建 18 个全新对话 → 派领单协议 → 每 10s 巡检
echo  工人 AI 自动到 VTaskBoard(端口 8767) 领单，READY 自动续跑
echo  收口条件：任务板全部完成；否则一直运行，Ctrl+C 停塔不停工
echo ════════════════════════════════════════════════════
echo.

where python >nul 2>nul
if errorlevel 1 (
  echo [错误] 未找到 python，请先安装并加入 PATH
  pause
  exit /b 1
)

python tools\claim_tower.py --start 18 --watch --interval 10 %*
echo.
echo 塔已退出（工人仍在自循环）。按任意键关闭窗口。
pause >nul
