try {
  & "D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\mv-instrument.ps1" -Action Snapshot -Root (Join-Path $env:TEMP 'mv-test') -Label 'first-burn'
}
catch {
  Write-Output "POS: $($_.InvocationInfo.PositionMessage)"
  Write-Output "SCRIPT: $($_.InvocationInfo.ScriptName) LINE $($_.InvocationInfo.ScriptLineNumber)"
  Write-Output "MSG: $($_.Exception.Message)"
}
