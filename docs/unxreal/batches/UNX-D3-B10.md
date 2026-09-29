# UNX-D3-B10 · Reg* 族（一）：主干 API 全语义（F13781–F13800 · 20 条）

> AI-18 承办｜域账累计：B01–B09 54,000 + 本批 6,000 = 60,000 / 240,000｜嫁接源：Microsoft Win32 Reg* 文档语义（S1 母版：入参样本集/输出/错误码逐码）+ Windows 行为观测｜防重：Reg* 全族约 6 批消化的任务书铺排，本批为前 2 批之一（主干 API），B11 收尾（Notify/配额/安全/卸载）；错误码矩阵条目（F13799）与 F13781 总纲分层（总纲立框架、F13799 全量对账）｜批注（AI-18）：Reg* 是 E4 安装器/N 域软件的直接消费面——错误码逐码对齐是判据（S1）

### UNX-F13781 · Reg* 面架构与错误码矩阵总纲（S1 落地）
- 域/批：D3/B10｜纯功能行数：380｜状态：[骨架]｜判据：UNX-F13781-J1 全族 API 清单/错误码矩阵框架/语义档体例三件齐备，逐码可对账
### UNX-F13782 · RegOpenKeyEx 全语义
- 域/批：D3/B10｜纯功能行数：360｜状态：[骨架]｜判据：UNX-F13782-J1 打开成功/NOT_FOUND/ACCESS_DENIED 三类 ×samDesired 组合矩阵与 Windows 逐码一致
### UNX-F13783 · RegCreateKeyEx（disposition 双值语义）
- 域/批：D3/B10｜纯功能行数：360｜状态：[骨架]｜判据：UNX-F13783-J1 REG_CREATED_NEW_KEY/REG_OPENED_EXISTING_KEY 判定与 Windows 对照一致（同名/大小写变体含）
### UNX-F13784 · RegCloseKey 与句柄联动
- 域/批：D3/B10｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F13784-J1 关闭后句柄失效（复用拒止）与句柄账联动一致，双关行为对齐
### UNX-F13785 · RegFlushKey 语义
- 域/批：D3/B10｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F13785-J1 Flush 触发落盘可观测（脏页清账），语义与 Windows 文档一致
### UNX-F13786 · RegQueryValueEx 类型保真
- 域/批：D3/B10｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F13786-J1 全 REG_ 类型读取类型/数据双保真（与写入值逐字节一致），类型不符拒止有码
### UNX-F13787 · RegQueryValueEx 缓冲语义（ERROR_MORE_DATA）
- 域/批：D3/B10｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F13787-J1 缓冲不足返回 MORE_DATA 且 SIZE 回填可用值（Windows 语义含空终止差异档）
### UNX-F13788 · RegSetValueEx（限制与内嵌阈值）
- 域/批：D3/B10｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F13788-J1 写入类型全集/大小上限/inline 阈值分档与 Windows 读回一致
### UNX-F13789 · samDesired 访问掩码全位语义（KEY_* 组合展开）
- 域/批：D3/B10｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F13789-J1 KEY_READ/KEY_WRITE 等组合位展开表齐备，位级行为矩阵逐项可观测
### UNX-F13790 · RegQueryInfoKey 五元账
- 域/批：D3/B10｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F13790-J1 子键数/值数/类名/最大长度/最后写入时间五元与 reg query 对照一致
### UNX-F13791 · RegEnumKeyEx 索引语义
- 域/批：D3/B10｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F13791-J1 索引枚举（越界 NO_MORE_ITEMS/中途插入行为）与 Windows 对照一致
### UNX-F13792 · RegEnumValue 索引语义
- 域/批：D3/B10｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F13792-J1 值枚举序（含默认值位置）与 Windows 对照一致，越界码一致
### UNX-F13793 · RegQueryMultipleValues
- 域/批：D3/B10｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F13793-J1 多值查询原子性（全成或全败）与 Windows 对照一致
### UNX-F13794 · RegLoadMUIString
- 域/批：D3/B10｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F13794-J1 MUI 字符串解析（引用回退直读）与 Windows 对照一致，缺失回退码一致
### UNX-F13795 · RegCopyTree
- 域/批：D3/B10｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F13795-J1 全树复制（含安全描述引用策略）后 diff 零差，目标非空拒止有码
### UNX-F13796 · RegDeleteTree/RegDeleteKeyEx/RegDeleteKeyValue
- 域/批：D3/B10｜纯功能行数：320｜状态：[骨架]｜判据：UNX-F13796-J1 三删除入口语义矩阵（范围/子键视图/WOW64 键）与 Windows 逐码一致
### UNX-F13797 · RegSaveKey/RegLoadKey 挂载语义
- 域/批：D3/B10｜纯功能行数：280｜状态：[骨架]｜判据：UNX-F13797-J1 Load 挂载点与卸载对账一致，Save/Load 往返 diff 零差
### UNX-F13798 · RegOverridePredefKey 与预定义句柄映射
- 域/批：D3/B10｜纯功能行数：260｜状态：[骨架]｜判据：UNX-F13798-J1 七预定义句柄映射与 Override 重定向行为和 Windows 对照一致
### UNX-F13799 · Reg* 错误码矩阵全量对账（ERROR_ 全集）
- 域/批：D3/B10｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F13799-J1 全族 × 全错误码矩阵逐码对账（S1 档），不一致码清单为零
### UNX-F13800 · ktest 引导面 B10 批断言集
- 域/批：D3/B10｜纯功能行数：340｜状态：[骨架]｜判据：UNX-F13800-J1 本批 19 条判据聚合入 ktest 引导面，一次命令全跑通过率 100% 才算绿
