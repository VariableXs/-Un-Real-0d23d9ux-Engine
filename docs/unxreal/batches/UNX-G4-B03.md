# UNX-G4-B03 · 命令录制语义批：CommandList 指令翻译面 I（F26441–F26460 · 20 条）

> AI-34 承办（波 15 首产段立账 · 一次对话 300 项明令）｜批主题：命令录制语义批：CommandList 指令翻译面 I｜域账累计：18000 / 240,000｜判据主轴：vkd3d 测试集全绿账（豁免挂 ADR ≤5%）+ D3D12 samples 20/20 全链账 + 三面 Windows 对照差=0（J-测/J-样/J-根/J-栅/J-稳）｜嫁接源：vkd3d-proton 波14 基线版（钉定，波内禁升，升级全量重跑）＋D3D12 规范 promote/decay 实测锚（ADR-UNX-008）＋Wine/vkd3d 语义参考（只跟随）｜防重：全域 ID F26401–F27200 与已收口域零撞号（grep 五范围：kernel/varix/src、docs/START、_attic、已 finalize deepen 册、总纲既有段）｜上游：AI-32（Vulkan 设备面/WSI · 接口⑨消费方 · Schema 先行+fake 对接，B24 冻结签为前置）、AI-31（KMD 显存域/驻留事件）；横向 AI-33（DXGI 共享面与 hack 账基础设施 · B01 前联签，实现归属以 ID 段为准）；下游 AI-70（N5 收口消费样本账）｜红线声明：本批无引导设施红线与硬件数据安全红线触发条目（预申报为空）；DXR 档位声明禁虚标（非 RT 机不虚报不冒充）；豁免不入账=虚报（同 AI-32 纪律）；体验日志/异常显性化/交互词典三条间接纪律全程生效

### UNX-F26441 · 绘制指令族翻译（Draw/DrawInstanced/DrawIndexed）
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26441-J1 三绘制指令→vkCmdDraw 系映射黄金样本 30 组全过；参数越界（超顶点数）→校验拒止或 GPU 裁剪行为与 Windows 一致；空绘制 no-op 断言
### UNX-F26442 · Dispatch 计算指令族与组数语义
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26442-J1 Dispatch(组数)→vkCmdDispatch 映射断言；组数为零 no-op；超设备上限拒止错误码一致
### UNX-F26443 · Copy/CopyStructure/CopyTextureRegion 资源拷贝族
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26443-J1 buffer↔buffer/buffer↔texture 拷贝黄金样本 40 组全过；格式不匹配拒止矩阵；子资源区域换算（subresource 索引）与规范逐项一致
### UNX-F26444 · CopyTiles 与 tile 语义预告位（B12 深水）
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26444-J1 CopyTiles 地基翻译断言（packed/linear 两种布局各 10 剧本）；深水位预告登记；swizzle 模式如实记账（硬件不支持→受限登记不冒充）
### UNX-F26445 · ResolveSubresource 与 MSAA 解算
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26445-J1 Resolve 翻译 vkCmdResolveImage 黄金 20 组全过；格式约束校验（同 typeless 族）；MSAA 采样数不支持→显性错误
### UNX-F26446 · Clear 指令族（ClearRTV/DSV/UAV 四语义）
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26446-J1 四 Clear→vkCmdClear 系映射全过；只清指定视图（attachment 粒度）断言；全屏矩形模式与 FastClear 路径行为一致断言
### UNX-F26447 · 资源屏障指令地基（SetResourceTransitionBarrier）
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26447-J1 transition barrier 翻译→vkCmdPipelineBarrier 黄金 20 组全过；非法状态转移（如 PRESENT→COPY_DEST 未过 COMMON）校验拒止与 Windows 一致；深水归 B13–B15
### UNX-F26448 · UAV 屏障与全局/原子同步语义
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26448-J1 全局 UAV barrier 翻译断言；原子计数（AtomicOp）后 UAV 可见性黄金剧本 20 组；冗余屏障合并预告位（性能命门登记，实现归 B15）
### UNX-F26449 · 查询指令族（Begin/EndQuery/ResolveQueryData）
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26449-J1 时间戳/OCCLUSION/统计三查询类型全链（begin→end→resolve）黄金 30 组全过；堆内偏移对齐校验断言
### UNX-F26450 · 间接指令族（ExecuteIndirect 四参数语义）
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26450-J1 ExecuteIndirect→vkCmdDrawIndirectByteCountEXT 类映射黄金 20 组全过；count buffer 前缀和/arg buffer 布局校验；设备不支持→如实降级记账
### UNX-F26451 · 指令校验层地基（翻译前 D3D12 侧预校验）
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26451-J1 校验层拦截非法指令矩阵 20 格（错误资源/错误阶段/错误槽位）全过；校验开销 P95 <5% 实测；DEBUG 层关时零开销探针
### UNX-F26452 · 根参数绑定指令地基（SetGraphicsRoot* 族骨架）
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26452-J1 SetRootConstants/SetRootCBV/SetRootTable/SetRootDescriptor 四族指令登记翻译断言；槽位越界拒止；深水归 B05–B06
### UNX-F26453 · 视口与剪裁（RSSetViewports/ScissorRects 校验）
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26453-J1 视口/剪裁设置黄金 20 组全过；数量 0 合法（禁用语义）断言；空矩形/负值边界行为与 Windows 一致
### UNX-F26454 · 混合因子与目标写掩码运行期覆盖（OMSetBlendFactor 等）
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26454-J1 blend factor/stencil ref/primitive topology 运行期设置断言；与 PSO 静态态冲突裁决（动态覆盖优先）与 Windows 一致
### UNX-F26455 · 索引/顶点缓冲绑定（IASetPrimitiveTopology 等）
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26455-J1 topology 全枚举（point/line/tri/patch 全族）映射断言；patch-list 无 tess PSO→校验拒止；绑定为空解绑语义断言
### UNX-F26456 · 流输出（SOSetTargets）地基
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26456-J1 SO targets 绑定翻译断言；与 tess/stream-out PSO 组合黄金 10 剧本；能力不支持→CheckFeatureSupport 如实返回
### UNX-F26457 · 录制指令 fuzz 与锁步回归面
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26457-J1 指令序列 fuzz 10^4（合法/非法混合）零崩溃；锁步测试器（F26432）回归 100 剧本全绿；种子库 ≥100 登记
### UNX-F26458 · 指令翻译性能基线（每指令翻译开销账）
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26458-J1 高频指令（draw/copy/barrier）翻译开销 P95 基线落账；回归阈值 ±10%；与 G3 DXVK 同口径对照列
### UNX-F26459 · 录制期错误三要素呈现全覆盖
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26459-J1 录制期可失败点全量盘点（≥25 处）逐处错误三要素呈现断言；裸错误码 grep 零命中留痕；体验结论字段（顺畅/报错）入日志
### UNX-F26460 · B03 收口：指令翻译面 I 全链对账
- 域/批：G4/B03｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F26460-J1 B03 段 10 剧本端到端（绘制/拷贝/查询/间接）全过；批内求和 6,000 守恒；未竟之账移交 B04
