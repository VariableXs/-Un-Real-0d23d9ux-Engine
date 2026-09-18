try {
  & "D:\2\14\-Un-Real-0d23d9ux-Engine-main\portable\AI-P\Manage-Versions.ps1" -Action Snapshot -Root (Join-Path $env:TEMP 'mv-test') -Label 'first-burn'
}
catch {
  Write-Output "POS: $($_.InvocationInfo.PositionMessage)"
  Write-Output "MSG: $($_.Exception.Message)"
  Write-Output "TGT: $($_.Exception.TargetObject)"
}
