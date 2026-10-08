/**
 * H4 批次登记册（F351-F400 · AI-H4）：
 * 50 项 × 模块 × 主册判据摘文 × 单测的一处一事实总表——
 * F375 三走查与 F400 一行账的数据源（账、册、检三处对齐的「册」）。
 * 每行判据摘自《Varix STAR I start.md》主册 F351-F400 各条【验收判据】首句。
 */

export interface H4RegistryEntry {
  item: string;
  title: string;
  /** 实装模块文件（相对 src/system/h4/）。 */
  module: string;
  /** 主册验收判据摘文（首句）。 */
  criteria: string;
  /** 单测文件（相对 src/system/h4/__tests__/）。 */
  test: string;
}

export const H4_REGISTRY: ReadonlyArray<H4RegistryEntry> = [
  { item: "F351", title: "工作区快照", module: "f351-workspaceSnapshot.ts", criteria: "快照/恢复精度（<1px 含多屏）；未开应用自动启动顺序；快照数量上限 10 与管理；恢复时最小化窗状态还原", test: "f351-workspaceSnapshot.test.ts" },
  { item: "F352", title: "缩略图窗上直接操作", module: "f352-thumbnailOps.ts", criteria: "三型缩略图（普通/媒体/传输）操作集；关闭命中精度；迷你键响应 <100ms；悬停稳定性（操作中缩略图不消失）", test: "f352-thumbnailOps.test.ts" },
  { item: "F353", title: "跨屏拖拽位置记忆", module: "f353-crossScreenMemory.ts", criteria: "回流适配（副屏拔除 <1s 完成）；回原位精度；副屏缺席提示一次；多次插拔循环稳定性（5 次）", test: "f353-crossScreenMemory.test.ts" },
  { item: "F354", title: "任务栏资源摘要", module: "f354-resourceSummary.ts", criteria: "三图数据准确性（对账 F060/性能计数）；30s 历史刷新（1s 一点）；空闲通道判据（前台帧率无影响实测）；默认关", test: "f354-resourceSummary.test.ts" },
  { item: "F355", title: "Edge 深度协同", module: "f355-edgeSynergy.ts", criteria: "三管子用例；系统待遇清单逐项验证；下载通知联动；兼容性回退路径（A2 判例工厂引用）", test: "f355-edgeSynergy.test.ts" },
  { item: "F356", title: "PWA 应用化", module: "f356-pwaInstall.ts", criteria: "安装/卸载全链；独立窗口属性（Alt+Tab/贴靠/快照）；清单字段取用；卸载干净度扫描；列表平权审计", test: "f356-pwaInstall.test.ts" },
  { item: "F357", title: "下载收口体验", module: "f357-downloadClosure.ts", criteria: "通知双钮链路；睡眠豁免；校验失败重下路径；系统不越界审计（无第二套下载 UI）", test: "f357-downloadClosure.test.ts" },
  { item: "F358", title: "全局画中画", module: "f358-globalPip.ts", criteria: "四键功能；停靠吸附；并发上限与排队；焦点语义；回原窗状态接续（时间点不跳）", test: "f358-globalPip.test.ts" },
  { item: "F359", title: "全局屏幕拾色器", module: "f359-colorPicker.ts", criteria: "取色准确性（对已知色板 20 点零误差）；放大环倍率；HEX/RGB 双格式复制；进 F234 联动；连续模式", test: "f359-colorPicker.test.ts" },
  { item: "F360", title: "像素标尺与网格叠加", module: "f360-pixelRuler.ts", criteria: "读数准确性（已知尺寸窗口零误差）；点击穿透判据；网格 20% 透明度；量测拖拽实时性；快捷退出（Esc 秒退）", test: "f360-pixelRuler.test.ts" },
  { item: "F361", title: "屏幕录制", module: "f361-screenRecorder.ts", criteria: "三模式录制用例；开销实测（录制时前台帧率降幅 <5fps）；双轨音频；红框与浮条；产物规格（帧率/码率入册）", test: "f361-screenRecorder.test.ts" },
  { item: "F362", title: "录屏产物管理", module: "f362-recordingOutput.ts", criteria: "自动命名格式；分节 5 分钟边界；中断恢复用例（录制中断电，保留已完成分节）；大小预估误差 <10%", test: "f362-recordingOutput.test.ts" },
  { item: "F363", title: "专注计时器", module: "f363-focusTimer.ts", criteria: "两档+自定范围；徽标实时性；结束提醒双通道；放弃真实性；每日累计对账", test: "f363-focusTimer.test.ts" },
  { item: "F364", title: "下载文件夹一键整理", module: "f364-downloadsTidy.ts", criteria: "方案预览准确性（分类零错放）；勾选执行；整批撤销；自动动手=0 判据（后台无移动行为审计）；重复执行幂等", test: "f364-downloadsTidy.test.ts" },
  { item: "F365", title: "重复文件查找", module: "f365-dupeFinder.ts", criteria: "内容哈希命中（改名重复用例）；组排序与保留推荐；删除走回收站；空闲扫描；误判率（抽查 20 组人工复核）", test: "f365-dupeFinder.test.ts" },
  { item: "F366", title: "托盘电池显示选项", module: "f366-trayBattery.ts", criteria: "三档渲染；色段四档阈值与实际电量对账；低电三态独立视觉；插电标记；切换即时性与持久化", test: "f366-trayBattery.test.ts" },
  { item: "F367", title: "桌面分区吸附", module: "f367-zoneSnap.ts", criteria: "吸附 8px 阈值与参考线；分区四区定义与淡显；按区排列用例；未启用零差异判据", test: "f367-zoneSnap.test.ts" },
  { item: "F368", title: "最小化到托盘", module: "f368-minimizeToTray.ts", criteria: "声明行为与悬停提示；徽标实时性；真退出路径；任务栏不显示判据；托盘溢出收纳兼容（F075 语义）", test: "f368-minimizeToTray.test.ts" },
  { item: "F369", title: "后台任务中心", module: "f369-taskCenter.ts", criteria: "任务注册完整性（系统任务全入册）；四字段信息准确性；暂停/恢复逐任务+全局两级；调度分级对账 F057", test: "f369-taskCenter.test.ts" },
  { item: "F370", title: "后台不惊扰承诺", module: "f370-backgroundQuiet.ts", criteria: "三不逐项测试（注入后台高 IO/高错误场景测前台指标）；退避重试用例；最终失败通知一条；横幅=0 判据", test: "f370-backgroundQuiet.test.ts" },
  { item: "F371", title: "开机时长徽标", module: "f371-bootBadge.ts", criteria: "时长与 B-2x 实测链对账（误差 <100ms）；淡入淡出时序；关闭开关；历史曲线与徽标同源", test: "f371-bootBadge.test.ts" },
  { item: "F372", title: "系统活动人话时间线", module: "f372-activityTimeline.ts", criteria: "映射表覆盖率（系统事件大类全转译）；展开详情；24h 窗口与存储上限；只读+哈希链判据；时间准确性对账 F295", test: "f372-activityTimeline.test.ts" },
  { item: "F373", title: "键盘布局管理", module: "f373-keyboardLayouts.ts", criteria: "三处同步（F327 判据复用）；轮切顺序=设置顺序；切换不吞键（100 键快速混切）；布局级选项隔离；增删即时反映", test: "f373-keyboardLayouts.test.ts" },
  { item: "F374", title: "快捷键速查浮层", module: "f374-hotkeySheet.ts", criteria: "600ms 触发与松开即隐；同源实时性（改键后立即反映）；覆盖层点击穿透（松开前不误触）；应用专属组叠加；性能（覆盖层帧率无损）", test: "f374-hotkeySheet.test.ts" },
  { item: "F375", title: "H 域总判据", module: "f375-hDomainGate.ts", criteria: "三走查各出报告（数据+录屏+日期入账）；一致性对照表 100% 绿；15 分钟任务链无卡壳；走查不过的条目回炉不发布", test: "f375-hDomainGate.test.ts" },
  { item: "F376", title: "标题栏系统菜单", module: "f376-systemMenuMatrix.ts", criteria: "六项顺序对照；置灰状态机（四状态×六项矩阵）；键盘模式衔接；菜单几何偏移", test: "f376-systemMenuMatrix.test.ts" },
  { item: "F377", title: "键盘移动与调整窗口", module: "f377-keyboardWindowEdit.ts", criteria: "移动/大小两模式方向键行为；Enter 落定精度（<1px）；Esc 还原；半透明提示；边界贴边", test: "f377-keyboardWindowEdit.test.ts" },
  { item: "F378", title: "列宽双击自适应", module: "f378-columnAutofit.ts", criteria: "自适应算法（最长可见项+12px 余量）；拖拽气泡实时；最右列吃满；记忆联动；最小 40px 钳制", test: "f378-columnAutofit.test.ts" },
  { item: "F379", title: "表头排序指示", module: "f379-headerSort.ts", criteria: "三态循环；多级排序指示与上限；滚动位置保持判据；与 F219 记忆联动；即时性 <100ms", test: "f379-headerSort.test.ts" },
  { item: "F380", title: "空行点击清除选择", module: "f380-blankClick.ts", criteria: "三点击行为（单击/Ctrl 单击/右键）；空白菜单项清单审计（无对象操作=0）；清除后焦点处理；与 F203 框选衔接", test: "f380-blankClick.test.ts" },
  { item: "F381", title: "树形控件半选与记忆", module: "f381-treeTriState.ts", criteria: "三态联动用例（勾/取消/半选传递）；展开态记忆（重启验证）；键盘四操作；半选视觉规范（横杠居中 60% 宽）", test: "f381-treeTriState.test.ts" },
  { item: "F382", title: "对话框位置记忆", module: "f382-dialogPosition.ts", criteria: "居中偏上基准；拖动记忆（同类归组规则入册）；跨屏完整落屏判据；记忆容量上限与淘汰", test: "f382-dialogPosition.test.ts" },
  { item: "F383", title: "弹窗排队不叠罗汉", module: "f383-modalQueue.ts", criteria: "模态唯一性审计（并发模态=0）；排队 200ms 间隔；横幅 3 条上限与溢流；焦点唯一；队列公平（先到先出）", test: "f383-modalQueue.test.ts" },
  { item: "F384", title: "焦点陷阱（模态完整性）", module: "f384-focusTrap.ts", criteria: "陷阱完整性（模态内 Tab 50 次零逃逸）；非模态不困判据；归还联动 F206；陷阱开启/关闭即时性", test: "f384-focusTrap.test.ts" },
  { item: "F385", title: "无障碍语义树", module: "f385-semanticTree.ts", criteria: "语义树覆盖率（全系统控件扫描，无名可交互元素=0）；角色/状态完整性抽查 50 控件；弹窗播报；第三方读屏接口文档公开（F135 文档站）", test: "f385-semanticTree.test.ts" },
  { item: "F386", title: "阅读模式", module: "f386-readingMode.ts", criteria: "三参数（行距 1.6/45 字/衬线可选）实测；内容零修改判据（开关前后文本哈希一致）；模式入口统一（查看菜单）；记忆（每应用记住开关态）", test: "f386-readingMode.test.ts" },
  { item: "F387", title: "灰度模式", module: "f387-grayscaleMode.ts", criteria: "全系统灰度一致性（滤镜单点审计）；灰度下关键操作走查（20 任务全绿——无障碍设计质量的试金石）；切换即时性；与 F113 高对比度/F114 色弱滤镜互斥逻辑", test: "f387-grayscaleMode.test.ts" },
  { item: "F388", title: "竖屏与异形屏适配", module: "f388-portraitAdaptation.ts", criteria: "竖屏贴靠形制用例；任务栏侧边模式；单列默认判据；跨屏重适配 <100ms；异形圆角安全区（若有）不遮内容", test: "f388-portraitAdaptation.test.ts" },
  { item: "F389", title: "滚动长截图", module: "f389-scrollStitch.ts", criteria: "拼接去重算法用例（3 个典型页面）；动态内容诚实失败；接缝人工评审记录；产物规格与保存链", test: "f389-scrollStitch.test.ts" },
  { item: "F390", title: "选中文本查词", module: "f390-wordLookup.ts", criteria: "离线判据（断网全功能）；释义卡时序（<300ms 呼出）；不抢焦点；生词 20 条；词典大小与加载（按需分页载入内存）", test: "f390-wordLookup.test.ts" },
  { item: "F391", title: "选中文本翻译", module: "f391-textTranslate.ts", criteria: "长短分界（500 字符）两路用例；Edge 跳转参数（选中内容带过去）；浮卡同形制复用 F390 判据；离线时诚实提示「翻译需要网络」", test: "f391-textTranslate.test.ts" },
  { item: "F392", title: "文件夹大小列", module: "f392-folderSize.ts", criteria: "异步淡入填入；「~」估算标注阈值；计量服务单点审计；缓存（目录内容不变时秒出）；与三功能同源对账", test: "f392-folderSize.test.ts" },
  { item: "F393", title: "存储热点图", module: "f393-storageTreemap.ts", criteria: "面积与真实占用对账（±2%）；下钻流畅（三层 <500ms）；点选联动定位；榜单准确性；绘制开销（空闲通道 F369 纪律）", test: "f393-storageTreemap.test.ts" },
  { item: "F394", title: "清理建议收口页", module: "f394-cleanupSummary.ts", criteria: "四来源汇总对账（各项数字与源头一致）；总账准确；不可逆标注与二次确认；执行后空间实测回收 ≥90% 账面值", test: "f394-cleanupSummary.test.ts" },
  { item: "F395", title: "U 盘健康监控", module: "f395-usbHealth.ts", criteria: "读得到/读不到两分支用例；写入量累计准确性（对账 IO 计数）；两阈值提示与出路链接；多介质（S: 共享卷）分列", test: "f395-usbHealth.test.ts" },
  { item: "F396", title: "备份向导", module: "f396-backupWizard.ts", criteria: "三步流程用例；增量链还原（连做三次增量后整体还原比对）；可恢复性校验判据；暂停续传；提醒周期设置", test: "f396-backupWizard.test.ts" },
  { item: "F397", title: "还原演练", module: "f397-restoreDrill.ts", criteria: "季度提醒周期；引导步骤与 F198 三卡联动；模拟零副作用判据；演练记录；首次恢复加练提示", test: "f397-restoreDrill.test.ts" },
  { item: "F398", title: "界面语言热切", module: "f398-languageHotSwap.ts", criteria: "换装 <1 分钟实测；状态保持；回退标注机制；词条覆盖率对账 F132；RTL 接口存在性（代码判据）", test: "f398-languageHotSwap.test.ts" },
  { item: "F399", title: "彩蛋总谱", module: "f399-easterEggs.ts", criteria: "三枚触发条件与一次性判据；性能零影响（彩蛋动画走 F124 谱）；不藏功能审计；谱系动画与 F199 数据同源", test: "f399-easterEggs.test.ts" },
  { item: "F400", title: "H 域收官登记", module: "f400-hDomainClosure.ts", criteria: "一行账与正文标题 100% 一致（脚本生成保证）；总检检查点新增 200 个全绿基线；三处同源审计；F200 条款修订完成", test: "f400-hDomainClosure.test.ts" },
];

/** 登记册完整性自检：50 项、编号连续、模块与测试文件名一一对应。 */
export function auditRegistry(): { pass: boolean; problems: string[] } {
  const problems: string[] = [];
  if (H4_REGISTRY.length !== 50) problems.push(`登记册 ${H4_REGISTRY.length} 项 ≠ 50`);
  H4_REGISTRY.forEach((e, i) => {
    if (e.item !== `F${351 + i}`) problems.push(`编号断档：第 ${i} 行是 ${e.item}`);
    if (!e.module.toLowerCase().startsWith(e.item.toLowerCase())) problems.push(`${e.item}: 模块文件名未挂编号`);
    if (!e.test.toLowerCase().startsWith(e.item.toLowerCase())) problems.push(`${e.item}: 测试文件名未挂编号`);
  });
  return { pass: problems.length === 0, problems };
}

/** 一行账数据源（F400 消费）。 */
export function registryAsTitles(): Array<{ item: string; title: string }> {
  return H4_REGISTRY.map((e) => ({ item: e.item, title: e.title }));
}
