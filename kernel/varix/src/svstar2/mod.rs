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
//! | [`veh11_reverb`] | F1411 混响区与环境声学（分区即氛围：预设四族具名+Custom 空槽，跨分区序关系单调不靠互异性证明；卷积+备用三态 FSM 带迟滞死区防抖，MAC/样本恒等于 IR 长度故预算表与采样率解耦；IR 校验取长度双端+采样率逐位（不静默重采样，相位错的混响极难定位），延迟走 uniform-partitioned 闭式 (K−1)·P/fs 而非 L/fs（后者差一个数量级）；过渡时长=clamp(距离/速度) 且速度趋零钳上界不发散，频率量走几何插值中点恰为 √(ab)；边界以签名兑现——渲染入口只接听者坐标，播放链想用混响得先改签名） | VE 册 #VE-F1411 |
//! | [`vek10_fxaa`] | F2010 抗锯齿四法之 FXAA（单 pass 全屏：亮度对比边缘检测→边缘方向→定向模糊，Console/FXAA 3.11 质量档；低/中/高三预设阈值+跨度+步数三轴皆不同，档位强度用 touched 集合包含关系 S(low)⊆S(mid)⊆S(high) 验证；输出取值集闭包 {中心,A,B} 逐位相等故闭包判据用 == 而非近似；序位守卫三违规带三要素；模糊代价以合成高频图案的高频能量比作可复算证据；双线性采样是正确性前提——最近邻会让 A/B 落回中心像素使 FXAA 退化为空操作而结构判据全绿；与 CAS 联动给警告而非静默关掉） | VE 册 #VE-F2010 |
//! | [`vek11_taa`] | F2011 抗锯齿四法之 TAA（历史帧累积+子像素抖动：Halton(2,3) 8 相位，两底数分别进位；原始包围盒约 0.8125×0.7778 像素，由 normalize_scale() 按实测反推归一系数使 x 方向恰好铺满 1.0 像素，系数不写死常数；校验管线 velocity reject + 深度/法线相似度 + 场景切换强制失效，失效一律**权重精确归零**而非打折——打折会让鬼影残留，故用 enum HistoryUse 两态表达让『打折后仍在用』无法书写；clipping 用 3×3 邻域 min/max AABB 且**先 clip 后混合**（顺序交换即色偏），方差 clipping 由邻域矩导出；降级显性：速度缓冲缺失退化为几何启发校验并产 DegradeRecord 告警，静默降级时记录为空、判据立刻转红；历史显存 16 字节/像素含深度（只算颜色会低估 1/3），超配额拒绝 TAA 并回退 FXAA 建议；与 F2009 MSAA 互斥且被丢弃方具名；闪烁证据可复算——权重跳变的亮度台阶实测为单步的 3.6 倍） | VE 册 #VE-F2011 |
//! | [`vea01`] | F0001 虚拟显卡探测仲裁器 | VE 册 #VE-F0001 |
//! | [`veb13_heads`] | F0213 virtio 多头与 EDID（逐输出独立使能与模式/EDID 三级注入优先级与非法拒载保默认/热增删幂等重算以最新 cfg 为准/布局表 O(输出数) 应用与版本化持久化/位置语义播报名） | VE 册 #VE-F0213 |
//! | [`veb14_perf`] | F0214 virtio 性能与诊断接口（按帧统计命令数/传输字节/队列深度与提交到完成延迟的对数分桶草图在线更新分位/诊断快照按需拉取不常驻无请求零开销/计数溢出饱和不回绕且可区分恰好等于上限与已饱和/打点缺失标记缺测不入分位不补零/快照请求并发串行化排队有界满则拒收不覆盖/计数入遥测总线受 F0096 预算治理按提交序确定性抽样） | VE 册 #VE-F0214 |
//! | [`veb15_suite`] | F0215 virtio 一致性测试套件（三层用例：协议层状态机序与特性协商矩阵/功能层资源创建导出与 2D 更新与多头切换回归/恢复层注入错误验 F0212 处置矩阵；命令流编码走黄金流比对且对齐 F0206 版本戳，版本不匹配判阻断而非降级、重建黄金流须显式人工确认；宿主侧 fast 档与 QEMU 批队列 full 档为**同一份清单的两个投影**、full 是全集 fast 是子集；缺特性用例判 N/A 三值枚举单列统计且不算失败不计入通过数、N/A 早退路径零建销不污染清理对账；清理断言为独立收尾步、创建数须恰等于本档真跑用例数且过度销毁判失败；失败输出含复现最小命令序列） | VE 册 #VE-F0215 |
//! | [`veb16_declaration`] | F0216 virtio 参考驱动宣告（本组实现即 virtio 家族参考实现，声明支持范围 QEMU 6.0 以上 + VIRTIO_F_VERSION_1 必需 + 2D/virgl/Venus 三通路与语义承诺，外部实现按本组语义对齐；变更走语义版本化且破坏性变更提前一版公告；宣告为可执行断言非文档——每条承诺带测试套件证据指针，缺证据即阻断发布；宣告与实现不符以测试结果为准修正；范围外请求明确拒绝并给具体越界项不模糊承诺；版本结构化三元组比对 O(1)，通路线编码显式映射且自洽） | VE 册 #VE-F0216 |
//! | [`veb17_suspend`] | F0216 virtio 热重置与 suspend/resume（suspend 冻结队列并保存设备态 cfg×队列×资源表，resume 按快照重建并重协商特性；快照带独立重算摘要，损坏即走全量重初始化而非先试着重建；重协商丢失必需特性判不可重建，资源表超容量走降级重建并通知；热重置走 F0212 reset 序但保留资源表语义，与快照重建走不同字段不可混用；重置超预算即兜底中断且兜底状态可观测；建销账本饱和不回绕、漏销与过度销毁均可检出；恢复过程用户可见提示可关闭） | VE 册 #VE-F0217 |
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
//! | [`veo04_lexer`] | F2804 CSS 词法器与分词管线（CSS Syntax L3 全 25 类 token；单遍线性无回溯，前瞻窗口 ≤3 且 peek 不消费；零拷贝靠 token 存 [start,end) 偏移 + 流侧切片，不存 &str 以免自引用；错误恢复点必带偏移区间并立案；上限四闸（源字节/token 数/单 token 字节/嵌套深度）越界即钳制 + 告警 + 立案） | VE 册 #VE-F2804 |
//! | [`veo04_lexer_checks`] | F2804 域自检（45 条判据，五族分述：架构声明 6 / 集成边界 4 / 解析子集 5 / 降级矩阵 8 / 性能 5 / 判据 17） | VE 册 #VE-F2804 |
//! | [`vew01_sdk_arch`] | F4601 W 域开工与插件 SDK 总架构（四层/双承诺/承接/层冻结） | VE 册 #VE-F4601 |
//! | [`veab01_audioarch`] | F5601 AB 域开工与音频总架构（四层架构/音频三律/AA域十件承接追补） | VE 册 #VE-F5601 |
//! | [`ved14_traverse`] | F0614 图层渲染遍历器 | VE 册 #VE-F0614 |
//! | [`ved15_cache`] | F0615 图层缓存策略 | VE 册 #VE-F0615 |
//! | [`ved16_scale`] | F0616 大层数性能（虚拟化与扁平化） | VE 册 #VE-F0616 |
//! | [`ved17_consistency`] | F0617 图层树一致性校验 | VE 册 #VE-F0617 |
//! | [`ved18_debugview`] | F0618 图层树调试可视化（四开关覆盖/独立调试通道/只读面板接口/懒加载展开预算/树文本转储省略契约/结构级隐私边界） | VE 册 #VE-F0618 |
//! | [`ved19_surface`] | F0619 图层树与表面协议对接（表面帧协议/相位机半帧不落盘/damage 双向回流/提交点契约/缓冲引用世代与所有权显性） | VE 册 #VE-F0619 |
//! | [`ved20_closeout`] | F0620 图层树组收口（十九件证据实探齐备核验/证据缺项阻断回补/台账指纹绑定的双签/移交授权独立于双签/一致性趋势劣化登记/回归回溯/经验包三契约机检谓词/无障碍核验行三态） | VE 册 #VE-F0620 |
//! | [`ved21_blendreg`] | F0621 混合模式规范实现总纲（声称24vs条款18差额逐格登记含未归因项/三路同公式以FormulaId全等判定/条款段号与行号逐条对账/四条横切纪律按族绑定且反装饰/抽样覆盖面位图显性/条目级N/A必带理由/歧义裁决留痕拒空条款号） | VE 册 #VE-F0621 |
//! | [`ved22_separable`] | F0622 可分离混合模式 12 种（公式单一来源IR三路物化：标量/SIMD宽通道/WGSL文本；dodge与burn除零取值方向相反分档携带无传参错配面；soft-light三段全实现含D分支与连续段夹逼探针；可分离性机器证明=通道置换等变+alpha签名分离；反假变体19/19捕获含4条初测漏网补强） | VE 册 #VE-F0622 |
//! | [`ved23_nonseparable`] | F0623 不可分离混合模式 4 种（ClipColor 两段正序正反可分辨；SatClip 三分支后两支输出同值故直接断枚举与台账；Lum 权重逐字核对 0.3/0.59/0.11 拒Rec.709；四路同源含独立f64 oracle；顺序契约以双alpha<1语料可测） | VE 册 #VE-F0623 |
//! | [`ved24_additive`] | F0624 附加混合族（plus-lighter 线性光加法 Co=min(Cs+Cb,1) 预乘 alpha 同步相加；线性前提双层拦截：LinearRgb/SrgbRgb 异类型无隐式转换 + 运行期四层空间断言且诊断码互不遮蔽；自实现 fpow 供 no_std 真内核 sRGB 传输；候选登记三态如实 plus-darker 仅草案；场景登记含高光叠加反例推荐 screen；加法≠源覆盖以 F0622 normal 同语料对拍；反假变体 9/9 捕获含 1 处实测弱门禁补强） | VE 册 #VE-F0624 |
//! | [`vea20_stencil`] | F0020 深度模板状态机 | VE 册 #VE-F0020 |
//! | [`vea21_raster`] | F0021 光栅化状态机 | VE 册 #VE-F0021 |
//! | [`vea22_vlayout`] | F0022 顶点输入布局描述器（声明式偏移推导/编译期签名闸门/对齐修正/规范形去重/A27 位置空间） | VE 册 #VE-F0022 |
//! | [`vea23_psocache`] | F0023 渲染管线对象缓存（四段缓存：键构造/查表命中/异步编译/LRU 淘汰；哈希只查表、等价由描述符全等判定；编译失败降级带原因与上限；命中率四桶归因；预热把卡顿挪到加载期；持久化只存键与就绪事实） | VE 册 #VE-F0023 |
//! | [`vea24_hotreload`] | F0024 着色器热重载协调器（四段流水：请求合并/重编译/管线重建/场景验证；坏码不落地靠 active+staged 双份槽位、验证通过才提升；回滚真有快照可退；冻结是状态位非线程锁；断点期间排队保最新） | VE 册 #VE-F0024 |
//! | [`vea25_cbuf`] | F0025 常量缓冲更新策略器（四档策略按作用域与时机两轴分；成本模型=字节×每帧次数且不摊一次性切换成本；脏标记精确到槽位；漂移双向校准且假净优先并记动作序；失配建议落到具体档位附代价对比） | VE 册 #VE-F0025 |
//! | [`vea26_desc_heap`] | F0026 描述符堆与绑定模型（堆三段：分配/分片/复用；跨 D3D12/Vulkan/Metal 抽象，差异只在绑定类别数/槽位编号规则/一帧绑定次数三处，绑定语义三家共用同一来源；耗尽防护给可行动原因；分帧回收有预算且在飞描述符一律不收） | VE 册 #VE-F0026 |
//! | [`vea27_bindlayout`] | F0027 绑定布局编译器（离线编译：反射自动生成+手写布局双源交叉校验，不一致以反射为准并报差异；槽位重复/越界/断裂三类冲突，重复指名两处下标；产物缓存供A23 PSO 缓存键使用，指纹只查表、等价靠全等复核、撞而内容不等判损坏重编；反射部分失败阻断不退手动；超编译预算不阻断产出只报并归因最慢一步） | VE 册 #VE-F0027 |
//! | [`vea28_querypool`] | F0028 查询池管理（GPU 查询池化：遮挡/时间戳/流水线统计三类分池不混用，跨类访问以专属错误码报出；结果延迟取回不阻塞同步点，未就绪留待下帧而非返回占位值；溢出防护用饱和+粘滞标志绝不回绕；池化复用带代号推进防 ABA，重试超限强制归还槽位不泄漏） | VE 册 #VE-F0028 |
//! | [`vea29_indirect`] | F0029 间接绘制命令生成器（GPU 侧命令生成：预检→变体分配→预分配→定长 32 字节显式小端写入，零 unsafe 零 transmute，枚举走显式 wire() 编码0x51/0x52/0x53 与判别值刻意不同；边界预检九种专属错误码，越界一律在写缓冲之前拦截；变体表存量化前的形状源故粒度变粗后可重算去重，爆炸时按粒度递进合并而非删表或拒绝，变体号始终稠密无洞；CPU 回退保留种类与原始偏移且条数与拦截数独立对账；调试回放逐行给字节区间与解码字段，未知线编码显式渲染不兜底；读屏只报聚合计数不泄漏单条命令参数） | VE 册 #VE-F0029 |
//! | [`vea30_batcher`] | F0030 多绘制合批器（同材质同管线的绘制合并为实例化/批渲染；批键含种类与管线状态与材质三维，缺一即错合，全零键构造期判无效；收益成式实算——省下的驱动调用数减每批状态绑定与实例缓冲更新开销，净收益为负即放弃合批；打断归因分材质切换/管线切换/拓扑不兼容三类专属码，容量类单列不污染内容打断率，材质与管线同时不同时归因材质（主序即直接原因）；动态实例照合不误——合批改的是驱动调用次数而非实例数据可变性，动态性不进批键也不构成打断原因；放弃的批不丢弃，其绘制逐条标原路输出且条数与被放弃批内绘制数绝对对账；高打断率才出资产建议且逐实际发生的原因一一对应，材质建议合并材质、管线建议拆分状态、拓扑建议重排绘制序；读屏面板六行双语只报聚合计数不泄漏单条绘制参数） | VE 册 #VE-F0030 |
//! | [`vea31_framegraph`] | F0031 帧图资源屏障自动插入（按资源访问序列两两定关系自动推理屏障：写后读挡可见性、写后写挡顺序覆写、读后写挡读未取完被覆写，读后读唯一不需挡；屏障只跨节点插入——节点内先后访问由自身指令序保证，对自己插屏障是自环空转；手写屏障为结构性禁令，API 恒返回 ManualBarrierForbidden 不留口子，因帧图已从资源边推出屏障、人工再写一份必然随改动腐化；环检测用迭代三色 DFS（no_std 下不用递归），有环即拒绝且不返回半成品屏障清单——强行按加入顺序出屏障等于给一份看起来能用的帧图；屏障容量溢出与兜底均登记告警不静默截断；冗余消除只合并相邻且同资源同类的屏障，跨资源跨类别与非相邻一律不合并（合并会丢粒度掩盖真实依赖）；热图视图按资源给访问数屏障数与密度；读屏面板六行双语只报聚合计数与环状态不泄漏资源绑定细节） | VE 册 #VE-F0031 |
//! | [`vea32_resstate`] | F0032 资源状态跟踪器（每资源三列状态：当前态/历史态环形缓冲/预期态，三列合成一个「当前」就丢掉了声明与事实的对照而那正是跟踪器的存在理由；状态机违例检测为阻断级且被拒不改进态——不存在的跳转意味着状态机已错，继续跑等于往错误状态上叠加操作；终态转出单独归因不与「不存在该边」混同；漂移与违例严格分离：预期态不符与外部路径改动走校准并留痕（校准即承认现实，预期对齐实际），零违例——混为一谈会让合法外部改动被当 bug 阻断或真 bug 被静默校准；跳转表显式建模为 allowed_next 函数，判据遍历 36 种组合证表与判定函数逐项一致；跟踪开销零帧预算声明：历史深度有硬上限，超预算即降采样并登记次数，降采样后已有历史立即裁剪否则内存没真降；孤儿检测按「已销毁且条目仍在」绝对值口径，destroy 只标终态不删条目否则把泄漏藏了；读屏面板六行双语只报聚合计数不泄漏资源句柄与尺寸） | VE 册 #VE-F0032 |
//! | [`vea33_alias`] | F0033 别名与堆复用仲裁（显存别名安全域分析：活跃区间用**半开**区间 [begin,end)，端点相接不算重叠——闭区间会把逐帧连续绘制的相邻对全判重叠，别名功能整体失效；两资源同时活跃一律阻断且不降级（内容互相踩踏不是性能问题），不提供强制别名口子，阻断的对不进收益表不省显存；别名键必含对齐量——只看尺寸会漏对齐，把8字节对齐的资源别名到256字节对齐的块上逻辑跑得通但每次访问可能未对齐命中；安全先于收益判定，重叠对即便净收益很高也阻断；净收益=分开字节减共用块减别名开销，≤0即不别名（不为别名而别名）；共用块取两侧较大者；省下字节只累加正收益行，与净值合计口径分离；仲裁结论附两侧区间证据可复核（只给bool的结论无法复核，而仲裁失误即显存踩踏是最难查的一类）；候选对超预算登记非常规结论且不混入常规三类计数；读屏面板六行双语只报聚合计数不泄漏单对资源尺寸与内容哈希） | VE 册 #VE-F0033 |
//! | [`vea34_transient`] | F0034 瞬态资源分配器（帧内生命周期分析用**半开**区间 [first_touch,release)，端点相接不算冲突——闭区间会把首尾相接的流水绘制全判冲突，瞬态分配器整体失效；瞬态与持久**物理分池**，持久块绝不经瞬态空闲链表取用（否则帧末回收会释放持久资源在用的内存）；票据同时记「声明池 want」与「实际池 pool」两个字段，槽位归还看实际池、跨帧泄漏检测看声明池——降级票据声明瞬态而落持久堆，按实际池判泄漏会让它永远逃过检测、持久占用逐帧累积到耗尽而告警显示 0；池满**降级转持久堆并告知**（正确性优先于性能，不让渲染失败），降级既可观测（序号可查）又字节可查（供 A03 记账）；跨帧泄漏**检出与强制回收是两个独立计数**——只检不收则泄漏累积、只收不检则契约破坏静默；P95 取最近秩 ceil(0.95n) 而非均值（均值被大量便宜请求稀释、掩盖尾部劣化），耗时账为环形窗口只留最近 256 笔；耗时劣化可归因到具体请求序号（序号与样本同槽存储，环形淘汰后靠调用方回填映射必然对不上）；非法请求（零字节/超单块上限）**先拒不占槽位**，否则白占的槽会把后续合法请求全推去降级；块尺寸向上取整到256 字节粒度；读屏面板七行双语只报聚合计数、不泄漏单请求字节与槽位数） | VE 册 #VE-F0034 |
//! | [`vez01_vfxarch`] | F5201 Z 域开工与特效库总架构（Z 域定位：让画面会呼吸会说话——特效是内容的表情；域本色：特效即内容，为内容服务不为炫技服务，丰盛不以帧率为代价；四子系统粒子/后处理/环境/转场**责任互斥**，各产一类独占效果，跨类产出即越界——越界不靠自觉，靠归属判据逐类核对；两翼=参数系统（同一特效在不同场景取不同值）+组合系统（多特效按什么次序叠加），两翼是四子系统**唯一**的形参来源与编排入口，绕过两翼直接取常量则两翼形同虚设；特效即内容落在可测承诺上：无语义声明不入册、越权炫技被评审拦截且拦截有专属错误码不与参数越界合并、每个子系统受帧预算硬约束超预算登记降级不静默截断；层冻结=只能加不能改且加须升版本，改语义先走 unfreeze，直接改即越权；承接 Y 域 F5193 十件移交包（脚本绑定/渲染协议/预算口径三承接面），缺件回溯到源头件号；层间对拍必须能发现注入的失配而非恒真通过） | VE 册 #VE-F5201 |
//! | [`vem02_track`] | F2402 关键帧轨道系统（六类轨道/容器多轨/绑定协议/单源扩展） | VE 册 #VE-F2402 |
//! | [`vem03_interp`] | F2403 关键帧插值（四插值器/可插拔注册/确定性/贝塞尔纪律） | VE 册 #VE-F2403 |
//! | [`vem04_batch`] | F2404 关键帧批量操作（四操作/语义单源/单步撤销/原子事务） | VE 册 #VE-F2404 |
//! | [`vem05_asset`] | F2405 动画曲线资产（m.anim. 开放容器/生态单点/F1948 签名三件/F1956 版本三件/往返逐位零损失） | VE 册 #VE-F2405 |
//! | [`vem06_event`] | F2406 动画事件轨（事件总线第三生产者 M 域 + F2402 第七类离散轨 + 正播触发/倒播默认抑制语义表 + 同帧去抖取末值 + F1925/F2322 家族同构） | VE 册 #VE-F2406 |
//! | [`vem06_checks`] | F2406 域自检（判据逐条映射，40 项） | VE 册 #VE-F2406 |
//! | [`vem07_perf`] | F2407 动画求值性能（类型×时间轴指纹分批 + 零分配热路径 + 值/脏标记缓存 + LOD 先于精度降级次序） | VE 册 #VE-F2407 |
//! | [`vem07_checks`] | F2407 域自检（判据逐条映射，126 项分 a/b/c 三族） | VE 册 #VE-F2407 |
//! | [`vem08_debug`] | F2408 动画调试数据（曲线可视按需拾取+端点钉死LOD抽稀/ 定容环形值流 / 权重热力三冗余 + 三统计量口径唯一 + F1946 调优协议实测工作单元 + F1764 信封 M 段四类型漂移拦截 + 发行版剔除零成本） | VE 册 #VE-F2408 |
//! | [`vem08_checks`] | F2408 域自检（判据逐条映射，分 a/b/c 三族；剔除判据双向验证） | VE 册 #VE-F2408 |
//! | [`veb18_compat`] | F0218 QEMU 版本兼容矩阵（版本行×特性×预期值；五登记行升序 + 尾部回退 O(1) 查表 + 探测记录入诊断快照；未知版本按回退行预期并标未认证、矩阵与实测冲突以实测为准并留档待修、探测失败走全行交集保守预期并告警；下游 F0215 按矩阵跳 N/A、F0216 宣告引用最低支持版本） | VE 册 #VE-F0218 |
//! | [`veb18_checks`] | F0218 域自检（判据逐条映射，46 项分 a/b/c 三族；变异双向验证 23/23全捕获） | VE 册 #VE-F0218 |
//! | [`veb19_debugchan`] | F0219 virtio 主客联调通道（宿主注入逐帧日志开关/强制刷新/快照导出，客户事件回传；通道独立于渲染数据面渲染路径零指令且结构上不持有渲染状态引用；配额按**字节当量**而非条数（快照 4096 对日志开关 12，差两个数量级以上，按条限流会让贵消息挤掉便宜消息）；限流但**安全类消息永不被丢**（丢掉的是最后的证据）且旁路不占配额但单独计数；通道不通降级仅宿主侧并回执告知，**降级只改送到哪一侧不改命令合法性故校验排在其前**；非法命令拒绝并回执专属原因码（参数个数/参数值/参数语义/未知类型四类）；限流丢弃必留回执且回执环形缓冲有界；周期间配额账本与全程累计账本分离；线上编码显式映射不用 enum as u8） | VE 册 #VE-F0219 |
//! | [`ven02_tree`] | F2602 控件树模型（四要素/三不变量/三操作原子事务/M04 绑定路径解析；自 F2603 迁入的 Rust 权威实现） | VE 册 #VE-F2602 |
//! | [`ven03_ctype`] | F2603 控件类型体系（六类最小集/扩展三件套/类型注册制/内核-上层分层边界） | VE 册 #VE-F2603 |
//! | [`ven03_checks`] | F2602/F2603 域自检（判据逐条映射，55 项分三批落集） | VE 册 #VE-F2603 |
//! | [`ven04_prop`] | F2604 控件属性系统（四段管线/依赖属性继承+绑定/M04 属性侧兑现/零风暴纪律） | VE 册 #VE-F2604 |
//! | [`ven04_checks`] | F2604 域自检（判据逐条映射，56 项分两批落集） | VE 册 #VE-F2604 |
//! | [`ven05_dual`] | F2605 逻辑-可视双树与模板展开（双树分离/模板展开时机/单向数据流/三遍历/D-N 边界/同步断言/降级矩阵） | VE 册 #VE-F2605 |
//! | [`ven05_checks`] | F2605 域自检（判据逐条映射，56 项分两批落集） | VE 册 #VE-F2605 |
//! | [`ven06_incr`] | F2606 控件树增量更新（精确失效/帧边界批处理/三分发/双树增量同步；溢出强制提交+错路逐条审计；缺失目标绝不退化全树） | VE 册 #VE-F2606 |
//! | [`ven06_checks`] | F2606 域自检（判据逐条映射，70 项分两批落集；变异双向验证 22/22 全捕获——含三位全占拆分、脏掩码整条丢弃、缺失目标不退化全树、溢出强制提交正向计数四条，实测补自「两条拆分与三条拆分是不同形态」「纯脏掩码下守卫摘与不摘外部表现相同」「两个入口各写一份兜底」「负向断言不覆盖正向计数」四处弱门禁） | VE 册 #VE-F2606 |
//! | [`ver01_arch`] | F3401 令牌运行时架构（四件两律总纲） | VE 册 #VE-F3401 |
//! | [`ver01b_parser`] | F3402 令牌解析器（JSON/TOML 双格式 + 引用 DAG + 迭代 DFS 环检测 + 断链三要素） | VE 册 #VE-F3402 |
//! | [`ver01c_cascade`] | F3403 令牌依赖图与级联（依赖图可视化 + 批量合并级联 + 双深度闸 + 耗时画像） | VE 册 #VE-F3403 |
//! | [`ver01d_switch`] | F3404 主题切换事务（原子换肤：双缓冲单指针翻转 + 预演干跑 + 快照回滚 + 悬空兜底 + 截图一致性断言） | VE 册 #VE-F3404 |
//! | [`ver01e_typetree`] | F3405 令牌类型系统（六类封闭全集 + 声明先行校验 + px/rem/ms 显式单位 + 三级降级矩阵 + E17 报告） | VE 册 #VE-F3405 |
//! | [`ver01f_overlay`] | F3406 令牌覆盖层（四级栈默认→主题→场景→组件 + 读时刻优先级仲裁 + 越级不生效 + 来源审计 + E14 准入报告） | VE 册 #VE-F3406 |
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
//! | [`veq03_handle`] | F3203 资源句柄与生命周期（类型化句柄+五态弧表+计数与图双源对账+分代GC与误收P1红线） | VE 册 #VE-F3203 |
//! | [`veq04_type`] | F3204 资源类型系统（十类闭集×四要素登记+开放封闭扩展点+跨类型双拦截+两级语义+命名空间） | VE 册 #VE-F3204 |
//! | [`vee01_arch`] | F0801 文字渲染域总架构（四段单向流+ 三向兑现 + 1.5ms 预算） | VE 册 #VE-F0801 |
//! | [`vee02_utf8`] | F0802 字符编码与 UTF-8 解码（四档处置 + 偏移表 + 200MB/s） | VE 册 #VE-F0802 |
//! | [`vee03_outline`] | F0803 字形轮廓与贝塞尔（二次升三次 + 围向约定 + 1/64 量化） | VE 册 #VE-F0803 |
//! | [`vee07_prims`] | F0807 文本图元渲染（合批 2000 + 绘制≤2 + 四效果定长 uniform + 变换 + 批次键三要素各自参与归组 + 绘制超限必产告警且单批不误报；域自检 13 项） | VE 册 #VE-F0807 |
//! | [`vee08_fontmetric`] | F0808 字体度量（ascent/descent/行高取 **hhea 与 OS/2 双来源**并逐字段显性声明来源——只给最终值不给来源，「为什么这行比那行高」就不可查，而渲染与命中测试对不上正是这类 bug 最难查的表现；冲突超阈值一律**以 OS/2 为准并告警**，静默选来源会让同一字体在不同代码路径算出不同行高；冲突判定用**相对比例**（5%）而非绝对差——同一绝对差在小字体是巨变、大字体可忽略，绝对阈值会让小字体频繁误报、大字体漏报真冲突；度量表缺失（畸形字体）**退化为 em 推导并计数**，计数让「多少字体走了降级」可查，否则静默降级让排版悄悄变形无人知；行高三模式（字体默认推荐值 / 紧凑 1.0em / 宽松 1.3em）互不相同且 UI 可全局指定；度量按字体实例缓存，缓存键含字体 ID+字号+字重+字形档位四段各 32 位（**单射**，漏一个参数就会用错度量导致文本重叠或行距跳变），实例化参数变更即失效重算；缓存满时登记超限且**仍返回度量**（正确性优先，缓存满不能不让排版），且不挤掉已有条目；字距查询区分「无该字对(None)」与「调整恰为 0(Some(0))」——排版上二者不同；应用字距的 advance 双向夹取不溢出不回绕；**基线就是 ascent** 而非行高的一半（后者是常见错误近似）；查询命中均耗 0（锚点 ≤0.001ms）、全量快照 ≤0.5ms；读屏面板七行双语只报聚合计数不泄漏单字体度量值） | VE 册 #VE-F0808 |
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
//! | [`vef04_checks`] | F1004 域自检（判据逐条映射，22 项） | VE 册 #VE-F1004 |
//! | [`vef05_text`] | F1005 PNG 文本块族（tEXt Latin-1 单 NUL 分隔 / iTXt UTF-8 四段结构含压缩 / zTXt 压缩 Latin-1；元数据条目四元组 + 列表语义共存不覆盖 + 展示层转义非剥离 + 单条 2MB 与条数 500 双闸；iTXt 压缩体长度靠试探解压定界——因 zlib 流内部允许 NUL 字节，扫 NUL 定界法已实证失效） | VE 册 #VE-F1005 |
//! | [`vef05_checks`] | F1005 域自检（判据逐条映射，36 项；11 变体实测全部转红） | VE 册 #VE-F1005 |
//! | [`vef07_pngstream`] | F1007 PNG 流式解码（feed 增量接口/chunk 1B..1MB 无关性/行级回调与部分输出/签名→头→中间块→图像数据→结束状态机/中断恢复经块游标与 inflate 状态序列化/Adam7 逐遍渐进） | VE 册 #VE-F1007 |
//! | [`vef07_pngstream_checks`] | F1007 域自检（判据逐条映射，61 项；含自研 inflate 对上游 mech_inflate 的独立逐字节对拍 8 项） | VE 册 #VE-F1007 |
//! | [`vef07_stream`] | F1007 PNG 流式解码·第二实现（feed 增量接口按 IDAT 载荷前缀分批推进/位级可序列化 inflate 与 NeedMore 二分/符号级事务与匹配续解/反滤波参照行跨 feed 存活/驻留量 O(1) 口径/游标携成果与全状态跨中断/中止终态） | VE 册 #VE-F1007 |
//! | [`vef07_checks`] | F1007 域自检（判据逐条映射，43 项分 a/b/c 三族；基准取上游 vef01 全量解码，含上游 F1002 滤波方向缺陷取证） | VE 册 #VE-F1007 |
//! | [`vef54_aaarch`] | F5401 AA 域网络总架构（传输→会话→复制→玩法四层 + 层间接口逐条冻结不可解冻 + 层间失配只对拍不补偿 + Z 域移交包承接面三落点回溯绑源 + 带宽预算突发上界与帧预算双闸 + 公平判定与时钟负载无关） | VE 册 #VE-F5401 |
//! | [`vef54_checks`] | F5401 域自检（判据逐条映射，43 项分 a/b/c 三族；变异双向验证 15/17 捕获 + 2 项等价变异留痕） | VE 册 #VE-F5401 |
//! | [`veu01_arch`] | F4201 U 域开工与一致性总架构（五层+接口冻结+承接落地+双维入约） | VE 册 #VE-F4201 |
//! | [`veu02_model`] | F4202 跨域一致性模型（四类×三型+关系代数+环检测+红线+版本化） | VE 册 #VE-F4202 |
//! | [`veu03_registry`] | F4203 契约注册中心（四能力+五字段冻结+唯一性+引用计数+生命周期） | VE 册 #VE-F4203 |
//! | [`veu05_flow`] | F4205 契约变更流程引擎（六步流步步留痕含被拒与作废/兼容闸双「必须」拆位判定/回滚协议两失败形态可区分/会签缺挂起+催办累加/灰度档位与观测分离/弱化即破坏红线由内容推导不接受自报/F4203 注册单源不复制状态） | VE 册 #VE-F4205 |
//! | [`veu05_flow_checks`] | F4205 域自检（判据逐条映射：六步留痕/兼容闸/灰度/会签/弱化即破坏/判据） | VE 册 #VE-F4205 |
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
//! | [`vec21_parser`] | F0421 语法分析器架构（递归下降手写分析器选型留痕含被拒策略逐条理由 / 记号流前瞻窗口 k=2 不回扫由消费水位结构性保证并可变异体验证 / 解析只产生动作 AST 构建降为可替换消费者且回调拒绝被隔离不影响状态机 / 解析深度上限超限报嵌套源头而非当前位置 / 文法派生表 FIRST-FOLLOW 规范期拦截歧义且可前缀分解不误拦 / 诊断码锚点引用 F0420 移交两条可机检下游动作兑现） | VE 册 #VE-F0421 |
//! | [`vec21_checks`] | F0421 域自检（判据逐条映射，78 项：选型决策 14 / 前瞻窗口 10 / 动作分离 11 / 深度防护 13 / 锚点引用 8 / 文法冲突 10 / 性能 6 / 进度保底 6） | VE 册 #VE-F0421 |
//! | [`vec22_ast`] | F0422 AST 节点定义与 arena 池（节点类型全集按声明/语句/表达式三大族+ 根承载体分族且分族直接决定arena 分桶 / 同族节点连续布局由「单一Vec 桶 + 尾部 +1 bump」结构性保证并以连续段实测而非信任设计 / 无单节点释放入口使零碎片在API 层无可写代码且父指针是桶下标故整池释放后取节点走 Err 而非悬垂 / 节点版本登记簿与实现全集集合相等且版本号严格递增倒退即拒 / 骨架20 条产生式映射全覆盖未登记产生式显性计数不回退默认类型 / 跨度缺失按锚点降级标注而非阻断） | VE 册 #VE-F0422 |
//! | [`vec22_checks`] | F0422 域自检（判据逐条映射，64 项：三大族 10 / arena 连续 8 / 一次释放 7 / 版本登记 9 / 错误路径与边界 19 / 上游对接 8 / 门禁自洽 3；变异双向验证 14/15全捕获，未捕获项由编译期数组长度声明在类型层拦截） | VE 册 #VE-F0422 |
//! | [`ves04_flow`] | F3604 创作工作流引擎（DAG 契约复用 F3005+ 三预置流 + 断点续作 + 沙箱 + 驱动协议） | VE 册 #VE-F3604 |
//! | [`ves04_checks`] | F3604 域自检（判据逐条映射，55 项分两批落集） | VE 册 #VE-F3604 |
//! | [`vez50_arch`] | F5001 Y 域开工与场景图脚本总架构（四层设计→词法→语法→运行时，层间接口冻结；脚本为人写= 可读性优先于机巧，诊断须带源位置与人话建议；承接 X 域 F4993 十件移交包三承接面，缺件回溯源头件号；层间失配→对拍、接口越权→冻结流程） | VE 册 #VE-F5001 |
//! | [`vez50_checks`] | F5001 域自检（判据逐条映射，47 项分 a/b/c 三族；变异双向验证 30/30 全捕获） | VE 册 #VE-F5001 |
//! | [`vel06_render`] | F2206 粒子渲染接口（渲染形态抽象：billboard 面片/网格粒子/拖尾三型，形态恰为三且各有独立参数集；模拟→渲染单向解耦——渲染面收`&[ParticleView]` 只读消费故形态切换在类型上不可能改模拟，切换另走帧边界闸 F1762；速度拉伸保面积守恒 width=base/k 解析式非估值，相机对齐态不吃拉伸因子以守住朝向二态语义分界；I02 对接以 InstanceSink trait 声明签名使「未就绪挂起」可被构造与测试，容量按顶点/实例两口径各自对账、不足则拒绝并给错误三要素而非静默截断；拖尾定容环形历史缓冲溢出即覆盖并计覆盖数，环形语义退化为 FIFO 会静默丢新点故判据钉「保最新三点」；三条拒绝路径一律记袋，拒绝原因只走返回值等于「发生过但查不到」） | VE 册 #VE-F2206 |
//! | [`vel06_checks`] | F2206 域自检（判据逐条映射，61 项；变异双向验证 12/12 全捕获——含拖尾宽度轴垂直段向与环形读出时间序两条，实测补自「宽度距离量法对side 方向不敏感」与「cap=3 推 6 次游标恰回 0 使退化实现与正确实现数值重合」两处弱门禁） | VE 册 #VE-F2206 |

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
pub mod vec22_ast;
pub mod vec22_checks;
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
pub mod ved23_nonseparable;
pub mod ved24_additive;

pub mod vea20_stencil;
pub mod vea21_raster;
pub mod vea22_vlayout;
pub mod vea23_psocache;
pub mod vea24_hotreload;
pub mod vea25_cbuf;
pub mod vea26_desc_heap;
pub mod vea27_bindlayout;
pub mod vea28_querypool;
pub mod vea29_indirect;
pub mod vea30_batcher;
pub mod vea31_framegraph;
pub mod vea32_resstate;
pub mod vea33_alias;
pub mod vea34_transient;
pub mod vez01_vfxarch;
pub mod vez01_vfxarch_checks;
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
pub mod veb16_checks;
pub mod veb16_declaration;
pub mod veb17_checks;
pub mod veb17_suspend;
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
pub mod veh11_checks;
pub mod veh11_reverb;
pub mod veh12_checks;
pub mod veh12_latency;
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
pub mod vee08_fontmetric;
pub mod vef01_checks;
pub mod vef01_pngdec;
pub mod vef02_checks;
pub mod vef02_pngenc;
pub mod vef03_adam7;
pub mod vef03_checks;
pub mod vef04_checks;
pub mod vef04_color;
pub mod vef05_checks;
pub mod vef05_text;
pub mod vef07_pngstream;
pub mod vef07_pngstream_checks;
pub mod vef07_stream;
pub mod vef07_checks;
pub mod vef54_aaarch;
pub mod vef54_checks;
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
pub mod vek11_taa;
pub mod vek11_checks;
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
pub mod vem08_debug;
pub mod vem08_checks;
pub mod veb18_compat;
pub mod veb18_checks;
pub mod veb19_debugchan;
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
pub mod veo04_lexer;
pub mod veo04_lexer_checks;
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
pub mod veq03_checks;
pub mod veq04_checks;
pub mod veq04_type;
pub mod veq03_handle;
pub mod ver01_arch;
pub mod ver01_checks;
pub mod ver01b_checks;
pub mod ver01b_parser;
pub mod ver01c_cascade;
pub mod ver01c_checks;
pub mod ver01d_checks;
pub mod ver01d_switch;
pub mod ver01e_checks;
pub mod ver01e_typetree;
pub mod ver01f_checks;
pub mod ver01f_overlay;
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
pub mod veu05_flow;
pub mod veu05_flow_checks;
pub mod veu08_checks;
pub mod veu08_density;
pub mod veu09_checks;
pub mod veu09_ime;
pub mod vev01_arch;
pub mod vev01_checks;
pub mod vev02_checks;
pub mod vev02_monitor;
pub mod vew01_sdk_arch;
/// VE-AB · AB01 批次 · AB 域开工与音频总架构（VE-F5601）
///
/// 四层架构（设备/混音/空间/内容，层序即权限序，反向依赖即越权）；
/// 音频三律（不炸耳/可静音/诚实音量）一律为红线，静音掉不算合规；
/// AA 域（F5593）十件承接追补，缺件之层不得冻结接口。
pub mod veab01_audioarch;
/// VE-AB · AB01 批次 · 音频域总架构域自检（VE-F5601）
///
/// 判据侧独立重算四层/三律/十件的映射表（层序、短名、落层三列逐条钉死），
/// 合法依赖放行与越权拒绝双向验证，红线不可降级逐条断言。
pub mod veab01_audioarch_checks;
pub mod vew02_manifest;
pub mod vew03_loader;
pub mod vew04_sandbox;
pub mod vew05_trust;
pub mod ves04_flow;
pub mod ves04_checks;
pub mod vez50_arch;
pub mod vez50_checks;

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
    let blocks: [(&'static str, CheckSet); 180] = [
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
("VE-F0421", vec21_checks::run_vec21_checks()),
("VE-F0422", vec22_checks::run_vec22_checks()),
        ("VE-F0601", ved01_checks::run_ved01_checks()),
        ("VE-F0602", ved02_checks::run_ved02_checks()),
        ("VE-F0603", ved03_checks::run_ved03_checks()),
        ("VE-F0613", ved13_dirty::run_ved13_checks()),
        ("VE-F4601", vew01_sdk_arch::run_vew01_checks()),
        ("VE-F5601", veab01_audioarch_checks::run_veab01_checks()),
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
        ("VE-F0623", ved23_nonseparable::run_ved23_checks()),
("VE-F0624", ved24_additive::run_ved24_checks()),

        ("VE-F0020", vea20_stencil::run_vea20_checks()),
        ("VE-F0021", vea21_raster::run_vea21_checks()),
("VE-F0022", vea22_vlayout::run_vea22_checks()),
        ("VE-F0023", vea23_psocache::run_vea23_checks()),
        ("VE-F0024", vea24_hotreload::run_vea24_checks()),
        ("VE-F0025", vea25_cbuf::run_vea25_checks()),
        ("VE-F0026", vea26_desc_heap::run_vea26_checks()),
        ("VE-F0027", vea27_bindlayout::run_vea27_checks()),
        ("VE-F0028", vea28_querypool::run_vea28_checks()),
        ("VE-F0029", vea29_indirect::run_vea29_checks()),
        ("VE-F0030", vea30_batcher::run_vea30_checks()),
        ("VE-F0031", vea31_framegraph::run_vea31_checks()),
        ("VE-F0032", vea32_resstate::run_vea32_checks()),
        ("VE-F0033", vea33_alias::run_vea33_checks()),
        ("VE-F0034", vea34_transient::run_vea34_checks()),
        ("VE-F5201", vez01_vfxarch_checks::run_vez01_checks()),
("VE-F0211", veb11_irq::run_veb11_checks()),
("VE-F0212", veb12_recovery::run_veb12_checks()),
("VE-F0213", veb13_heads::run_veb13_checks()),
("VE-F0214", veb14_perf::run_veb14_checks()),
("VE-F0215", veb15_checks::run_veb15_checks()),
("VE-F0216", veb16_checks::run_veb16_checks()),
("VE-F0217", veb17_checks::run_veb17_checks()),
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
("VE-F1411", veh11_checks::run_veh11_checks()),
("VE-F1412", veh12_checks::run_veh12_checks()),
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
("VE-F2804", veo04_lexer_checks::run_veo04_checks()),
        ("VE-F0801", vee01_checks::run_vee01_checks()),
        ("VE-F0802", vee02_checks::run_vee02_checks()),
        ("VE-F0803", vee03_checks::run_vee03_checks()),
        ("VE-F0804", vee04_checks::run_vee04_checks()),
        ("VE-F0805", vee05_hinting::run_vee05_checks()),
        ("VE-F0806", vee06_atlas::run_vee06_checks()),
        ("VE-F0807", vee07_prims::run_vee07_checks()),
        ("VE-F0808", vee08_fontmetric::run_vee08_checks()),
        ("VE-F2004", vek04_checks::run_vek04_checks()),
        ("VE-F2005", vek05_checks::run_vek05_checks()),
        ("VE-F2006", vek06_checks::run_vek06_checks()),
        ("VE-F2007", vek07_checks::run_vek07_checks()),
        ("VE-F2008", vek08_checks::run_vek08_checks()),
        ("VE-F2009", vek09_checks::run_vek09_checks()),
        ("VE-F2009-deep", vek09_checks::run_vek09_deep_checks()),
        ("VE-F2010", vek10_checks::run_vek10_checks()),
        ("VE-F2011", vek11_checks::run_vek11_checks()),
        ("VE-F2402", vem02_checks::run_vem02_checks()),
        ("VE-F2403", vem03_checks::run_vem03_checks()),
        ("VE-F2404", vem04_checks::run_vem04_checks()),
        ("VE-F2405", vem05_checks::run_vem05_checks()),
        ("VE-F2406", vem06_checks::run_vem06_checks()),
        ("VE-F2407-a", vem07_checks::run_vem07_checks_a_standalone()),
        ("VE-F2407-b", vem07_checks::run_vem07_checks_b_standalone()),
        ("VE-F2407-c", vem07_checks::run_vem07_checks_c_standalone()),
        ("VE-F2408-a", vem08_checks::run_vem08_checks_a_standalone()),
        ("VE-F2408-b", vem08_checks::run_vem08_checks_b_standalone()),
        ("VE-F2408-c", vem08_checks::run_vem08_checks_c_standalone()),
        ("VE-F0218-a", veb18_checks::run_veb18_checks_a_standalone()),
        ("VE-F0218-b", veb18_checks::run_veb18_checks_b_standalone()),
        ("VE-F0218-c", veb18_checks::run_veb18_checks_c_standalone()),
        ("VE-F0219", veb19_debugchan::run_veb19_checks()),
        ("VE-F2203", vel03_checks::run_vel03_all_checks()),
        ("VE-F2204", vel04_checks::run_vel04_all_checks()),
        ("VE-F2205", vel05_checks::run_vel05_all_checks()),
        ("VE-F2206", vel06_checks::run_vel06_all_checks()),
        ("VE-F3401", ver01_arch::run_ver01_checks()),
("VE-F3402", ver01b_checks::run_ver01b_checks()),
        ("VE-F3403", ver01c_checks::run_ver01c_checks()),
        ("VE-F3404", ver01d_checks::run_ver01d_checks()),
        ("VE-F3404-deep", ver01d_checks::run_ver01d_deep_checks()),
        ("VE-F3405", ver01e_checks::run_ver01e_checks()),
        ("VE-F3405-deep", ver01e_checks::run_ver01e_deep_checks()),
        ("VE-F3405-equiv", ver01e_checks::run_ver01e_equivalence_checks()),
        ("VE-F3406", ver01f_checks::run_ver01f_checks()),
        ("VE-F3601", ver02_arch::run_ver02_checks()),
        ("VE-F3602", ver03_arch::run_ver03_checks()),
        ("VE-F3603", ver04_arch::run_ver04_checks()),
        ("VE-F5001-a", vez50_checks::run_vez50_checks_a_standalone()),
        ("VE-F5001-b", vez50_checks::run_vez50_checks_b_standalone()),
        ("VE-F5001-c", vez50_checks::run_vez50_checks_c_standalone()),
        ("VE-F3801", ves01_sdomain_arch::run_f3801_checks()),
        ("VE-F3802", vet01_a11y_render_pipeline::run_f3802_checks()),
        ("VE-F3803", vet02_highcontrast_engine::run_f3803_checks()),
        ("VE-F4201", veu01_checks::run_veu01_checks()),
        ("VE-F4202", veu02_checks::run_veu02_checks()),
        ("VE-F4203", veu03_checks::run_veu03_checks()),
        ("VE-F4204", veu04_checks::run_veu04_checks()),
        ("VE-F4205", veu05_flow_checks::run_veu05_flow_checks()),
        ("VE-F4008", veu08_checks::run_veu08_checks()),
        ("VE-F4009", veu09_checks::run_veu09_checks()),
        ("VE-F4401", vev01_checks::run_vev01_checks()),
        ("VE-F4402-a", vev02_checks::run_vev02_checks_a()),
        ("VE-F4402-b", vev02_checks::run_vev02_checks_b()),
        ("VE-F3201", veq01_pipeline::run_veq01_checks()),
        ("VE-F3202", veq02_graph::run_veq02_checks()),
        ("VE-F3203", veq03_handle::run_veq03_checks()),
        ("VE-F3204", veq04_type::run_veq04_checks()),
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
        ("VE-F1005", vef05_checks::run_vef05_checks()),
        ("VE-F1007", vef07_pngstream_checks::run_vef07_pngstream_checks()),
        ("VE-F1007-stream-a", vef07_checks::run_vef07_checks_a_standalone()),
        ("VE-F1007-stream-b", vef07_checks::run_vef07_checks_b_standalone()),
        ("VE-F1007-stream-c", vef07_checks::run_vef07_checks_c_standalone()),
        ("VE-F5401-a", vef54_checks::run_vef54_aaarch_checks_a_standalone()),
        ("VE-F5401-b", vef54_checks::run_vef54_aaarch_checks_b_standalone()),
        ("VE-F5401-c", vef54_checks::run_vef54_aaarch_checks_c_standalone()),
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
