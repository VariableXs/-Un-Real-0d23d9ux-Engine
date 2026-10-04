; src-tauri/nsis/uninstaller-hooks.nsh — V-95 卸载器数据抉择（AI-20 质量门禁与收官组）
;
; 语义红线（化境 V-85/V-95 统一口径）：
;   - 删除一律「回收站式可反悔，绝不覆写」：SHFileOperationW FO_DELETE + FOF_ALLOWUNDO；
;   - 默认保留数据（含静默卸载 /SD IDNO）：卸载程序本体从不触碰用户数据目录；
;   - 移入回收站失败 = 如实报告并保持原样（绝不降级为永久删除）。
;
; 数据目录：$APPDATA\com.variable.app（tauri.conf.json identifier）。

!macro NSIS_HOOK_PREINSTALL
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend

; 卸载前数据抉择：是 → 数据目录整体移入回收站；否（默认/静默） → 完整保留。
!macro NSIS_HOOK_PREUNINSTALL
  MessageBox MB_YESNO|MB_ICONQUESTION "是否同时删除 Variable 的用户数据（文档、设置、备份、媒体库）？$\n$\n是 — 数据目录整体移入回收站（可反悔，绝不覆写）$\n否 — 完整保留数据，重装后可继续使用（推荐）" /SD IDNO IDYES var95_delete IDNO var95_keep

  var95_delete:
    Push "$APPDATA\com.variable.app"
    Call un.Var95Recycle
    Pop $0
    IntCmp $0 0 var95_del_ok 0 var95_del_ok
    IntCmp $0 -1 var95_del_none var95_del_fail var95_del_fail
    var95_del_ok:
      DetailPrint "V-95: 用户数据已移入回收站（可反悔）"
      Goto var95_done
    var95_del_none:
      DetailPrint "V-95: 未发现用户数据目录（无需处理）"
      Goto var95_done
    var95_del_fail:
      DetailPrint "V-95: 移入回收站失败（代码 $0）——数据保持原样保留"
      MessageBox MB_OK|MB_ICONINFORMATION "数据目录未能移入回收站，已保持原样保留：$\n$APPDATA\com.variable.app$\n$\n可手动处理该目录（普通删除即可，资源管理器默认入回收站）。"
      Goto var95_done

  var95_keep:
    DetailPrint "V-95: 保留用户数据（$APPDATA\com.variable.app）"
  var95_done:
!macroend

; 将路径移入回收站。栈协议：入参 = 路径；出参 = 0 成功 / -1 路径不存在 / 其余 = SHFileOperationW 返回码。
; 红线：FOF_ALLOWUNDO（回收站可反悔）+ FOF_NOERRORUI|FOF_SILENT|FOF_NOCONFIRMATION（不打扰、不弹系统确认）。
Function un.Var95Recycle
  Pop $0
  IfFileExists "$0\*.*" var95rb_go 0
  Push -1
  Return

  var95rb_go:
  ; 双 NUL 结尾路径串（SHFileOperationW 的 pFrom 要求）
  System::Call `*(&w1024 "$0", &w2 "")p.r1`
  ; SHFILEOPSTRUCTW（p=原生指针宽，i=4 字节；System 插件按自然对齐布局）：
  ;   hwnd=0 / wFunc=FO_DELETE(3) / pFrom=r1 / pTo=0 /
  ;   fFlags=FOF_ALLOWUNDO|FOF_NOCONFIRMATION|FOF_SILENT|FOF_NOERRORUI (0x454) / 其余 0
  System::Call `*(p 0, i 3, p r1, p 0, i 0x454, i 0, p 0, p 0)p.r2`
  System::Call `shell32::SHFileOperationW(p r2)i.r3`
  System::Free $2
  System::Free $1
  Push $3
FunctionEnd
