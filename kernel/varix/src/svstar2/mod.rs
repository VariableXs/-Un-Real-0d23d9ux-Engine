//! svstar2 — Varix STAR II · VE 图形引擎册 · 用户态服务落位。
//!
//! # 落位依据（★ 位置铁律，勿改 ★）
//!
//! 《VE Varix STAR II · 总纲与施工书》铁律 6：
//! > **内核红线**：全部功能针对 VARIX Rust 内核编写；内核态只留合成裁决与安全，
//! > VE 全部活在用户态服务；引擎崩溃零拖垮。
//!
//! 读法（勿误读为"能用前端语言写"）：
//! - 「用户态服务」指**不在内核态特权上下文运行**——崩溃不拖垮内核；
//! - 本仓内核树是 no_std Rust（`kernel/varix/Cargo.toml` 的 `[dependencies]` 为空，
//!   `[[bin]]` 带 `required-features = ["kernel-image"]`）；
//! - TypeScript 编译出 JS，**必须靠 V8/JSC 运行时才能执行**，内核态无堆、无 GC、
//!   无动态链接，装不下也不允许跑；且 GC 停顿与内核延迟硬要求直接冲突。
//! - 故 VE 全 6400 项落地为 **Rust**，本目录即其家。
//!
//! 目录名沿用仓库既有命名先例（`svstar` 服务守护域 → `svstar2` 承载 STAR II）。
//!
//! # 命名约定
//!
//! | 前缀 | 含义 |
//! | --- | --- |
//! | `vea01` … `vea10` | VE-A 域（内核图形抽象层，F0001-F0200）|
//! | `veb01` … `veb10` | VE-B 域（GPU 驱动矩阵，F0201-F0400）|
//! | 后续按域顺延 | 域字母与册内 `VE-x` 一致 |
//!
//! # 与既有模块的关系纪律（对齐 svstar/ 与 genstar2/ 的域约）
//!
//! - 本目录只放 **VE 册**功能；CoRun 册（操作系统本体/三通道兼容）走既有内核模块，
//!   CGPU 册（计算内核）另设落位——三册互不混装；
//! - 每个模块自带 `run_*_checks() -> CheckSet` 自检并登记进本域聚合器，
//!   聚合器在 `robust.rs` 以单行注册（不占 domains 定长数组名额）；
//! - 每个模块带 `#[cfg(test)]` 单元测试，宿主侧 `cargo test` 直跑；
//! - 全部确定性算法、零 IO、可序列化：时间用逻辑 tick 注入，不用墙钟——
//!   保证回归可复现、对拍可重现（规格对拍红线）。
//!
//! # 域内模块地图（施工中，随进度增补）
//!
//! | 模块 | 功能 | 判据锚 |
//! | --- | --- | --- |
//! | [`vek10_fxaa`] | F2010 抗锯齿四法之 FXAA（单 pass 全屏：亮度对比边缘检测→边缘方向→定向模糊，Console/FXAA 3.11 质量档；低/中/高三预设阈值+跨度+步数三轴皆不同，档位强度用 touched 集合包含关系 S(low)⊆S(mid)⊆S(high) 验证；输出取值集闭包 {中心,A,B} 逐位相等故闭包判据用 == 而非近似；序位守卫三违规带三要素；模糊代价以合成高频图案的高频能量比作可复算证据；双线性采样是正确性前提——最近邻会让 A/B 落回中心像素使 FXAA 退化为空操作而结构判据全绿；与 CAS 联动给警告而非静默关掉） | VE 册 #VE-F2010 |
//! | [`vea01`] | F0001 虚拟显卡探测仲裁器 | VE 册 #VE-F0001 |
//! | [`veb13_heads`] | F0213 virtio 多头与 EDID（逐输出独立使能与模式/EDID 三级注入优先级与非法拒载保默认/热增删幂等重算以最新 cfg 为准/布局表 O(输出数) 应用与版本化持久化/位置语义播报名） | VE 册 #VE-F0213 |
//! | [`veb14_perf`] | F0214 virtio 性能与诊断接口（按帧统计命令数/传输字节/队列深度与提交到完成延迟的对数分桶草图在线更新分位/诊断快照按需拉取不常驻无请求零开销/计数溢出饱和不回绕且可区分恰好等于上限与已饱和/打点缺失标记缺测不入分位不补零/快照请求并发串行化排队有界满则拒收不覆盖/计数入遥测总线受 F0096 预算治理按提交序确定性抽样） | VE 册 #VE-F0214 |
//! | [`veb15_suite`] | F0215 virtio 一致性测试套件（三层用例：协议层状态机序与特性协商矩阵/功能层资源创建导出与 2D 更新与多头切换回归/恢复层注入错误验 F0212 处置矩阵；命令流编码走黄金流比对且对齐 F0206 版本戳，版本不匹配判阻断而非降级、重建黄金流须显式人工确认；宿主侧 fast 档与 QEMU 批队列 full 档为**同一份清单的两个投影**、full 是全集 fast 是子集；缺特性用例判 N/A 三值枚举单列统计且不算失败不计入通过数、N/A 早退路径零建销不污染清理对账；清理断言为独立收尾步、创建数须恰等于本档真跑用例数且过度销毁判失败；失败输出含复现最小命令序列） | VE 册 #VE-F0215 |
//! | [`vea02`] | F0002 图形上下文生命周期管理器 | VE 册 #VE-F0002 |
//! | [`vea03`] | F0003 围栏与同步原语集 | VE 册 #VE-F0003 |
//! | [`vea04`] | F0004 命令缓冲环形分配器 | VE 册 #VE-F0004 |
//! | [`vea18_sampler`] | F0018 采样器状态库 | VE 册 #VE-F0018 |
//! | [`vea19_blend`] | F0019 混合状态机（四维+独立alpha/预置库/漂移失效缓存/实时预览） | VE 册 #VE-F0019 |
//! | [`vea11_hotplug`] | F0011 适配器热插拔与路径重选 | VE 册 #VE-F0011 |
//! | [`vea12_probe`] | F0012 渲染探针与时间戳基础设施 | VE 册 #VE-F0012 |
//! | [`vea13_softfall`] | F0013 软渲染回退路径 | VE 册 #VE-F0013 |
//! | [`vea14_snapshot`] | F0014 上下文快照与场景重放 | VE 册 #VE-F0014 |
//! | [`vea15_errclass`] | F0015 渲染错误分类与上抛纪律 | VE 册 #VE-F0015 |
//! | [`ved13_dirty`] | F0613 图层脏区收集（树级） | VE 册 #VE-F0613 |
//! | [`vew01_sdk_arch`] | F4601 W 域开工与插件 SDK 总架构（四层/双承诺/承接/层冻结） | VE 册 #VE-F4601 |
//! | [`ved14_traverse`] | F0614 图层渲染遍历器 | VE 册 #VE-F0614 |
//! | [`ved15_cache`] | F0615 图层缓存策略 | VE 册 #VE-F0615 |
//! | [`ved16_scale`] | F0616 大层数性能（虚拟化与扁平化） | VE 册 #VE-F0616 |
//! | [`ved17_consistency`] | F0617 图层树一致性校验 | VE 册 #VE-F0617 |
//! | [`ved18_debugview`] | F0618 图层树调试可视化（四开关覆盖/独立调试通道/只读面板接口/懒加载展开预算/树文本转储省略契约/结构级隐私边界） | VE 册 #VE-F0618 |
//! | [`ved19_surface`] | F0619 图层树与表面协议对接（表面帧协议/相位机半帧不落盘/damage 双向回流/提交点契约/缓冲引用世代与所有权显性） | VE 册 #VE-F0619 |
//! | [`ved20_closeout`] | F0620 图层树组收口（十九件证据实探齐备核验/证据缺项阻断回补/台账指纹绑定的双签/移交授权独立于双签/一致性趋势劣化登记/回归回溯/经验包三契约机检谓词/无障碍核验行三态） | VE 册 #VE-F0620 |
//! | [`ved21_blendreg`] | F0621 混合模式规范实现总纲（声称24vs条款18差额逐格登记含未归因项/三路同公式以FormulaId全等判定/条款段号与行号逐条对账/四条横切纪律按族绑定且反装饰/抽样覆盖面位图显性/条目级N/A必带理由/歧义裁决留痕拒空条款号） | VE 册 #VE-F0621 |
//! | [`ved22_separable`] | F0622 可分离混合模式 12 种（公式单一来源IR三路物化：标量/SIMD宽通道/WGSL文本；dodge与burn除零取值方向相反分档携带无传参错配面；soft-light三段全实现含D分支与连续段夹逼探针；可分离性机器证明=通道置换等变+alpha签名分离；反假变体19/19捕获含4条初测漏网补强） | VE 册 #VE-F0622 |
//! | [`vea20_stencil`] | F0020 深度模板状态机 | VE 册 #VE-F0020 |
//! | [`vea21_raster`] | F0021 光栅化状态机 | VE 册 #VE-F0021 |
//! | [`vea22_vlayout`] | F0022 顶点输入布局描述器（声明式偏移推导/编译期签名闸门/对齐修正/规范形去重/A27 位置空间） | VE 册 #VE-F0022 |
//! | [`vea23_psocache`] | F0023 渲染管线对象缓存（四段缓存：键构造/查表命中/异步编译/LRU 淘汰；哈希只查表、等价由描述符全等判定；编译失败降级带原因与上限；命中率四桶归因；预热把卡顿挪到加载期；持久化只存键与就绪事实） | VE 册 #VE-F0023 |
//! | [`vea24_hotreload`] | F0024 着色器热重载协调器（四段流水：请求合并/重编译/管线重建/场景验证；坏码不落地靠 active+staged 双份槽位、验证通过才提升；回滚真有快照可退；冻结是状态位非线程锁；断点期间排队保最新） | VE 册 #VE-F0024 |
//! | [`vea25_cbuf`] | F0025 常量缓冲更新策略器（四档策略按作用域与时机两轴分；成本模型=字节×每帧次数且不摊一次性切换成本；脏标记精确到槽位；漂移双向校准且假净优先并记动作序；失配建议落到具体档位附代价对比） | VE 册 #VE-F0025 |
//! | [`vea26_desc_heap`] | F0026 描述符堆与绑定模型（堆三段：分配/分片/复用；跨 D3D12/Vulkan/Metal 抽象，差异只在绑定类别数/槽位编号规则/一帧绑定次数三处，绑定语义三家共用同一来源；耗尽防护给可行动原因；分帧回收有预算且在飞描述符一律不收） | VE 册 #VE-F0026 |
//! | [`vea27_bindlayout`] | F0027 绑定布局编译器（离线编译：反射自动生成+手写布局双源交叉校验，不一致以反射为准并报差异；槽位重复/越界/断裂三类冲突，重复指名两处下标；产物缓存供A23 PSO 缓存键使用，指纹只查表、等价靠全等复核、撞而内容不等判损坏重编；反射部分失败阻断不退手动；超编译预算不阻断产出只报并归因最慢一步） | VE 册 #VE-F0027 |
//! | [`vem02_track`] | F2402 关键帧轨道系统（六类轨道/容器多轨/绑定协议/单源扩展） | VE 册 #VE-F2402 |
//! | [`vem03_interp`] | F2403 关键帧插值（四插值器/可插拔注册/确定性/贝塞尔纪律） | VE 册 #VE-F2403 |
//! | [`vem04_batch`] | F2404 关键帧批量操作（四操作/语义单源/单步撤销/原子事务） | VE 册 #VE-F2404 |
//! | [`vem05_asset`] | F2405 动画曲线资产（m.anim. 开放容器/生态单点/F1948 签名三件/F1956 版本三件/往返逐位零损失） | VE 册 #VE-F2405 |
//! | [`vem06_event`] | F2406 动画事件轨（事件总线第三生产者 M 域 + F2402 第七类离散轨 + 正播触发/倒播默认抑制语义表 + 同帧去抖取末值 + F1925/F2322 家族同构） | VE 册 #VE-F2406 |
//! | [`vem06_checks`] | F2406 域自检（判据逐条映射，40 项） | VE 册 #VE-F2406 |
//! | [`vem07_perf`] | F2407 动画求值性能（类型×时间轴指纹分批 + 零分配热路径 + 值/脏标记缓存 + LOD 先于精度降级次序） | VE 册 #VE-F2407 |
//! | [`vem07_checks`] | F2407 域自检（判据逐条映射，126 项分 a/b/c 三族） | VE 册 #VE-F2407 |
//! | [`ven02_tree`] | F2602 控件树模型（四要素/三不变量/三操作原子事务/M04 绑定路径解析；自 F2603 迁入的 Rust 权威实现） | VE 册 #VE-F2602 |
//! | [`ven03_ctype`] | F2603 控件类型体系（六类最小集/扩展三件套/类型注册制/内核-上层分层边界） | VE 册 #VE-F2603 |
//! | [`ven03_checks`] | F2602/F2603 域自检（判据逐条映射，55 项分三批落集） | VE 册 #VE-F2603 |
//! | [`ven04_prop`] | F2604 控件属性系统（四段管线/依赖属性继承+绑定/M04 属性侧兑现/零风暴纪律） | VE 册 #VE-F2604 |
//! | [`ven04_checks`] | F2604 域自检（判据逐条映射，56 项分两批落集） | VE 册 #VE-F2604 |
//! | [`ven05_dual`] | F2605 逻辑-可视双树与模板展开（双树分离/模板展开时机/单向数据流/三遍历/D-N 边界/同步断言/降级矩阵） | VE 册 #VE-F2605 |
//! | [`ven05_checks`] | F2605 域自检（判据逐条映射，56 项分两批落集） | VE 册 #VE-F2605 |
//! | [`ven06_incr`] | F2606 控件树增量更新（精确失效/帧边界批处理/三分发/双树增量同步；溢出强制提交+错路逐条审计） | VE 册 #VE-F2606 |
//! | [`ven06_checks`] | F2606 域自检（判据逐条映射，50 项分两批落集） | VE 册 #VE-F2606 |
//! | [`ver01_arch`] | F3401 令牌运行时架构（四件两律总纲） | VE 册 #VE-F3401 |
//! | [`ver01b_parser`] | F3402 令牌解析器（JSON/TOML 双格式 + 引用 DAG + 迭代 DFS 环检测 + 断链三要素） | VE 册 #VE-F3402 |
//! | [`ver01c_cascade`] | F3403 令牌依赖图与级联（依赖图可视化 + 批量合并级联 + 双深度闸 + 耗时画像） | VE 册 #VE-F3403 |
//! | [`ver01d_switch`] | F3404 主题切换事务（原子换肤：双缓冲单指针翻转 + 预演干跑 + 快照回滚 + 悬空兜底 + 截图一致性断言） | VE 册 #VE-F3404 |
//! | [`ver02_arch`] | F3601 R 域开工与域号 ADR（跳段裁决+五板块十项映射+四域分工+收敛复述） | VE 册 #VE-F3601 |
//! | [`ver03_arch`] | F3602 创作生态总架构（三层五段+开放格式P0+激励双单源+沙箱复述+收敛两段线） | VE 册 #VE-F3602 |
//! | [`ver04_arch`] | F3603 创作资产模型（七要素+七类两轴+许可三态+兼容四级+schema两级复用） | VE 册 #VE-F3603 |
//! | [`vep01_arch`] | F3001 P 域开工与动效库总架构（三组接口+十项映射+单源分工+三底线+第一红线） | VE 册 #VE-F3001 |
//! | [`vep02_lang`] | F3002 动效设计语言总纲（四原则+四级时长+语义化缓动+内建 reduce+单源取值） | VE 册 #VE-F3002 |
//! | [`vep02_checks`] | F3002 域自检（判据逐条映射，171项分两批落集） | VE 册 #VE-F3002 |
//! | [`vep03_token`] | F3003 动效令牌体系（时长/缓动/位移三族+跨主题恒定+单源注入+reduce 令牌层+硬编码 lint） | VE 册 #VE-F3003 |
//! | [`vep03_checks`] | F3003 域自检（判据逐条映射，分两批落集） | VE 册 #VE-F3003 |
//! | [`vep04_stack`] | F3004 与 VE-M/O04 动画栈关系（三层分工+三选一决策表+控制接口 v2+对拍红线） | VE 册 #VE-F3004 |
//! | [`vep04_checks`] | F3004 域自检（判据逐条映射，分两批落集） | VE 册 #VE-F3004 |
//! | [`vep05_orch`] | F3005 转场编排器（DAG 三边型+四原语+打断三策略+嵌套上限 8+统一 reduce） | VE 册 #VE-F3005 |
//! | [`vep05_checks`] | F3005 域自检（判据逐条映射，分两批落集） | VE 册 #VE-F3005 |
//! | [`veq01_pipeline`] | F3201 Q 域资源管线总架构（六段签名+十项映射+收敛红线） | VE 册 #VE-F3201 |
//! | [`veq02_graph`] | F3202 资源模型与引用图（五要素+四用途单源+32MB 红线） | VE 册 #VE-F3202 |
//! | [`vee01_arch`] | F0801 文字渲染域总架构（四段单向流+ 三向兑现 + 1.5ms 预算） | VE 册 #VE-F0801 |
//! | [`vee02_utf8`] | F0802 字符编码与 UTF-8 解码（四档处置 + 偏移表 + 200MB/s） | VE 册 #VE-F0802 |
//! | [`vee03_outline`] | F0803 字形轮廓与贝塞尔（二次升三次 + 围向约定 + 1/64 量化） | VE 册 #VE-F0803 |
//! | [`vee07_prims`] | F0807 文本图元渲染（合批 2000 + 绘制≤2 + 四效果定长 uniform + 变换） | VE 册 #VE-F0807 |
//! | [`vet01_a11y_render_pipeline`] | F3802 无障碍渲染管线 | VE 册 #VE-F3802 |
//! | [`vet02_highcontrast_engine`] | F3803 高对比渲染引擎（令牌段+后处理段+语义保持红线） | VE 册 #VE-F3803 |
//! | [`ves01_sdomain_arch`] | F3801 S 域开工与无障碍渲染总架构 | VE 册 #VE-F3801 |
//! | [`vei02_locale`] | F4002 语言标签与 Locale 模型（BCP47 四段+扩展/容错表/回退链/解析缓存/单源声明） | VE 册 #VE-F4002 |
//! | [`vei03_text_direction`] | F4003 文字方向模型（三方向统一+三层优先级/isolate自动补齐/首强启发可覆写） | VE 册 #VE-F4003 |
//! | [`vei04_typeset`] | F4004 国际化排版管线（四族路由/语言覆盖红线/降级显性/五段策略与执行分工） | VE 册 #VE-F4004 |
//! | [`vei05_font`] | F4005 字体国际化选型（四族字体集/回退链配置/字符覆盖验证/度量对齐/许可地域/三单源复用） | VE 册 #VE-F4005 |
//! | [`vei06_datetime`] | F4006 日期时间数字格式（CLDR 规则集锚定/五类格式/多历法转换/时区断言/热表单源） | VE 册 #VE-F4006 |
//! | [`vei07_plural`] | F4007 复数与性别规则（CLDR 六类复数/规则族函数表/语法性别模板/联合选择器/双向覆盖红线/缓存键七段） | VE 册 #VE-F4007 |
//! | [`vef01_pngdec`] | F1001 PNG 解码器核心（签名/IHDR七参数/PLTE/tRNS/反滤波/CRC 分级/输出 RGBA） | VE 册 #VE-F1001 |
//! | [`vef02_pngenc`] | F1002 PNG 编码器核心（五滤波两策略/zlib 与九档权衡/IDAT 分块/CRC/颜色降档/场景建议表） | VE 册 #VE-F1002 |
//! | [`vef03_adam7`] | F1003 PNG 交错模式（七遍常量表唯一来源/解码重排/编码拆分/每遍独立滤波/空遍合法/截断渐进语义） | VE 册 #VE-F1003 |
//! | [`veg03_webm_mkv`] | F1203 WebM/MKV 容器解封装（VINT 八宽度/未知长度重同步边界/Segment-Track-Cluster/Block lacing 三模式/编解码器承接/Attachment/Cues 索引/与 MP4 三维差异/ffprobe 对拍） | VE 册 #VE-F1203 |
//! | [`veg04_h264`] | F1204 H.264 解码器（NAL 头与起始码/防竞争剥离/位流读取器 ue-se/SPS-PPS 反序列化与热更新/切片头变长解码/帧内九模式/六抽头分像素插值/反量化与整数 IDCT/去块 bS/CAVLC-CABAC 双引擎/DPB 滑动窗口与溢出防护/畸形拦截） | VE 册 #VE-F1204 |
//! | [`veg04_checks`] | F1204 域自检（判据逐条映射，按码流层与像素层分两批共 169 项；去块 bS 双路径与三张规范表、整数 IDCT 两趟对称、六抽头与色度单次四点、CAVLC renorm 次数、16 类畸形分桶全覆盖与两条记账恒等式） | VE 册 #VE-F1204 |
//! | [`vef03_checks`] | F1003 域自检（判据逐条映射，24 项） | VE 册 #VE-F1003 |
//! | [`vef04_color`] | F1004 PNG 色彩管理（iCCP/sRGB/gAMA/cHRM 四块解析/优先级表唯一裁决/统一标注单一出口/线性化 F0159 联动） | VE 册 #VE-F1004 |
//! | [`vef04_checks`] | F1004 域自检（判据逐条映射，21 项） | VE 册 #VE-F1004 |
//! | [`veu01_arch`] | F4201 U 域开工与一致性总架构（五层+接口冻结+承接落地+双维入约） | VE 册 #VE-F4201 |
//! | [`veu02_model`] | F4202 跨域一致性模型（四类×三型+关系代数+环检测+红线+版本化） | VE 册 #VE-F4202 |
//! | [`veu03_registry`] | F4203 契约注册中心（四能力+五字段冻结+唯一性+引用计数+生命周期） | VE 册 #VE-F4203 |
//! | [`veu04_engine`] | F4204 一致性规则引擎（三段式可执行化/四路单源引用不复制/三元裁决含僵局升级与全平票非冲突/无豁免红线处置仅两向/全量增量显式覆盖范围/引用断阻断执行） | VE 册 #VE-F4204 |
//! | [`veu08_density`] | F4008 排版密度与语言（按书写系统族适配密度档复用 F3442 三档语义/+35% 膨胀红线断言/折行优先省略最后且策略留痕/单源复用可机检） | VE 册 #VE-F4008 |
//! | [`veu08_checks`] | F4008 域自检（六族判据：膨胀/密度/折行/策略/单源/错误路径） | VE 册 #VE-F4008 |
//! | [`veu09_ime`] | F4009 输入法协同（IME×i18n：组合期不改内容不变量/组合期命令键禁令/候选窗三轴跟随夹取/方向×插入点契约前向声明/语言→输入法切换联动台账） | VE 册 #VE-F4009 |
//! | [`veu09_checks`] | F4009 域自检（六组判据：组合期单源/候选跟随/方向联动/切换断言/IME 协同/判据自检） | VE 册 #VE-F4009 |
//! | [`vec14_include`] | F0414 include 解析与循环防护（搜索序显性+ 环检测输出环 + 包含图 + 缓存裁定） | VE 册 #VE-F0414 |
//! | [`vec15_encoding`] | F0415 源码编码处理（BOM 最长匹配优先 + UTF-8 假定显式留痕 + 非法字节五类分立报错 + 单遍转换到位） | VE 册 #VE-F0415 |
//! | [`vec16_report`] | F0416 词法错误报告（四族查表归类 + 三要素带规则引用 + 双侧定位 + 三级分级） | VE 册 #VE-F0416 |
//! | [`vec17_recover`] | F0417 词法错误恢复策略（三策略按类别查表选用 + 恢复显性计数 + 级联窗口反馈回退 + 预算兜底强制同步） | VE 册 #VE-F0417 |
//! | [`vec18_perf`] | F0418 词法性能工程（单遍零回溯断言 + 缓冲区复用池化 + 记号流arena 紧凑存储 + 流式内存上界 + 吞吐基准版本化退化门） | VE 册 #VE-F0418 |
//! | [`vec19_fuzz`] | F0419 词法 fuzz 测试（自持 LCG 三层语料 + 四不变量 + 🔴即时修发现账 + 种子三元组可复现 + 语料退化/未修🔴 阻断门禁） | VE 册 #VE-F0419 |
//! | [`vec19_checks`] | F0419 域自检（判据逐条映射，54 项） | VE 册 #VE-F0419 |
//! | [`vec20_closure`] | F0420 词法组收口（十八件证据集合差齐备 + 🔴清零/🟡闭环/🟢登记三档总账 + 双签主体相异 + 三条经验可机检下游动作 + 上游基准退化原样上抛） | VE 册 #VE-F0420 |
//! | [`vec20_checks`] | F0420 域自检（判据逐条映射，56 项） | VE 册 #VE-F0420 |
//! | [`vec21_parser`] | F0421 语法分析器架构（递归下降手写分析器选型留痕含被拒策略逐条理由 / 记号流前瞻窗口 k=2 不回扫由消费水位结构性保证 / 解析只产生动作 AST 构建降为可替换消费者且回调拒绝被隔离不影响状态机 / 解析深度上限超限报嵌套源头而非当前位置 / 文法派生表 FIRST-FOLLOW 规范期拦截歧义且可前缀分解不误拦 / 诊断码锚点引用 F0420 移交两条可机检下游动作兑现） | VE 册 #VE-F0421 |
//! | [`vec21_checks`] | F0421 域自检（判据逐条映射，78 项：选型决策 14 / 前瞻窗口 10 / 动作分离 11 / 深度防护 13 / 锚点引用 8 / 文法冲突 10 / 性能 6 / 进度保底 6） | VE 册 #VE-F0421 |
//! | [`ves04_flow`] | F3604 创作工作流引擎（DAG 契约复用 F3005+ 三预置流 + 断点续作 + 沙箱 + 驱动协议） | VE 册 #VE-F3604 |
//! | [`ves04_checks`] | F3604 域自检（判据逐条映射，55 项分两批落集） | VE 册 #VE-F3604 |

pub mod vea01_arbitrate;
pub mod vea01_engine;
pub mod vea01_index;
pub mod vea01_probe;
pub mod vea01_virtfeat;
pub mod vea02_ctx;
pub mod vea03_checks;
pub mod vea03_sync;
pub mod vea04_checks;
pub mod vea04_ring;
pub mod vea05_budget;
pub mod vea05_checks;
pub mod vea06_checks;
pub mod vea06_qsched;
pub mod vea07_caps;
pub mod vea07_checks;
pub mod vea08_checks;
pub mod vea08_handle;
pub mod vea09_checks;
pub mod vea09_recovery;
pub mod vea10_bus;
pub mod vea10_checks;
pub mod vea11_checks;
pub mod vea11_hotplug;
pub mod vea12_checks;
pub mod vea12_probe;
pub mod vea13_checks;
pub mod vea13_softfall;
pub mod vea14_checks;
pub mod vea14_snapshot;
pub mod vea15_checks;
pub mod vea15_errclass;
pub mod vea18_checks;
pub mod vea18_sampler;
pub mod vea19_blend;
pub mod vea19_checks;
pub mod veb01_checks;
pub mod veb01_device;
pub mod veb01_init;
pub mod veb01_report;
pub mod veb02_checks;
pub mod veb02_proto;
pub mod veb02_queue;
pub mod veb03_checks;
pub mod veb03_resource;
pub mod veb04_2dupdate;
pub mod veb04_checks;
pub mod veb05_checks;
pub mod veb05_virgl;
pub mod veb06_checks;
pub mod veb06_stream;
pub mod veb10_checks;
pub mod veb10_cursor;
pub mod vec01_checks;
pub mod vec01_constitution;
pub mod vec02_checks;
pub mod vec02_spec;
pub mod vec03_checks;
pub mod vec03_lexer;
pub mod vec04_checks;
pub mod vec04_keywords;
pub mod vec05_checks;
pub mod vec05_ident;
pub mod vec06_checks;
pub mod vec06_lit;
pub mod vec07_checks;
pub mod vec07_string;
pub mod vec08_checks;
pub mod vec08_comment;
pub mod vec09_checks;
pub mod vec09_operator;
pub mod vec10_brace;
pub mod vec10_checks;
pub mod vec11_checks;
pub mod vec11_prepro;
pub mod vec12_checks;
pub mod vec12_macro;
pub mod vec13_checks;
pub mod vec13_cond;
pub mod vec14_checks;
pub mod vec14_include;
pub mod vec15_checks;
pub mod vec15_encoding;
pub mod vec16_checks;
pub mod vec16_report;
pub mod vec17_checks;
pub mod vec17_recover;
pub mod vec18_checks;
pub mod vec18_perf;
pub mod vec19_checks;
pub mod vec19_fuzz;
pub mod vec20_checks;
pub mod vec20_closure;
pub mod vec21_checks;
pub mod vec21_parser;
pub mod ved01_checks;
pub mod ved01_tree;
pub mod ved02_checks;
pub mod ved02_xform;
pub mod ved03_alpha;
pub mod ved03_checks;
pub mod ved04_checks;
pub mod ved04_clip;
pub mod ved05_checks;
pub mod ved05_isolation;
pub mod ved06_checks;
pub mod ved06_zorder;
pub mod ved07_checks;
pub mod ved07_visibility;
pub mod ved13_dirty;
pub mod ved14_traverse;
pub mod ved15_cache;
pub mod ved16_scale;
pub mod ved17_consistency;
pub mod ved18_checks;
pub mod ved18_debugview;
pub mod ved19_surface;
pub mod ved20_closeout;
pub mod ved21_blendreg;
pub mod ved22_separable;

pub mod vea20_stencil;
pub mod vea21_raster;
pub mod vea22_vlayout;
pub mod vea23_psocache;
pub mod vea24_hotreload;
pub mod vea25_cbuf;
pub mod vea26_desc_heap;
pub mod vea27_bindlayout;
pub mod veb11_checks;
pub mod veb11_irq;
pub mod veb12_checks;
pub mod veb12_recovery;
pub mod veb13_checks;
pub mod veb13_heads;
pub mod veb14_checks;
pub mod veb14_perf;
pub mod veb15_checks;
pub mod veb15_suite;
pub mod veh03_checks;
pub mod veh03_mixgraph;
pub mod veh04_checks;
pub mod veh04_sendsidechain;
pub mod veh05_checks;
pub mod veh05_submix;
pub mod veh06_audiotoken;
pub mod veh06_checks;
pub mod veh07_checks;
pub mod veh07_fade;
pub mod veh09_checks;
pub mod veh09_spatial;
pub mod veh10_checks;
pub mod veh10_occlusion;
pub mod vee01_arch;
pub mod vee01_checks;
pub mod vee02_checks;
pub mod vee02_utf8;
pub mod vee03_checks;
pub mod vee03_outline;
pub mod vee04_checks;
pub mod vee04_raster;
pub mod vee05_hinting;
pub mod vee06_atlas;
pub mod vee07_prims;
pub mod vef01_checks;
pub mod vef01_pngdec;
pub mod vef02_checks;
pub mod vef02_pngenc;
pub mod vef03_adam7;
pub mod vef03_checks;
pub mod vef04_checks;
pub mod vef04_color;
pub mod veg03_checks;
pub mod veg03_webm_mkv;
pub mod veg04_checks;
pub mod veg04_h264;
pub mod veh01_boundary;
pub mod vei02_checks;
pub mod vei02_locale;
pub mod vei03_checks;
pub mod vei03_text_direction;
pub mod vei04_checks;
pub mod vei04_typeset;
pub mod vei05_checks;
pub mod vei05_font;
pub mod vei06_checks;
pub mod vei06_datetime;
pub mod vei07_checks;
pub mod vei07_plural;
pub mod veh01_checks;
pub mod veh02_audioarch;
pub mod veh02_checks;
// veh02_service.rs 为越权重复施工的孤儿文件（F1402 认领人 AI-ZCode-1，
// 落位 veh02_audioarch/veh02_checks）——其 run_veh02_checks 与在册实现
// 符号冲突，故不声明；文件保留待其作者自行清理。
pub mod vej04_checks;
pub mod vej04_pointlight;
pub mod vej05_checks;
pub mod vej05_spotlight;
pub mod vej06_area;
pub mod vej06_checks;
pub mod vej07_checks;
pub mod vej07_lightmgr;
pub mod vej08_probe;
pub mod vej08_checks;
pub mod vej09_ibl;
pub mod vej09_checks;
pub mod vek04_bloom;
pub mod vek04_checks;
pub mod vek05_params;
pub mod vek05_checks;
pub mod vek06_tonemap;
pub mod vek06_checks;
pub mod vek07_checks;
pub mod vek07_exposure;
pub mod vek08_colorspace;
pub mod vek08_checks;
pub mod vek09_msaa;
pub mod vek09_checks;
pub mod vek10_fxaa;
pub mod vek10_checks;
pub mod vem02_checks;
pub mod vem02_track;
pub mod vem03_checks;
pub mod vem03_interp;
pub mod vem04_batch;
pub mod vem04_checks;
pub mod vem05_asset;
pub mod vem05_checks;
pub mod vem06_event;
pub mod vem06_checks;
pub mod vem07_perf;
pub mod vem07_checks;
pub mod ven02_tree;
pub mod ven03_ctype;
pub mod ven03_checks;
pub mod ven04_checks;
pub mod ven04_prop;
pub mod ven05_checks;
pub mod ven05_dual;
pub mod ven06_checks;
pub mod ven06_incr;
pub mod vel03_checks;
pub mod vel03_emitter;
pub mod vel04_checks;
pub mod vel04_mode;
pub mod vel05_checks;
pub mod vel05_lifetime;
pub mod vel06_checks;
pub mod vel06_render;
pub mod veo01_arch;
pub mod veo01_checks;
pub mod veo02_vendor;
pub mod veo02_vendor_checks;
pub mod veo03_subset;
pub mod veo03_subset_checks;
pub mod vep01_arch;
pub mod vep01_checks;
pub mod vep02_checks;
pub mod vep02_lang;
pub mod vep03_checks;
pub mod vep03_token;
pub mod vep04_checks;
pub mod vep04_stack;
pub mod vep05_checks;
pub mod vep05_orch;
pub mod veq01_checks;
pub mod veq01_pipeline;
pub mod veq02_checks;
pub mod veq02_graph;
pub mod ver01_arch;
pub mod ver01_checks;
pub mod ver01b_checks;
pub mod ver01b_parser;
pub mod ver01c_cascade;
pub mod ver01c_checks;
pub mod ver01d_checks;
pub mod ver01d_switch;
pub mod ver02_arch;
pub mod ver02_checks;
pub mod ver03_arch;
pub mod ver03_checks;
pub mod ver04_arch;
pub mod ver04_checks;
pub mod ves01_sdomain_arch;
pub mod vet01_a11y_render_pipeline;
pub mod vet02_highcontrast_engine;
pub mod veu01_arch;
pub mod veu01_checks;
pub mod veu02_checks;
pub mod veu02_model;
pub mod veu03_checks;
pub mod veu03_registry;
pub mod veu04_checks;
pub mod veu04_engine;
pub mod veu08_checks;
pub mod veu08_density;
pub mod veu09_checks;
pub mod veu09_ime;
pub mod vev01_arch;
pub mod vev01_checks;
pub mod vew01_sdk_arch;
pub mod vew02_manifest;
pub mod vew03_loader;
pub mod vew04_sandbox;
pub mod vew05_trust;
pub mod ves04_flow;
pub mod ves04_checks;

pub use vea01_index::{ProbeReport, effective_renderer, run_a01};

use crate::checks::CheckSet;

/// 域标识（CheckSet 聚合用）。
pub const VEA_DOMAIN: &str = "svstar2-ve";

/// 本域自检聚合：逐项 `run_*_checks` 汇总。
///
/// CheckSet 容量上限见 `crate::checks::MAX_CHECKS`；单模块超限由该模块
/// 自身裁剪——聚合器如实报告每份 Set 的截断态。
pub fn run_svstar2_checks() -> CheckSet {
    let mut set = CheckSet::new(VEA_DOMAIN);
    // (标签, 子集) —— 逐项加行，施工一项加一项
    let blocks: [(&'static str, CheckSet); 175] = [
        ("VE-F0001", vea01_index::run_vea01_checks()),
        ("VE-F0002", vea02_ctx::run_vea02_checks()),
        ("VE-F0003", vea03_checks::run_vea03_checks()),
        ("VE-F0004", vea04_checks::run_vea04_checks()),
        ("VE-F0005", vea05_checks::run_vea05_checks()),
        ("VE-F0006", vea06_checks::run_vea06_checks()),
        ("VE-F0007", vea07_checks::run_vea07_checks()),
        ("VE-F0008", vea08_checks::run_vea08_checks()),
        ("VE-F0009", vea09_recovery::run_vea09_checks()),
        ("VE-F0010", vea10_bus::run_vea10_checks()),
        ("VE-F0011", vea11_hotplug::run_vea11_checks()),
        ("VE-F0012", vea12_probe::run_vea12_checks()),
        ("VE-F0013", vea13_softfall::run_vea13_checks()),
        ("VE-F0014", vea14_snapshot::run_vea14_checks()),
        ("VE-F0015", vea15_errclass::run_vea15_checks()),
        ("VE-F0018", vea18_checks::run_vea18_checks()),
        ("VE-F0019", vea19_checks::run_vea19_checks()),
        ("VE-F0201", veb01_checks::run_veb01_checks()),
        ("VE-F0202", veb02_checks::run_veb02_checks()),
        ("VE-F0203", veb03_checks::run_veb03_checks()),
        ("VE-F0204", veb04_checks::run_veb04_checks()),
        ("VE-F0210", veb10_cursor::run_veb10_checks()),
        ("VE-F0401", vec01_checks::run_vec01_checks()),
        ("VE-F0421", vec21_checks::run_vec21_checks()),
        ("VE-F0402", vec02_spec::run_vec02_checks()),
        ("VE-F0403", vec03_lexer::run_vec03_checks()),
        ("VE-F0404", vec04_keywords::run_vec04_checks()),
        ("VE-F0405", vec05_checks::run_vec05_checks()),
        ("VE-F0406", vec06_checks::run_vec06_checks()),
        ("VE-F0407", vec07_string::run_vec07_checks()),
        ("VE-F0408", vec08_comment::run_vec08_checks()),
        ("VE-F0409", vec09_operator::run_vec09_checks()),
        ("VE-F0410", vec10_brace::run_vec10_checks()),
        ("VE-F0411", vec11_prepro::run_vec11_checks()),
        ("VE-F0412", vec12_macro::run_vec12_checks()),
        ("VE-F0414", vec14_include::run_vec14_checks()),
        ("VE-F0415", vec15_checks::run_vec15_checks()),
        ("VE-F0416", vec16_checks::run_vec16_checks()),
        ("VE-F0417", vec17_checks::run_vec17_checks()),
        ("VE-F0418", vec18_checks::run_vec18_checks()),
        ("VE-F0419", vec19_checks::run_vec19_checks()),
("VE-F0420", vec20_checks::run_vec20_checks()),
        ("VE-F0601", ved01_checks::run_ved01_checks()),
        ("VE-F0602", ved02_checks::run_ved02_checks()),
        ("VE-F0603", ved03_checks::run_ved03_checks()),
        ("VE-F0613", ved13_dirty::run_ved13_checks()),
        ("VE-F4601", vew01_sdk_arch::run_vew01_checks()),
        ("VE-F4602", vew02_manifest::run_vew02_checks()),
        ("VE-F4603", vew03_loader::run_vew03_checks()),
        ("VE-F4604", vew04_sandbox::run_vew04_checks()),
        ("VE-F4605", vew05_trust::run_vew05_checks()),
        ("VE-F0614", ved14_traverse::run_ved14_checks()),
        ("VE-F0615", ved15_cache::run_ved15_checks()),
        ("VE-F0616", ved16_scale::run_ved16_checks()),
        ("VE-F0617", ved17_consistency::run_ved17_checks()),
        ("VE-F0618", ved18_checks::run_ved18_checks()),
        ("VE-F0621", ved21_blendreg::run_ved21_checks()),
        ("VE-F0622", ved22_separable::run_ved22_checks()),

        ("VE-F0020", vea20_stencil::run_vea20_checks()),
        ("VE-F0021", vea21_raster::run_vea21_checks()),
("VE-F0022", vea22_vlayout::run_vea22_checks()),
        ("VE-F0023", vea23_psocache::run_vea23_checks()),
        ("VE-F0024", vea24_hotreload::run_vea24_checks()),
        ("VE-F0025", vea25_cbuf::run_vea25_checks()),
        ("VE-F0026", vea26_desc_heap::run_vea26_checks()),
        ("VE-F0027", vea27_bindlayout::run_vea27_checks()),
("VE-F0211", veb11_irq::run_veb11_checks()),
("VE-F0212", veb12_recovery::run_veb12_checks()),
("VE-F0213", veb13_heads::run_veb13_checks()),
("VE-F0214", veb14_perf::run_veb14_checks()),
("VE-F0215", veb15_checks::run_veb15_checks()),
("VE-F0205", veb05_checks::run_veb05_checks()),
("VE-F0206", veb06_checks::run_veb06_checks()),
("VE-F0413", vec13_checks::run_vec13_checks()),
("VE-F0604", ved04_checks::run_ved04_checks()),
("VE-F0605", ved05_checks::run_ved05_checks()),
("VE-F0606", ved06_checks::run_ved06_checks()),
("VE-F0607", ved07_checks::run_ved07_checks()),
("VE-F1401", veh01_checks::run_veh01_checks()),
("VE-F1402", veh02_checks::run_veh02_checks()),
("VE-F1403", veh03_checks::run_veh03_checks()),
("VE-F1404", veh04_checks::run_veh04_checks()),
("VE-F1405", veh05_checks::run_veh05_checks()),
("VE-F1407", veh07_checks::run_veh07_checks()),
("VE-F1406", veh06_checks::run_veh06_checks()),
("VE-F1409", veh09_checks::run_veh09_checks()),
("VE-F1410", veh10_checks::run_veh10_checks()),
("VE-F1804", vej04_checks::run_vej04_checks()),
        ("VE-F1805", vej05_spotlight::run_vej05_checks()),
        ("VE-F1806", vej06_area::run_vej06_checks()),
        ("VE-J/F1807", vej07_checks::run_vej07_checks()),
("VE-J/F1808", vej08_checks::run_vej08_checks()),
        ("VE-J/F1809-a", vej09_checks::run_vej09_checks_a()),
        ("VE-J/F1809-b", vej09_checks::run_vej09_checks_b()),
("VE-F2801", veo01_checks::run_veo01_checks()),
        ("VE-F2802", veo02_vendor_checks::run_veo02_checks()),
        ("VE-F2803", veo03_subset_checks::run_veo03_checks()),
        ("VE-F0801", vee01_checks::run_vee01_checks()),
        ("VE-F0802", vee02_checks::run_vee02_checks()),
        ("VE-F0803", vee03_checks::run_vee03_checks()),
        ("VE-F0804", vee04_checks::run_vee04_checks()),
        ("VE-F0805", vee05_hinting::run_vee05_checks()),
        ("VE-F0806", vee06_atlas::run_vee06_checks()),
        ("VE-F0807", vee07_prims::run_vee07_checks()),
        ("VE-F2004", vek04_checks::run_vek04_checks()),
        ("VE-F2005", vek05_checks::run_vek05_checks()),
        ("VE-F2006", vek06_checks::run_vek06_checks()),
        ("VE-F2007", vek07_checks::run_vek07_checks()),
        ("VE-F2008", vek08_checks::run_vek08_checks()),
        ("VE-F2009", vek09_checks::run_vek09_checks()),
        ("VE-F2009-deep", vek09_checks::run_vek09_deep_checks()),
        ("VE-F2010", vek10_checks::run_vek10_checks()),
        ("VE-F2402", vem02_checks::run_vem02_checks()),
        ("VE-F2403", vem03_checks::run_vem03_checks()),
        ("VE-F2404", vem04_checks::run_vem04_checks()),
        ("VE-F2405", vem05_checks::run_vem05_checks()),
        ("VE-F2406", vem06_checks::run_vem06_checks()),
        ("VE-F2407-a", vem07_checks::run_vem07_checks_a_standalone()),
        ("VE-F2407-b", vem07_checks::run_vem07_checks_b_standalone()),
        ("VE-F2407-c", vem07_checks::run_vem07_checks_c_standalone()),
        ("VE-F2203", vel03_checks::run_vel03_all_checks()),
        ("VE-F2204", vel04_checks::run_vel04_all_checks()),
        ("VE-F2205", vel05_checks::run_vel05_all_checks()),
        ("VE-F2206", vel06_checks::run_vel06_all_checks()),
        ("VE-F3401", ver01_arch::run_ver01_checks()),
("VE-F3402", ver01b_checks::run_ver01b_checks()),
        ("VE-F3403", ver01c_checks::run_ver01c_checks()),
        ("VE-F3404", ver01d_checks::run_ver01d_checks()),
        ("VE-F3404-deep", ver01d_checks::run_ver01d_deep_checks()),
        ("VE-F3601", ver02_arch::run_ver02_checks()),
        ("VE-F3602", ver03_arch::run_ver03_checks()),
        ("VE-F3603", ver04_arch::run_ver04_checks()),
        ("VE-F3801", ves01_sdomain_arch::run_f3801_checks()),
        ("VE-F3802", vet01_a11y_render_pipeline::run_f3802_checks()),
        ("VE-F3803", vet02_highcontrast_engine::run_f3803_checks()),
        ("VE-F4201", veu01_checks::run_veu01_checks()),
        ("VE-F4202", veu02_checks::run_veu02_checks()),
        ("VE-F4203", veu03_checks::run_veu03_checks()),
        ("VE-F4204", veu04_checks::run_veu04_checks()),
        ("VE-F4008", veu08_checks::run_veu08_checks()),
        ("VE-F4009", veu09_checks::run_veu09_checks()),
        ("VE-F4401", vev01_checks::run_vev01_checks()),
        ("VE-F3201", veq01_pipeline::run_veq01_checks()),
        ("VE-F3202", veq02_graph::run_veq02_checks()),
        ("VE-F3001", vep01_checks::run_vep01_checks()),
        ("VE-F3002-a", vep02_checks::run_vep02_checks_a()),
        ("VE-F3002-b", vep02_checks::run_vep02_checks_b()),
        ("VE-F3003-a", vep03_checks::run_vep03_checks_a()),
        ("VE-F3003-b", vep03_checks::run_vep03_checks_b()),
        ("VE-F3004-a", vep04_checks::run_vep04_checks_a()),
        ("VE-F3004-b", vep04_checks::run_vep04_checks_b()),
        ("VE-F3005-a", vep05_checks::run_vep05_checks_a()),
        ("VE-F3005-b", vep05_checks::run_vep05_checks_b()),
        ("VE-F4002", vei02_checks::run_vei02_checks()),
        ("VE-F4003", vei03_checks::run_vei03_checks()),
        ("VE-F4004", vei04_checks::run_vei04_checks()),
        ("VE-F4005", vei05_checks::run_vei05_checks()),
        ("VE-F4006", vei06_checks::run_vei06_checks()),
        ("VE-F4007", vei07_checks::run_vei07_checks()),
        ("VE-F1001", vef01_checks::run_vef01_checks()),
        ("VE-F1002", vef02_checks::run_vef02_checks()),
        ("VE-F1003", vef03_checks::run_vef03_checks()),
        ("VE-F1004", vef04_checks::run_vef04_checks()),
        ("VE-F1203", veg03_checks::run_veg03_checks()),
        ("VE-F1204-a", veg04_checks::run_veg04_checks_a()),
        ("VE-F1204-b", veg04_checks::run_veg04_checks_b()),
        ("VE-F2602", ven03_checks::run_ven02_checks()),
        ("VE-F2603-a", ven03_checks::run_ven03_checks_a()),
        ("VE-F2603-b", ven03_checks::run_ven03_checks_b()),
        ("VE-F2604-a", ven04_checks::run_ven04_checks_a()),
        ("VE-F2604-b", ven04_checks::run_ven04_checks_b()),
        ("VE-F2605-a", ven05_checks::run_ven05_checks_a()),
        ("VE-F2605-b", ven05_checks::run_ven05_checks_b()),
        ("VE-F2606-a", ven06_checks::run_ven06_checks_a()),
        ("VE-F2606-b", ven06_checks::run_ven06_checks_b()),
        ("VE-F3604-a", ves04_checks::run_ves04_checks_a()),
        ("VE-F3604-b", ves04_checks::run_ves04_checks_b()),
    ];
    for (tag, sub) in blocks.iter() {
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            tag,
            passed,
            if passed { "" } else { "sub-checks red" },
        );
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svstar2_domain_aggregate_all_green() {
        let set = run_svstar2_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "svstar2 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    /// 落位纪律自检：VE 册功能必须在 Rust 侧，不许混进前端 TS。
    /// 这条断言是"位置写错"的最后一道闸——防止后人又把它写回 `src/`。
    #[test]
    fn vea_lives_in_kernel_not_frontend() {
        // VE-F0001 的类型定义在本模块树内可见，且由内核 crate 编译
        let probe = vea01_probe::AdapterProbeInput::physical(
            "0000:01:00.0",
            "NVIDIA GeForce RTX 4060",
            "551.23",
        );
        let r = vea01_engine::probe_all(&[probe]);
        assert_eq!(r.len(), 1, "VE-F0001 由内核 Rust 侧实现并可执行");
    }
}
