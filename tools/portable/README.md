# tools/portable —— PowerShell 静态检查工具

给 `portable/**/*.ps1` 用的静态检查。纯 Python 3 标准库，无需安装依赖，
在仓库根目录执行：

```bash
python3 tools/portable/ps_lex_check.py portable              # 词法（最权威）
python3 tools/portable/ps_struct_check.py portable           # 括号/字符串配平
python3 tools/portable/ps_case_collision_check.py portable   # 大小写同名变量
python3 tools/portable/xref_check.py                         # 函数/动作名交叉引用
```

退出码 0 = 通过，非 0 = 有问题。

## 各工具能查什么

| 工具 | 检查内容 | 强度 |
|---|---|---|
| `ps_lex_check.py` | 真正的词法器：注释、单/双引号串、here-string、反引号转义与续行、`$( )` 子表达式、**Unicode 智能引号定界**，并在 token 流上配平括号 | 最接近真解析器 |
| `ps_struct_check.py` | 括号/引号/here-string 计数配平 | 弱，仅作快速筛查 |
| `ps_case_collision_check.py` | 同一脚本内仅大小写不同的变量名（PowerShell 视为同一个变量） | 针对特定缺陷类 |
| `xref_check.py` | 函数调用、`-Action` 动作名、dot-source 路径是否都有定义 | 交叉引用 |

## 重要：静态检查通不过 ≠ 能被 PowerShell 解析

`ps_struct_check.py` 只是**计数器**。它曾对 CI 上被真 PowerShell 明确拒绝的
`portable/tests/Run-PortableTests.ps1` 给出 **26/26 通过**的误报
（根因见 `docs/AI5-测试交付.md` §4.1：UTF-8 无 BOM + 「应」字末字节 `0x94`
在 CP1252 中是 `U+201D`，而 PowerShell 把智能引号当字符串定界符）。

**语法正确性的最终裁判是真 PowerShell**：
`portable/tests/Run-PortableTests.ps1`，由 CI 的 `frontend` 作业在
`windows-latest` 上通过 `portable/AI5/__tests__/portable.test.ts` 真实执行。

## 回归自检

这两个工具都用真实缺陷验证过，不是摆设：

```bash
# 词法器应能复现编码缺陷：修复前 3 处错误
git show 9ea9f24^:portable/tests/Run-PortableTests.ps1 > /tmp/bad.ps1
python3 - <<'EOF'
import importlib.util
spec = importlib.util.spec_from_file_location('lc', 'tools/portable/ps_lex_check.py')
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
raw = open('/tmp/bad.ps1','rb').read()
errs, _ = m.lex(raw.decode('cp1252', errors='replace'))
print(f'{len(errs)} 处错误（应为 3）')
EOF

# 大小写冲突检查应能抓到修复前的 Accept-Gate.ps1
git show 48d9efb^:portable/AI5/Accept-Gate.ps1 > /tmp/bad2.ps1
python3 tools/portable/ps_case_collision_check.py /tmp   # 应报风险并退出 1
```
