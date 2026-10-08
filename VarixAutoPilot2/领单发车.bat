@echo off
chcp 65001 >nul
title VarixAutoPilot 领单产线塔（18 路 · VTaskBoard 版）
cd /d "%~dp0"

echo ════════════════════════════════════════════════════
echo  领单产线塔：建 18 个全新对话 → 派领单协议 → 静置 5 分钟体检
echo  协议载明：全部功能围绕 Rust 内核进行，功能代码全部为 Rust
echo  TS 存量（约 1400 个文件）全面迁 Rust：任务落在 TS 区先迁移再施工
echo  没正常起来的工人：归档旧对话 → 新建对话重来；之后每 10s 巡检
echo  切号：账号异常自动换下一个直到可用；≥2/3 对话存活则唤醒续用不重建
echo  积分耗尽五信号：页面「积分额度已耗尽」横幅 / 阻塞单积分原因 ≥2 例
echo  / 发送连败 ≥3 / 4min 内 4 个 AI 连续异常（任一命中即切号）
echo  TreeCode 没开会自动拉起；19871051162 永不切入（Variable 指定）
echo  切号成功后自动确保模型 GLM-5.3-Flash（高思考/300K/免单，只切一次）
echo  并把积分阻塞单自动重排回待领，任务继续做下去
echo  24h 守护：塔异常退出自动重启（崩溃/失联 60s 后拉起，不间断发车）
echo  彻底停止：Ctrl+C 塔优雅退出（工人继续自循环）；无账号停机不会自动重启
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

:loop
"%PY%" tools\claim_tower.py --start 18 --watch --interval 10 --settle-min 5 %*
set "EC=%errorlevel%"
if "%EC%"=="0" (
  echo.
  echo 塔已优雅退出（收口完成或人工 Ctrl+C，工人仍在自循环）。按任意键关闭窗口。
  pause >nul
  goto :eof
)
if "%EC%"=="2" (
  echo.
  echo 塔停机（无可用积分账号）——产线终止，需人工补充账号后重新发车。按任意键关闭窗口。
  pause >nul
  goto :eof
)
echo.
echo [24h 守护] 塔异常退出（错误码 %EC%）——60 秒后自动重启…
timeout /t 60 /nobreak >nul
goto loop
