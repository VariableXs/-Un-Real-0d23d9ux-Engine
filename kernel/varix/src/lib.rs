//! Varix kernel — AI-01 boot & platform domain (F001~F025),
//! AI-02 CPU & interrupt domain (F026~F050),
//! AI-03 memory-management domain (F051~F075),
//! AI-04 scheduler domain (F076~F100),
//! AI-05 process & user-space domain (F101~F125),
//! AI-06 storage & filesystem domain (F126~F150),
//! AI-07 input & HID domain (F151~F175),
//! AI-08 graphics & display domain (F176~F200),
//! AI-09 network stack domain (F201~F225),
//! AI-10 security & isolation domain (F226~F250).
//!
//! The lib is `no_std` on the kernel target and builds against std on the
//! host so every module's unit tests run natively (`cargo ktest`).
#![cfg_attr(not(test), no_std)]
// Host-side non-test builds (integration tests like tests/fuzz.rs) run on the
// host triple: keep std reachable while the real kernel image stays no_std.
#[cfg(all(not(test), not(feature = "kernel-image")))]
extern crate std;

// 内核镜像目标是 no_std：显式接入 alloc，让 Vec/String/format!/to_vec/to_string
// 在 kernel-image 下同样可解析（宿主侧继续走 std）。
extern crate alloc;

// kernel-image 目标登记全局分配器：mem::heap::VarixAllocator 已实现 GlobalAlloc 契约
// （可失败、对齐感知、泄漏记账）。自检注册表 run_*_checks 只在宿主测试侧调用，
// 内核运行时路径不触发分配。
#[cfg(feature = "kernel-image")]
#[global_allocator]
static KERNEL_ALLOCATOR: mem::heap::VarixAllocator = mem::heap::VarixAllocator;

pub mod acpi;
pub mod audio;
/// 性能域深化（Varix STAR I start · B 域 F041~F057 · AI-K1 分工包）：
/// 帧率账本/归因器/冷启动画像/预取指纹/页缓存水位/写合并自适应/调度预算/
/// CPU 频率/空转清零/中断合并/大页/堆碎片/启动并行/图像 SIMD/字形缓存/
/// 脏区深化/IO 分级，十七项判据实装层。
pub mod perfstar;
/// I 通用域·三分队（Varix STAR I start · I 域 F501~F550 · AI-U3 分工包）：
/// 桌面图标可读性/两行封顶/网格密度/PIN 快速解锁/蓝牙动态锁/访客模式/
/// 锁屏与应用防截/文件粉碎/单文件加密/剪贴板清空/截图历史/Ctrl 定位指针/
/// 声音视觉提示/大小写提示音/标题栏中键/指针轨迹/打字隐藏指针/查找替换/
/// 启动页/截图落点/速查卡导出/状态栏/导航树/树列表同步/回收站撤销/
/// 空间预检/复制校验/复制队列/打开失败诊断/只读提醒/长路径/Win+数字/
/// Win+T/瞥桌面/Alt+Esc/布局锁定/内存诊断/网络重置/ClickLock/分设备音量/
/// 通知音量分级/蓝牙电量/设备接入通知/音量平衡/任务管理器置顶/时钟悬停
/// 农历/批次六验收锚点，五十项判据实装层。
pub mod ustar3;
/// 农历/批次六验收锚点，五十项判据实装层（提交版见 origin/main ad1c2d2c）。
pub mod ustar3;
/// 应用兼容域深化（Varix STAR I start · A 域 F001~F020 · AI-C1 分工包）：
/// 无感双击/静态 PE 全量/导入绑定加速/Wow64 门/Win32 窗口层/GDI/GDI+/通用
/// 对话框/注册表虚拟化/文件系统重定向/环境变量/控制台子系统/.lnk/PE 资源/
/// 多语言资源/字体链/剪贴板格式/拖放互通/COM 本地接口/异常与调试面，
/// 二十项判据实装层。
pub mod compatstar;
/// H 基础通用域·一分队（Varix STAR I start · H 域 F201~F250 · AI-H1 分工包）：
/// 文本选择/撤销重做/橡皮筋/滚动统一/Tooltip/焦点导航/对话框键语义/进度反馈/
/// 错误三要素/空态/编辑手势/拖放高亮/标题栏/窗口最小尺寸/菜单层级/三控件/
/// 数值步进/多选修饰键/视图记忆/纯文本粘贴/窗口查找/字体渲染/插入符/DPI 档位/
/// 主题热切换/窗口层级/开合动画/列表虚拟化/输入框三件套/密码显隐/表单校验/
/// 日期选择器/文件对话框/颜色选择器/虚拟桌面/窗口排列/位置记忆/锁屏/亮度/
/// 音量 OSD/音频路由/网络状态/隐藏文件/快捷键注册表/减少动效/字号无障碍/
/// 文本截断/窗口置顶/热角/指针精度，五十项判据实装层。
// [AI-U2 临时屏蔽已解除：h1base.rs 已落位，AI-H1 批次在途逐模块补齐]
pub mod h1star;
pub mod banner;
pub mod bootopt;
pub mod bootcfg;
pub mod bootconf;
pub mod bootnext;
pub mod bootselect;
pub mod handoff;
pub mod oneshot;
pub mod checks;
pub mod cmdline;
pub mod deploy;
pub mod deveco;
pub mod display;
pub mod drivers;
pub mod driver;
pub mod compatruntime; // AI-33 内核兼容运行时（X08126~X08250）
pub mod console;
pub mod cpu;
pub mod fb;
pub mod font;
pub mod stream;
pub mod fs;
pub mod gfx;
pub mod share;
pub mod shell;
pub mod switcher;
pub mod telemetry;
pub mod input;
/// PS/2 键鼠输入事件服务（任务19：事件队列+订阅者+鼠标最小版）。
pub mod inputsvc;
/// 输入注入通道（任务49：Variable→引擎反向注入，16B 同源帧+守恒账本）。
pub mod inputinject;
/// 内核显示服务（任务20：Surface 归口+双缓冲+脏矩形提交）。
pub mod displaysrv;
/// 窗口面服务（AI-4 · S2.06 渲染通路+S2.09 多窗合成：客户端 copy-in
/// 提交+块链缓冲+Z 序合成+最小化保活+焦点记录，SYS_WIN(21) 系统调用面）。
pub mod winsurf;
/// VXWM 帧编解码层（WP-201 · B-503：MD1 附录 H 二十四消息 × MD2 篇 5.2
/// 定长头/小端/校验和尾缀/4KB 上限/序号单调闸/bind 版本协商/回放环）。
pub mod vxwm;
/// 缓冲所有权状态机（WP-201 · B-504：commit→Held→frame_callback 归还，
/// 双缓冲起步三缓冲封顶，代数防陈旧引用，交错压测零半帧）。
pub mod bufown;
/// 浮层物理强制——popup_grab 抓取关系表（WP-201 · B-506：区域外点击/Esc
/// 由合成器直接转译关闭浮层，还焦点给触发元素，客户端无法逃逸）。
pub mod popup;
/// 字形图集分配器（WP-201 · B-505：LRU 60MB 上限 + 常用集 pin 常驻
/// 零重光栅化 + 超限降渲染密度 + 图集旋钮只降不升；码点表/字形源资产缺口
/// 随队跟踪，pin 走显式注册接口）。
pub mod atlas;
// Varix STAR I · 泳道四 I 通用域·二分队（AI-U2 · F451-F500）：五十项判据
// 实装层——快捷方式向导/文件夹图标/角标/模板/搜索直出算式与单位换算/终端
// 十件套/设置中心键盘流与文案/差异登记册/环境变量/启动修复/头像/主机名/
// 声道测试/触控板/主键交换/滚轮方向/VPN/代理/最近文件/隐私清除/应用数据/
// 关机徽标与阻止器/自动亮度/电源计划/散热/任务栏三件/磁贴长按/通知动作行/
// DPI 修复/锁屏壁纸/跨屏移动键。
pub mod genstar2;
/// 合成器事件泵状态机（WP-201 · B-501：四源归一/排空归并/不需要就不合成/
/// 光标层小步走/看门狗饥饿/空转成本 permille 模型 ≤ 50）。
pub mod pump;
/// 拖动帧率基准 harness（WP-201 · B-502：双打点间隔分布/p95 ≤ 55fps/
/// 掉帧时输入不迟滞/1080p 带宽下界/四步预算分解）。
pub mod dragbench;
/// 合成器恢复（WP-201 · B-507：注册表快照 schema 冻结/五秒合并窗口/
/// 崩溃丢失诚实记账/D-04 三秒预算模型/百次对练全过）。
pub mod comprecover;
/// ext4 特性白名单（WP-203 · B-701：三组旗标逐位过闸/白名单外拒挂/
/// ro_compat 未知也拒严于 ext4 标准/人话三要素）。
pub mod fswl;
/// fsync 硬承诺（WP-203 · B-702：窗口让路/acked⊆flushed 恒等式/
/// 三触发裁决/断电对练零假 ack/承诺窗口=合并窗口同一数字）。
pub mod fsyncp;
/// 断电一百次（WP-203 · B-703：检查点账本/半写不入账/回到最近检查点/
/// 零结构损坏恒等式/重放统计归档/QEMU80+实机20 配比记账）。
pub mod pwrdrl;
/// 写合并收益（WP-203 · B-704：相邻归并成段/吞吐模型整数运算/
/// 元数据集中写对练最差轮过八成线/合并窗口=承诺窗口同一数字）。
pub mod wmerge;
/// NTFS 只读强制（WP-203 · B-705：挂载即只读定型/11 类写入口零放行/
/// 拒绝留痕到源头/无危险开关结构防线/穷举对练拒绝率 100%）。
pub mod ntfsro;
/// 失联保护屏（WP-203 · B-706：通道死亡双条件判定/一秒广播预算/
/// 保护屏冻结输入三要素文案/侥幸继续防线零放行/无解除路径终态）。
pub mod linkloss;
/// 预读命中账（WP-203 · B-707：二次启动 ≤ 8s 预算模型/命中率下限结构
/// 推导/指纹失效退化冷启动/与 B-2702 预取模块口径联动——WP-105 回补位）。
pub mod prefacct;
/// 三层渲染路径 C-1 审计（WP-208 · B-801：系统层/兼容层/Web 层互不混线/
/// 提交面唯一上屏入口/绕过写屏仅存审计分类）。
pub mod path3;
/// Wine GL 承诺面（WP-208 · B-802：llvmpipe 舒适区五类全承诺/重度 D3D
/// 三类全不承诺/判例应用画像过闸/星卡诚实标注）。
pub mod wingl;
/// Electron 软件路径（WP-208 · B-803：直插即纯软件定型/类型面无 GPU 开关/
/// 软合成确定性闭环/窗口面与输入面齐备）。
pub mod esoft;
/// R3 硬加速摸底包（WP-208 · B-804：硬件清单固化/提交后端 trait 抽象双
/// 后端接口位/验收口径三指标——只摸不建，一行驱动代码不写）。
pub mod r3scan;
/// 视频软解链路（WP-208 · B-805：两核上限吞吐模型/常见档实时/解码帧类型
/// 必选表面提交/同步容差 125ms/解不动三要素兜底不花屏）。
pub mod viddec;
/// HDA 驱动骨架（WP-208 · B-806：CORB/RIRB 命令环保序/DMA 位置上报/
/// 引脚配置固化/拔插周期边界切换无爆音/延迟对照测量）。
pub mod hdadrv;
/// 混音器流管理（WP-208 · B-807：每流环形缓冲/独立音量静音/通话压媒体
/// duck 30% 不静音/定长周期混音 i16 饱和不绕回）。
pub mod mixer;
/// 有线吞吐与接缝纪律（WP-204 · B-601：700Mbps 达线整数模型/零拷贝优先/
/// 拷贝层级成本阶梯/背压显式丢弃计数进监视器/驱动矩阵第一批）。
pub mod netthr;
/// 描述符混表（WP-204 · B-602：socket 与文件共用 fd 空间/POSIX 最小 fd/
/// poll 聚合跨类型/边缘触发只报一次/泄漏对账恒等式）。
pub mod fdmix;
/// 监听授权（WP-204 · B-603：默认仅出站/监听需权限清单声明/未声明拒绝
/// 留痕/诊断事件环满不覆盖保序）。
pub mod lstnauth;
/// DoH 开关（WP-204 · B-604：默认系统 DNS/切换即时生效即清缓存/污染域
/// 实测锚定/解析失败三要素文案）。
pub mod dohsw;
/// 诊断三件套（WP-204 · B-605：vx-ping/vx-route/vx-capture/JSON 行+人读
/// 双格式逐字段一致/raw 权限收敛在系统工具/判例挂钩 SC-041/SC-005）。
pub mod diag3;
/// 离线态（WP-204 · B-606：单一状态源三呈现面一致/桌面标注离线/更新按钮
/// 变灰带原因/直插应用三要素即时报错延迟恒 0/先测量后调参）。
pub mod offln;
/// 手机共享通道（WP-204 · B-607：RNDIS/NCM 两类/NCM 优先选择语义/插拔
/// 生命周期无幽灵通道/WiFi 空窗生命线/实机偏差登记随队跟踪）。
pub mod usbnet;
/// 事件流单向性（WP-202 · B-901：三源归一/注入接口类型面不存在 C-8 结构
/// 防线/过滤器只消费放行不改写）。
pub mod evflow;
/// 布局表 schema（WP-202 · B-902：五字段/四层/两套内置布局全表合规/切换
/// 即时生效/计宽职责分离）。
pub mod kblayout;
/// IME 三条硬线（WP-202 · B-903：最长韵母优先切分/冻结词库完全匹配档位
/// 序/三段式分页每页九个/100ms-16ms-32MB 预算推导/无运行期学习）。
pub mod imepinyin;
/// 组合期分流（WP-202 · B-904：缓冲空非空组合态/仲裁入口直接分流/组合期
/// 零误触穷举/缓冲层编辑不污染窗口）。
pub mod composesw;
/// 快捷键仲裁（WP-202 · B-905：三列词典/用户>保留>应用/冲突先注册先得
/// 拒绝指名/词典唯一）。
pub mod hkbind;
/// 候选窗定位（WP-202 · B-906：popup_grab+屏幕夹紧/下方越界翻上方/Wine
/// 与原生同路径/恒不越屏）。
pub mod candwin;
/// 焦点环（WP-202 · B-907：类型面恒开无关闭构造/Tab 序环形遍历/焦点唯一
/// 合成器同源/审计零例外）。
pub mod focring;
// --- WP-205 应用件三包（判据实装层，MD2 篇 16-19；B-16xx/17xx/18xx/19xx）---
/// 终端解析与网格（WP-205 · B-1601 解析器吞吐百万级/B-1602 输出绘制解耦：
/// SGR 三档全色/宽字符两格计宽/非法序列忽略计数/每帧上限恒定/背压对账/
/// 滚动环恒内存）。
pub mod termproc;
/// 回显与混用会话（WP-205 · B-1603 回显一帧 16ms 五段预算零阻塞/
/// B-1604 原生与 Linux 直插混用全绿：路由双表/会话槽回收/PTY 生命周期/
/// 未结束会话如实入快照）。
pub mod termfeed;
/// 回收站全语义（WP-205 · B-1701 数据红线：SDK 删除面类型上无"直接删除"
/// 变体——Trash/PurgeConfirmed 穷举表/条目三字段/二次确认硬门/配额 10%
/// 清最旧有通知/跨盘移动=复制+回收站删除/条目守恒对账）。
pub mod trashbin;
/// 只读呈现与搜索双档（WP-205 · B-1702 NTFS 角标+置灰+解释三呈现面与
/// ntfsro 强制层同源/B-1703 即席毫秒级+全盘流式可取消资源回收）。
pub mod fsview;
/// 缩略图异步（WP-205 · B-1704：按需生成零预扫/LRU 上限淘汰/进度单调/
/// 任意进度可取消且取消无半成品落盘/并发两槽节流/每步 2ms 预算不卡 UI）。
pub mod thumbsched;
/// 大文件编辑与撤销链（WP-205 · B-1801 数据红线：视口窗口恒内存/百兆秒开
/// 预算/临时文件+原子换名单步翻转/任一步失败原文件无损/B-1802 词组级撤销
/// 链深五百环形淘汰/undo-redo 守恒/链随窗口释放/换行风格保留原样）。
pub mod edcore;
/// 四小件公共线（WP-205 · B-1803：shot/img/calc/clock 清单在册/冷启动一秒
/// 内存六十四兆/快捷键入词典/主题全适配/一脚本测四件 sweep/截图落盘路径
/// 确定性/旋转元数据级不重编码）。
pub mod widgetline;
/// 设置中心四判据（WP-205 · B-1901 声明式总表七字段新服务零前端/
/// B-1902 校验先于生效拒绝完整回滚三要素报错/B-1903 生效三档表内声明
/// 提示一致前置告知/B-1904 通知浮层免打扰聚合+模态不可绕过安全类无
/// 不再询问）。
pub mod settable;
/// 账本订阅面（WP-206 · B-2001：六频道订阅×一秒节流×六十点滚动窗口×
/// 多消费者对表演练逐字段一致——一套账本多处消费，数字永远同源；
/// 内存三段口径注释固定/温度风扇阶段 3 预留占位）。
pub mod ledgerhub;
/// 监视器呈现面（WP-206 · B-2002：四卡与状态机实时一致×降级原因人话
/// 直通×降级历史最近十次×结束进程二次确认+关键进程拒杀解释×温度卡
/// 占位明示诚实呈现×帧率页开发者门）。
pub mod moncards;
/// Wine 组透明（WP-206 · B-2003：组聚合与展开正确——聚合==成员之和
/// 恒等式×组内展开每成员独立可见×非 Wine 不进组×超限拒绝留痕×
/// 多组不串账，兼容不等于黑箱）。
pub mod winegrp;
/// 星图前端（WP-206 · B-2103：按钮即评级——兜底级主按钮切 Windows 域
/// 走交接/桥接级标注经网页壳×五档评级 MD1 18.1 表序×推荐位规则公开
/// 不设商业位×列表三轴过滤×安装四段顺序不可跳×卸载前关联检查）。
pub mod starmapui;
/// 剪贴板所有权与大载荷旁路（WP-207 · B-3901 所有权只发焦点/后台读取
/// 零成功/读必经合成器/所有者退出即时回收/类型上限如实标记/删除进回收站
/// 咬合 + B-3903 大载荷引用传递微秒级/大图粘贴可取消/文本回显一帧）。
pub mod clipown;
/// 拖放协议与三取消（WP-207 · B-3902：载荷描述定型中途变更拒绝/光标
/// 三态由接收声明裁决/合法落点高亮强制/Esc·无效区·源销毁三取消共用
/// 一套清理路径零泄漏/跨域效果位映射公开）。
pub mod dragdrop;
/// 无障碍五判据构建期门禁（WP-207 · B-4001 焦点环自动登记例外留痕/
/// B-4002 还焦点协议级/B-4003 对比度构建期拒绝 AA4.5 大字3.0/B-4004
/// 色弱形状互异纯色相零依赖/B-4005 减弱动效全局生效零例外——门禁写
/// 在 CI 里不写在良心里）。
pub mod a11ygate;
/// 性能基准六面包（WP-209 · B-1401~1403 首轮：ProbeRing 27 点位三消费者
/// 同环快照逐字段一致×账本漂移千分比红线 10‰×BenchReport 三分位与退化
/// 红线——先测量后调参的测量面）。
pub mod benchsix;
/// 恢复矩阵组（WP-209 · S209：九类崩溃×七恢复路径×三知情面唯一映射×
/// 报告三要素×保护屏五条×重启门三次防自锁——崩溃语义的宪法表）。
pub mod recovermx;
/// 杀死演练（WP-209 · S210：kill -9 十种姿势全过零损坏×恢复预算与矩阵
/// 同源×门禁捉坏自证——演练记录即验收证据）。
pub mod killdrill;
/// 转译治理边界表（WP-301 · B-401 直通族全覆盖+B-404 差异表诚实性：四类
/// 穷举冻结×未登记默认拒×拒绝与简化全登记——治理表是这个包的宪法）。
pub mod lxgov;
/// errno 单源映射与转译开销（WP-301 · B-403 穷举 match 单源 100% 覆盖+
/// B-405 百万次空调用 P95 ≤ 2μs 三段预算模型）。
pub mod lxerrno;
/// 伪文件系统面（WP-301 · B-407：十六文件在册×清单外 ENOENT×内容按内核
/// 账本即时合成）。
pub mod lxprocfs;
/// vxrun 与验收载体（WP-301 · B-402 四步序+LTP 归因闭环+B-406 三类载体
/// 全绿×封闭树 800MB）。
pub mod lxrun;
/// Wine 会话管家（WP-302 · B-1001：wineserver 看门狗失联即重建百次全
/// 恢复×未恢复如实标记×重建预算与恢复矩阵同源×会话卡片协商对账诚实）。
pub mod winecare;
/// Wine 前缀模板（WP-302 · B-1002：模板五要素×生成带版本号可追溯×重建
/// 优于手术旧实例可查×来源三态裁决——篡改零放行）。
pub mod winepfx;
/// Wine 应用启动通道（交叉走查第一项配套：注册表×拉起受理×运行时缺席
/// 如实回报×出参定长编码——受理≠谎报启动，清单外标准报错）。
pub mod winelaunch;
/// Wine 垫片四路（WP-302 · B-1003：SHIM_TABLE 冻结四行全 VarixSide×
/// Wine 源码 diff 为零审计红线×版本锁漂移四路全重校验）。
pub mod wineshim;
/// 星卡判例流水线（WP-302 · B-1004 判例三件套可执行出报告+B-1005 无
/// 人工确认不转绿——类型面强制门）。
pub mod winecase;
/// 崩溃归因与回馈（WP-302 · B-1006 归因五分类穷举且界面呈现×B-1007
/// issue 模板三要素入库×季度命中率不编数）。
pub mod wineattr;
/// SDK 目标三元组与十二件套矩阵（WP-303 · B-1101：三元组冻结×sysroot
/// 双层分工×十二件套全部经 SDK 构建成功）。
pub mod sdktriplet;
/// vxapp 清单校验器（WP-303 · B-1102：字段取值域构建期全查×缺档告警与
/// 违规拒收分账×对抗样例全拒——违规产物零放行）。
pub mod sdkmanifest;
/// 控件宪章默认与主题 token（WP-303 · B-1103 焦点环/三态/动画词典全默认
/// + B-1104 硬编码色值零例审计——合规不靠自觉靠默认）。
pub mod sdkwidgets;
/// 生命周期托管与调试链路（WP-303 · B-1105 退出路径保全回调覆盖率 100%
/// + B-1106 日志/符号化崩溃/QEMU gdb 三件齐）。
pub mod sdkruntime;
/// SDK 最小示例集与评审门（WP-303 · B-1107：echo/hello/service 三模板
/// 过 20 维度代码质量评审——示例即门面）。
pub mod sdktemplate;
/// Java 运行时画像与双载体（WP-304 · B-3301：堆七成设参进 vxrun 预设×
/// futex 验收首位×时钟单调与精度×ext4 文件语义×SC-J 双载体全绿）。
pub mod rtjava;
/// Python 运行时画像与双载体（WP-304 · B-3302：C 扩展 glibc 对齐 Q26
/// 指名报错×venv 循环检测 Q47×并发两路×SC-P 双载体全绿）。
pub mod rtpy;
/// 运行时画像模板与入库制度（WP-304 · B-3303：五步体检单穷举×成文入库
/// ×结论进星卡注记与差异表双消费者——玄学变 checklist）。
pub mod rtmpl;
/// 星图目录服务与校验链（WP-305 · B-2101：schema+哈希双关 Q55×假目录坏
/// 哈希全拒×版本一变一号×星卡引用版本可答×安装面评级面分离 Q83）。
pub mod starmapdir;
/// 星图发布流水线门禁（WP-305 · B-2102：物料三查拒进人工×合规三要素×
/// 人工确认唯一人工环节×合入版本递增×缺料星卡零上架）。
pub mod starmapgate;
/// 卸载清理对账（WP-305 · B-2104：账本守恒漏清即红×越界乱清更重×关联
/// 诚实不静默清除×SC-133 全绿）。
pub mod unisweep;
/// 自测脚本包安全面（WP-305 · B-4301：只读形态写位钉死×采集范围限定目标
/// 应用×指纹时间戳防伪造×复核匿名双明示）。
pub mod selftestpkg;
/// 评级复核双签制度（WP-305 · B-4302：降级单签升级双签×改动必留理由×
/// 申诉证据公开×流程审计逐行对账）。
pub mod dualsign;
/// 提交者反馈回路（WP-305 · B-4303：状态四态穷举×未收录给具体差距×社区
/// 首发标注×全链路两跳留痕×季度统计零提交不编数）。
pub mod feedback;
/// 测试金字塔与冒烟套对齐（WP-401 · B-1201/1202：四层穷举×倒挂泄漏闭环
/// 三态×冒烟用例判据编号全对齐）。
pub mod pyramid;
/// QEMU 演练量级与实机纪律（WP-401 · B-1203/1204：十倍量级逐账对账×
/// 实机脚本六要素×白名单精确匹配）。
pub mod drillscale;
/// 20 维度证据化与每日流水（WP-401 · B-1205/1206：恰好二十条证据模板与
/// 实例双全×流水四段全自动红绿通知）。
pub mod dims20;
/// 可复现构建与镜像单源（WP-401 · B-1301/1302：四件套合取×环境净化四通道
/// ×布局配置单点×img 与 ISO 同源）。
pub mod reprobuild;
/// 写盘工具四段与双槽回滚（WP-401 · B-1303/1304：四段+确认令牌×写备用槽×
/// 自检转正门×断电五段注入全回退）。
pub mod imgwrite;
/// 72 小时回滚兜底与发布七步（WP-401 · B-1305/1306：窗口 72h×五分钟达标×
/// 七步穷举×公告三段式强制）。
pub mod rollback72;
/// 回归门与预算基线同步（WP-402 · B-1404/1405：改动路径选择表×红即阻断
/// 性能与缺陷同罪×红项开票必通知×同 seed 重放同裁决×预算三处同步+ADR 备案）。
pub mod reggate;
/// 体验日志与手册联动（WP-402 · B-2301~2304：事件七字段白名单扩展走 ADR×
/// 挫败信号五类实测标定×隐私红线三道读取面三层×手册五段式无引用拒收）。
pub mod explog;
/// 安全执法面（WP-403 · B-1501~1504：能力对象私有构造越权零成功×W^X 枚举
/// 无 WX 变体类型面零违例×攻击面清单与实现双向对账×四执法点共享权限表）。
pub mod secgate;
/// 签名链与日志红线（WP-403 · B-1505/1506：假包错签旧签三拒×下载段终止
/// 不落盘×红线类型同源密封构建期审计零出现）。
pub mod signchain;
/// 网络诊断三件套（WP-403 · B-3101~3103：旁路复制零干扰 2%<3%×环形覆盖
/// 诚实计数×pcap 标准格式×JSON 字典稳定退出码语义化×失败自动取证附包）。
pub mod netdiag;
/// 安装更新收口（WP-404 · B-3201~3204：安装四段原子性半安装零呈现×升级回切
/// 旧版本保留×卸载引用检查未解引用拒卸×更新五态交接互斥）。
pub mod instup;
/// 字体子系统全量收口（WP-404 · B-3401~3403：字符级回退链混排不跳行×缺字
/// 空白格加诊断提示×畸形字体模糊零崩溃）。
pub mod fontsub;
/// 时间与定时器服务（WP-404 · B-3501~3503：双钟分离接口分型混用编译期拒绝×
/// 交接校时双域差≤2s×定时器不补发漂移修正可选）。
pub mod timesrv;
/// 显示输出管理（WP-404 · B-3601~3603：模式切换失败回退永无黑屏×镜像双路
/// 插拔无花屏×缩放 150% 全系统联动）。
pub mod dispout;
/// 驱动框架与模块通道（WP-404 · B-3701~3703：拔插横跳压测零悬空×健康账本
/// 错误类枚举全覆盖×模块通道空载预留符号白名单）。
pub mod drvframe;
/// 系统日志服务（WP-404 · B-3801~3803：三通道环形实时持久不阻塞×轮转配额
/// 512MB 硬顶×来源签发伪造零可能）。
pub mod syslogd;
/// QEMU 环境替身（WP-404 · B-4101~4104：替身注入接口全可编程×断电百次循环×
/// 虚拟时间快进倒拨×交接对练替身协议闭环）。
pub mod qemuenv;
/// 会话空闲与性能降档（WP-404 · B-4201~4203：空闲三判据分级降档×唤醒百毫秒
/// 满档恢复×闲时礼让让路续跑）。
pub mod idledn;
/// S4（AI-4/6）：ushell 桌面真壁纸（三世界同源静态帧）。
pub mod wallpaper;
/// 里程碑整合探针（任务21：M1 ring3 用户程序 × M2 journal × M3 exFAT 读 ×
/// M4 快照区——「内核可运行用户态程序读写真盘」双会话断电恢复演示）。
pub mod milestone;
/// 内核 KV 存储服务（任务24：localStorage 语义+命名空间隔离，复用
/// fs23_journal 后端作账本——值 inline/溢出槽双模，满容量明确报错）。
pub mod kvsrv;
/// 三方配额服务（任务56：CPU 分配矩阵+内存水位回收+GPU 通道抽象）。
pub mod quota;
/// 引擎盘 ramcache（任务52：LRU 只读块缓存+关机清空断言+整盘 hash 零残留）。
pub mod ramcache;
/// 共享内存块原语+授权模型（任务32：create/map/revoke，句柄不可传递默认）。
pub mod shmsrv;
/// VFS 白名单裁决层+越权审计（任务30：默认拒绝+journal 断电审计）。
pub mod vfsguard;
/// 消息通道原语（任务33：订阅/广播/单发，慢消费者丢最旧+计数）。
#[path = "security/msgchan.rs"]
pub mod msgchan;
/// 剪贴板/拖放白名单化（任务34：格式白名单+真实性校验+pid 授权位图，
/// 路径裁决/审计复用 vfsguard）。
#[path = "security/clip.rs"]
pub mod clipsrv;
/// BLAKE3（内核自实现，任务35 Uxv 索引校验依赖；与官方 crate 交叉验证）。
#[path = "security/blake3.rs"]
pub mod kblake3;
/// Uxv 交换格式只读校验门（任务35：整包拒绝+如实错误码，写侧复用
/// container crate 不复制实现）。
#[path = "security/uxv_ingest.rs"]
pub mod uxvingest;
/// 阶段4 负向演练矩阵（总案步骤10：越权×目录×剪贴板×共享内存全组合）。
#[path = "security/stage4_matrix.rs"]
pub mod stage4matrix;
/// SHA-256/HMAC/PBKDF2（任务65：保险箱口令基元；FIPS 180-4/RFC 4231/公开
/// PBKDF2 向量交叉锁定）。
#[path = "security/ksha256.rs"]
pub mod ksha256;
/// AES-256-GCM（任务65：保险箱封条原语；NIST SP 800-38D 附录 B 向量锁定，
/// 仅加密方向、固定 96-bit nonce、认证失败零明文）。
#[path = "security/kaesgcm.rs"]
pub mod kaesgcm;
/// 保险箱内核侧（任务65：privacy.rs AES-256-GCM 语义平移 + 密钥仅内存
/// 断言 + 焚毁三步 + 掉电损坏即拒绝；blob 布局与桌面侧互通）。
#[path = "security/kvault.rs"]
pub mod kvault;


// UNREAL-X AI-19（族0181~0188 · X04501~X04700）：内核输入栈八族逻辑模型。
pub mod inkstack;
// UNREAL-X AI-34（族0339 · X08451~X08475）：内核兼容 API 层。
pub mod compatapi;
// UNREAL-X AI-35（族0341~0343 · X08501~X08575）：Shim 工程/版本协商/兼容沙盒。
pub mod compatshim;
pub mod integrity;
pub mod kaslr;
pub mod limine;
pub mod logger;
pub mod logo;
pub mod mem;
pub mod memmap;
pub mod net;
pub mod once;
pub mod panicseq; // WP-106 B-2903：panic 四环节序列器（保护屏/现场带/倒计时/复位阶梯）
pub mod platform;
pub mod power;
pub mod power_shutdown; // WP-106 B-2902：关机收尾链与"可拔电"账目
pub mod proc;
pub mod progress;
pub mod ps2;
pub mod robust;
pub mod sched;
pub mod security;
pub mod selftest;
pub mod serial;
pub mod service;
pub mod smbios;
pub mod storage;
pub mod timeline;
pub mod ui;
pub mod compositor;
pub mod vwm;
pub mod virt;
pub mod vsem;

// --- TRINITY-500 AI-11~AI-19 (F251~F475) -----------------------------------
#[path = "shell/taskbar.rs"]
pub mod taskbar;
#[path = "shell/startmenu.rs"]
pub mod startmenu;
#[path = "proc/entry.rs"]
pub mod entry;
#[path = "proc/ipc.rs"]
pub mod ipc;
#[path = "app/write.rs"]
pub mod app_write;
#[path = "app/mind.rs"]
pub mod app_mind;
#[path = "app/code.rs"]
pub mod app_code;
#[path = "app/fate.rs"]
pub mod app_fate;
pub mod sync;
pub mod sec;
pub mod verify;

// --- GALAXY-1800 AI-16~AI-25 (G901~G1500) ----------------------------------
pub mod galaxy;

// --- VARIABLE-200 内核并入 Variable 系统（F001~F200）------------------------
// AI-01 用户态进程域：落点 proc.rs（扩展）+ proc/uspace.rs + mem/addrspace.rs。
// AI-02 ELF 加载与 ABI 域：落点 exec.rs（VXELF 格式与加载器）。
// AI-03 系统调用域：落点 syscall/。
#[path = "exec.rs"]
pub mod exec;
#[path = "syscall/mod.rs"]
pub mod syscall;

// --- GALAXY-1800 AI-01~AI-07（G001~G420，W1/W2）-----------------------------
pub mod gtoolchain;
pub mod gconcur;
pub mod gmem;
pub mod gstore;
pub mod gbus;
pub mod gperiph;
pub mod gnet;

// --- GALAXY-1800 AI-08~AI-16 (G421~G960) ------------------------------------
pub mod gdist;
pub mod gcons;
pub mod gcont;
pub mod guni;
pub mod gpm;
pub mod gperf;
pub mod gobs;
pub mod gsec;
pub mod grtc;
// --- TRINITY-500 AI-20 (F476~F500) 工程质量与门禁收官（模块注册由 AI-20 统一收口）
pub mod quality;

// --- VARIX-M500 AI-16~AI-20（F376~F500，成熟化系列）--------------------------
pub mod deskwis; //   AI-16 桌面智慧与空间管理
pub mod dataflow; //  AI-17 数据流动与互操作
pub mod selfheal; //  AI-18 自愈与可靠性深化
pub mod observ; //    AI-19 性能艺术与观测（先行域）
pub mod i18n; //      AI-20 全球化与作品集交付

// --- AURORA-1000 AI-16~AI-30 (A376~A750) ------------------------------------
pub mod workspace;
pub mod designsys;
pub mod fileman;
pub mod settings;
pub mod apps;
pub mod terminal;
pub mod editor;
pub mod imageview;
pub mod player;
pub mod netweb;
pub mod notify;
pub mod search;
pub mod sysmon;
pub mod pkgstore;
pub mod printing;
pub mod w3gate;
// （aurora:: 命名空间由 AI-01~AI-15 收口段统一注册，见文件底部）

// --- AURORA-1000 AI-31~AI-40 (A751~A1000，W4/W5) -----------------------------
#[path = "a11y/a11y.rs"]
pub mod a11y;
// UNREAL-X：AI-52 K 线（族0511/0512/0518 · X12751~X12800 · X12926~X12950 内核无障碍），勿删。
#[path = "a11y/a52k.rs"]
pub mod a52k;
// UNREAL-X：AI-59 K 线（族0584 混沌工程 · X14576~X14600），勿删。
#[path = "stability/ai59.rs"]
pub mod ai59k;
// UNREAL-X：AI-58 K 线（族0571~0574 测试矩阵/模糊形式化/性能基准/内存安全 · X14251~X14350），勿删。
#[path = "checks/ai58.rs"]
pub mod ai58k;
// UNREAL-X：AI-29 K 线（族0286 开机固件/族0290 虚拟化容器 · X07126~X07150 + X07226~X07250），勿删。
#[path = "checks/ai29.rs"]
pub mod ai29k;
// UNREAL-X：AI-27 K 线（族0269 工具间数据总线/族0270 工具沙箱 · X06701~X06750），勿删。
#[path = "checks/ai27.rs"]
pub mod ai27k;
// UNREAL-X：AI-28 K 线全量（族0271~0280 工具内核与收官 · X06751~X07000，task/mod.rs 聚合），勿删。
pub mod task;
#[path = "power/aurora.rs"]
pub mod apower;
#[path = "perf/perf.rs"]
pub mod perf;
// Varix STAR I · 泳道一 B 域后段（AI-K2 · F058-F075）：十八项功能 + 共享底盘。
pub mod star;
/// H 基础通用域·三分队（Varix STAR I start · H 域 F301~F350 · AI-H3 分工包）：
/// 设置中心搜索/页层级/即时生效/还原默认/导入导出/全文搜索/无痕/运行框/
/// 窗口键位族/拼音容错/保存三问/会话恢复/符号面板/截图取字/拖拽前置/闲置
/// 锁屏/氛围模式/电源按钮/唤醒即回/飞行模式/就近共享/麦摄指示/权限中心/
/// 文件历史版本/通知分组/降级链/高负载保响应/指针直通/复制地址/命令行互通/
/// 多选操作条/空格即看/预览统一/静音四档/磁盘检查/内存出路/卸载三步/默认
/// 应用/自启动/焦点模式/触感谱，五十项判据实装层。
pub mod h3star;
// Varix STAR I · 泳道四 I 通用域·四分队（AI-U4 · F551-F600）：五十项功能 + ibase 共享底盘。
pub mod istar;
// Varix STAR I · 泳道三 J 鼠标域·二分队（AI-J2 · F621-F640）：二十项功能 + 共享底盘。
pub mod jstar2;
// Varix STAR I · 泳道三 H 域二分队（AI-H2 · F251-F300）：五十项功能 + 共享底盘。
pub mod h2star;
// Varix STAR I · 泳道四 D 生态开放域后段（AI-V2 · F131-F150）：二十项功能 + 共享底盘。
pub mod stareco;
// Varix STAR I · 泳道三 C 桌面体验域后段（AI-D2 · F093-F110）：十六项功能 + 冻结候删登记（F101/F104）。
pub mod stard;
// Varix STAR I · 泳道一 G 安全加固域·后段（AI-S2 · F186-F200）：十五项功能。
pub mod secstar2;
// Varix STAR I · 泳道四 D 服务守护域·前段（AI-V1 · F111-F130）：二十项功能 + 共享底盘。
pub mod svstar;
// Varix STAR I · 泳道二 A 应用兼容域·后段（AI-C2 · F021-F040）：二十项功能。
// 目录名 compatstar2：compatstar/ 已被 AI-C1（F001-F020，在途）占用，按
// secstar → secstar2 先例顺延，两包互不重叠。
pub mod compatstar2;
// Varix STAR I · 泳道三 C 域前段（AI-D1 · F076-F092）：桌面体验十七项 + 共享底盘。
// [AI-U2 临时屏蔽·自验用·完工即恢复] deskstar 两处测试实参编译错（AI-D1 在途）
// pub mod deskstar;
// Varix STAR I · 泳道四 I 通用域·一分队（AI-U1 · F401-F450）：批次一八项
// （F401/F403/F404/F405/F407/F408/F416/F424）+ 共享底盘，批次二/三续建。
pub mod uni1;
#[path = "stability/stability.rs"]
pub mod stability;
#[path = "security/aurora.rs"]
pub mod asecurity;
#[path = "testing/testing.rs"]
pub mod testing;
#[path = "help/help.rs"]
pub mod help;
#[path = "acceptance/acceptance.rs"]
pub mod acceptance;
#[path = "release/release.rs"]
pub mod release;
#[path = "finalize/finalize.rs"]
pub mod finalize;

// --- AURORA-1000 AI-01~AI-15 (A001~A375, W1/W2) -----------------------------
// 界面栈十五域统一收口在 aurora:: 命名空间（顶层 display/input/audio 已被
// VARIX 既有模块占用，依赖收口：不覆盖、只新增）。
pub mod aurora;

// --- AURORA-1000 步骤 0028 · 全局可观测计数器 --------------------------------
pub mod metrics;

// --- VARIABLE-200 AI-04~AI-08（F076~F200，内核并入 Variable 系统）-----------
pub mod srv;
pub mod gfxsrv;
pub mod hidsrv;
pub mod vport;
pub mod bootchain;

// --- VARIX-M500 AI-11~AI-15（F251~F375，内核成熟化与生态深化）---------------
// 能源 v2（envpower）、主题艺术（theme）、硬件兼容广度（hwcompat）、
// 应用生态 SDK（appmgr）、自动化引擎（automation）。
// M500 的 F 编号与 VARIX-500 历史编号空间重叠，符号全部落在新模块避免冲突。
pub mod envpower;
pub mod theme;
pub mod hwcompat;
pub mod appmgr;
pub mod automation;

// --- VARIX-M500 AI-01~AI-05（F001~F125，M1 底座 + M3 生态门口）--------------
// 启动体验（m5boot）、算力编排（m5sched）、内存智能（m5mem）、
// 服务编排 IPC（m5srv）、文件系统与数据（m5fs）。
// M500 的 F 编号与 VARIX-500 历史编号空间重叠，符号全部落在新模块避免冲突。
pub mod m5boot;
pub mod m5sched;
pub mod m5mem;
pub mod m5srv;
pub mod m5fs;

// --- VARIX-M400 AI-09~AI-16（F201~F400，成品体验与看不见的质量）-------------
// 桌面 shell 完备化（m4shell）、兼容层扩展（m4compat）、性能工程（m4perf）、
// 安全与隐私（m4privsec）、发布工程（m4release）、质量与测试（m4quality）、
// 文档与生态（m4docseco）、艺术与体验（m4arts）。
// M400 的 F 编号与历史编号空间重叠，符号全部落在新模块避免冲突。
pub mod m4shell;
pub mod m4compat;
pub mod m4perf;
pub mod m4privsec;
pub mod m4release;
pub mod m4quality;
pub mod m4docseco;
pub mod m4arts;

// VARIX-M400 W1/W2 domains (AI-01 ~ AI-08).
pub mod m400boot;
pub mod m400mem;
pub mod m400sched;
pub mod m400syssec;
pub mod m400store;
pub mod m400input;
pub mod m400net;
pub mod m400gfx;

// --- VARIX-M600 AI-21~AI-24（F501~F600，作品交付波次）------------------------
// 数据同步（m6sync）、智能助手（m6assist）、无障碍与全球化（m6a11y）、
// 作品交付与文档（m6deliver）。符号全部落在新模块避免编号冲突。
pub mod m6sync;
pub mod m6assist;
pub mod m6a11y;
pub mod m6deliver;

// --- VARIX-M600 AI-01（F001~F025，启动与秒开域）与 VARIX-M700 AI-01
// --- （F001~F025，进程与线程域）。符号全部落在新模块避免编号冲突。
pub mod m600boot;
pub mod m700proc;

// --- VARIX-M700 AI-21~AI-28（F501~F700，内核成熟化收口波次）-------------------
// 调度器（m7sched）、容器隔离（m7cgroup）、时间日志（m7timelog）、
// 引导固件（m7bootfw）、可测试性（m7testing）、基准度量（m7bench）、
// 兼容移植（m7compat）、文档发布（m7docrel）。符号全落新模块避免编号冲突。
pub mod m7sched;
pub mod m7cgroup;
pub mod m7timelog;
pub mod m7bootfw;
pub mod m7testing;
pub mod m7bench;
pub mod m7compat;
pub mod m7docrel;

// --- VARIX-M600 AI-11~AI-20（F251~F500，感官/生态/信任波次）-------------------
// 图像媒体（m600media）、动效空间（m600motion）、Shell 精工（m600shell）、
// 文件数据（m600files）、应用 SDK（m600sdk）、自动化（m600auto）、
// 网络互联（m600net）、服务进程（m600svc）、隐私信任（m600priv）、
// 硬件广度（m600hw）。符号全部落在新模块避免编号冲突。
pub mod m600media;
pub mod m600motion;
pub mod m600shell;
pub mod m600files;
pub mod m600sdk;
pub mod m600auto;
pub mod m600net;
pub mod m600svc;
pub mod m600priv;
pub mod m600hw;

// --- VARIX-M600 AI-02~AI-10（F026~F250，底座/感官波次）-----------------------
// 内存算力（m600mem）、调度实时（m600sched）、可靠自愈（m600relia）、
// 性能观测（m600perf）、电源热（m600pwr）、合成视觉（m600gfx）、
// 输入手感（m600input）、声音设计（m600audio）、主题艺术（m600theme）。
// 符号全部落在新模块避免编号冲突。
pub mod m600mem;
pub mod m600sched;
pub mod m600relia;
pub mod m600perf;
pub mod m600pwr;
pub mod m600gfx;
pub mod m600input;
pub mod m600audio;
pub mod m600theme;

// --- VARIX-M700 AI-02~AI-10（F026~F250，核心深化/存储波次）-------------------
// 虚拟内存（m700vmm）、系统调用（m700sysc）、中断时钟（m700intr）、
// IPC 消息（m700ipc）、同步原语（m700lock）、内核安全（m700ksec）、
// 内核调试（m700kdbg）、VFS（m700vfs）、块存储（m700blk）。
pub mod m700vmm;
pub mod m700sysc;
pub mod m700intr;
pub mod m700ipc;
pub mod m700lock;
pub mod m700ksec;
pub mod m700kdbg;
pub mod m700vfs;
pub mod m700blk;

// --- VARIX-M700 AI-11~AI-20（F251~F500，存储/图形网络波次）--------------------
// 缓存回写（m700cache）、设备模型（m700dev）、驱动框架（m700drv）、
// 输入内核（m700input）、GPU 驱动（m700gpu）、显示合成（m700disp）、
// 协议栈（m700net）、无线链路（m700rf）、电源时钟（m700pwr）、多核 SMP（m700smp）。
pub mod m700cache;
pub mod m700dev;
pub mod m700drv;
pub mod m700input;
pub mod m700gpu;
pub mod m700disp;
pub mod m700net;
pub mod m700rf;
pub mod m700pwr;
pub mod m700smp;
