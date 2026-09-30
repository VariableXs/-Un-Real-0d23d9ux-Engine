# UNX-D4-B02 · 窗口树深化与窗口属性（F14421–F14440 · 20 条）

> AI-19 承办｜批主题：SetWindowPos 全语义 + 显示态状态机 + 槽位/属性/子类化 + 风格组合矩阵回归（F 型地基批之二）｜域账累计：B01–B02 12,000 / 240,000｜嫁接源：Wine user32 窗口管理参考实现（只跟随参考）+ MSDN/Windows Internals 版本锚（ADR-UNX-008）+ Windows 真机行为观测（Spy 录制）｜防重：与 B01 分层——B01 立窗口对象与类表地基，本批深化树操作与属性/显示态；Z 序底座在 B01、SWP 全语义在本批，判据颗粒不重｜上游：AI-18（D3 句柄表协议）、AI-26（vxwm 失效区通知预告）｜批注：风格组合矩阵回归账与 B01 创建矩阵为创建时/运行时两段，防重声明见批内条目

### UNX-F14421 · SetWindowPos 全语义与 SWP 位矩阵
- 域/批：D4/B02｜纯功能行数：361｜状态：[骨架]｜判据：UNX-F14421-J1 SWP_NOSIZE/NOMOVE/NOZORDER/NOACTIVATE/FRAMECHANGED 位组合矩阵抽样 120 组与基准机 Spy 记录一致
### UNX-F14422 · 子窗口裁剪（WS_CLIPCHILDREN/WS_CLIPSIBLINGS）
- 域/批：D4/B02｜纯功能行数：301｜状态：[骨架]｜判据：UNX-F14422-J1 两风格对失效区与绘制裁剪的联动行为对照一致，组合矩阵 8 格全对
### UNX-F14423 · SetParent 重挂与重绘调度序
- 域/批：D4/B02｜纯功能行数：321｜状态：[骨架]｜判据：UNX-F14423-J1 重挂后父子账/Z 序/重绘调度序与基准机一致，跨进程 SetParent 拒止对齐
### UNX-F14424 · IsChild/GetAncestor 属主判定链
- 域/批：D4/B02｜纯功能行数：281｜状态：[骨架]｜判据：UNX-F14424-J1 GA_PARENT/ROOT/OWNER 三轴判定 50 组零错，环树注入拒止
### UNX-F14425 · 窗口属性槽（SetProp/GetProp/RemoveProp/EnumProps）
- 域/批：D4/B02｜纯功能行数：301｜状态：[骨架]｜判据：UNX-F14425-J1 属性增删改查枚举 1 万次账零漂移，进程退出属性清扫零残留
### UNX-F14426 · GWL_WNDPROC 子类化与链式子类账
- 域/批：D4/B02｜纯功能行数：341｜状态：[骨架]｜判据：UNX-F14426-J1 多级子类链安装/拆除顺序账零漂移，链序回绕（CallWindowProc）与基准机消息序一致
### UNX-F14427 · SetWindowLongPtr 风格位切换语义
- 域/批：D4/B02｜纯功能行数：321｜状态：[骨架]｜判据：UNX-F14427-J1 GWL_STYLE/EXSTYLE 切换触发 WM_STYLECHANGING/CHANGED 序对照一致，需重绘风格的 FRAMECHANGED 调度有账
### UNX-F14428 · 窗口 ID 与对话框控制 ID 账
- 域/批：D4/B02｜纯功能行数：281｜状态：[骨架]｜判据：UNX-F14428-J1 GWLP_ID 读写与 GetDlgCtrlID/GetDlgItem 路由一致，ID 冲突检测有账
### UNX-F14429 · ShowWindow 状态机（SW_ 指令全集）
- 域/批：D4/B02｜纯功能行数：341｜状态：[骨架]｜判据：UNX-F14429-J1 SW_ 指令全集状态转移表逐格对照一致，重复同态指令幂等有账
### UNX-F14430 · 最小化/最大化/恢复三态与 WINDOWPLACEMENT 账
- 域/批：D4/B02｜纯功能行数：321｜状态：[骨架]｜判据：UNX-F14430-J1 三态往返放置账（正常/最小/最大化位置恢复）与基准机逐值一致 20/20 组
### UNX-F14431 · WS_VISIBLE 可见性传播语义
- 域/批：D4/B02｜纯功能行数：281｜状态：[骨架]｜判据：UNX-F14431-J1 父隐子隐/父显子按位显的传播矩阵对照一致，IsWindowVisible 与 IsWindowEnabled 正交有账
### UNX-F14432 · EnableWindow 与禁用态消息路由
- 域/批：D4/B02｜纯功能行数：281｜状态：[骨架]｜判据：UNX-F14432-J1 禁用窗口鼠标/键盘拒止路径对照一致，禁用切换返回值语义（原禁用态）逐格对齐
### UNX-F14433 · WM_SYSCOMMAND 派发与标题栏按钮语义
- 域/批：D4/B02｜纯功能行数：301｜状态：[骨架]｜判据：UNX-F14433-J1 SC_MINIMIZE/MAXIMIZE/CLOSE/MOVE 等 SC 码派发序与基准机一致，未处理回落 DefWindowProc 链有账
### UNX-F14434 · 分层窗口 WS_EX_LAYERED 底座语义
- 域/批：D4/B02｜纯功能行数：301｜状态：[骨架]｜判据：UNX-F14434-J1 SetLayeredWindowAttributes 色键/alpha 两模式语义档与基准机一致，像素合成实现归 F3 分界声明
### UNX-F14435 · 层叠/平铺与图标排列账
- 域/批：D4/B02｜纯功能行数：261｜状态：[骨架]｜判据：UNX-F14435-J1 Cascade/Tile 布局算法与基准机对照一致（20 窗布局逐值容差内）
### UNX-F14436 · 跨线程窗口操作边界账
- 域/批：D4/B02｜纯功能行数：281｜状态：[骨架]｜判据：UNX-F14436-J1 他线程窗口的 SetWindowText/SendMessage 允许与 DestroyWindow 限制边界矩阵对照一致
### UNX-F14437 · 窗口销毁竞态防护与悬空句柄账
- 域/批：D4/B02｜纯功能行数：301｜状态：[骨架]｜判据：UNX-F14437-J1 销毁中操作注入 10 族全拒不崩，悬空句柄 IsWindow 判定 10/10 准确
### UNX-F14438 · GetWindowInfo/GetWindowPlacement 查询族一致性
- 域/批：D4/B02｜纯功能行数：281｜状态：[骨架]｜判据：UNX-F14438-J1 查询结构逐字段与内部账一致（抽样 50 组），cbSize 校验拒止对齐
### UNX-F14439 · 风格组合矩阵回归账（WS_×WS_EX_ 抽样）
- 域/批：D4/B02｜纯功能行数：301｜状态：[骨架]｜判据：UNX-F14439-J1 风格两两组合抽样 200 组创建-查询-销毁回归零漂移，与 B01 创建矩阵防重（运行时段）
### UNX-F14440 · ktest 引导面 B02 批断言集
- 域/批：D4/B02｜纯功能行数：241｜状态：[骨架]｜判据：UNX-F14440-J1 本批 19 条判据聚合入 ktest 引导面，一次命令全跑通过率 100%，失败注入必红三条
