# AI-U3 v2 深化批次 · 缺陷账本（施工期自检发现即修，全部闭环零遗留）

| # | 级 | 现象 | 位置 | 修法 | 状态 |
| --- | --- | --- | --- | --- | --- |
| 1 | 🟡 | spaceCheck 内残留 `shortfalls.add ? null :` 笔误表达式 | copyops.ts | 改为直 push | 即时修 |
| 2 | 🟡 | filesec 自检含无意义 try/catch 死代码（bad 变量未用） | filesec.ts | 改为常量断言 | 即时修 |
| 3 | 🟡 | F502 换行器把英文单词拆半（"dashboard"→"dash/board"，违反"不在英文单词中间断"判据） | deskicons.ts wrapIconLabel | 重写：ASCII 整词降行、CJK 逐字拆、超长 token 硬拆三分支 | 即时修 |
| 4 | 🟡 | F512 pushShot 满编时把新入架条目自己当淘汰对象（findIndex 扫到自身） | filesec.ts pushShot | 淘汰候选排除新条目（slice 掉末位） | 即时修 |
| 5 | 🟡 | F502 truncated=true 但第二行恰好满宽时不追加省略号（省略号语义丢失） | deskicons.ts 收尾逻辑 | truncated 时强制截尾+省略号 | 即时修 |
| 6 | 🟢 | U3_DEFAULTS 声明在实例化之后（TDZ 风险） | u3store.ts | 移到 class 之前 | 即时修 |
| 7 | 🟢 | Toast kind 用词不符 uiStore 类型（ok/warn/err → success/info/error） | U3Runtime.tsx | 对齐枚举 | 即时修 |
| 8 | 🟢 | 单测 6 处预期错误（F508 黑块数/F522 中档点数/F526 主册原文 245MB/F549 跨年=冬月十三等） | u3.spec.ts | 按主册原文与内核 v1 同源事实修正 | 即时修 |
| 9 | 🟢 | F534 自检样例路径仅 352 字符不足 600 判据线 | copyops.ts 自检 | 100 层嵌套 ≈1900 字符 | 即时修 |
| 10 | 🟢 | UNLOCK_PATH 测试导入遗漏 | u3.spec.ts | 补 import | 即时修 |

并发环境事项：施工期间同工作区有其他 AI 并发操作（staged 文件曾被其他批次写入/清空），
本包采用 pathspec 限定提交（20 文件纯净无夹带）保障领地边界。desktopxp 13 个失败测试
为 AI-D2 untracked 在制品（F093-F110 其领地），与本包零关系。
