# -*- coding: utf-8 -*-
"""PS 5.1 has no [short] accelerator -> use [int16]."""
p = r'D:\2\14\-Un-Real-0d23d9ux-Engine-main\portable\AI-P\Engine-Input-Injection.ps1'
s = open(p, 'r', encoding='utf-8-sig').read()
n = s.count('[short]$Dx') + s.count('[short]$Dy')
print('short sites:', n)
assert n >= 2
s = s.replace('[short]$Dx', '[int16]$Dx').replace('[short]$Dy', '[int16]$Dy')
# Send-Mouse wrapper too
s = s.replace('function Send-Mouse { param($Writer, $Reader, [short]$Dx, [short]$Dy, [byte]$Buttons) }', '')
s = s.replace('[short]$Dx, [short]$Dy', '[int16]$Dx, [int16]$Dy')
open(p, 'w', encoding='utf-8-sig', newline='').write(s)
s2 = open(p, 'r', encoding='utf-8-sig').read()
assert '[short]' not in s2
print('int16 fixed')
