# UNX-G2-B19 · Vulkan 管线与 descriptor 深化（F25161–F25180 · 20 条）

> AI-32 承办｜域账累计：B01–B19 骨架累计 114,000 / 240,000｜批型：G2 嫁接批（Mesa 跟随季度版，风险级高，兜底 lavapipe——任务书全嫁接档）｜嫁接源：Mesa（iris/crocus/llvmpipe/RADV/NVK/lavapipe）+ piglit + VK-GL-CTS 只跟随钉版，改动只落 winsys/适配器白名单层｜防重：全域五范围 grep 已执行（kernel/docs/VE/START/deepen）零 G2 ID 撞号｜红线：无引导设施红线与硬件数据安全红线触发条目；winsys 全部内核接触走 G1 冻结联签接口零旁路；真机判据一律登记 R-G2-002 随闸门补测（双轨产线零 QEMU 零实机写）｜批注（AI-32）：主题框架「VK 管线/descriptor」；上游 G1（AI-31）未收口按 Schema 先行 + fake 对接（R-G2-001）；软渲染标注四十六字诀「缺件如实标」全域生效

### UNX-F25161 · graphics pipeline cache 命中账
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25161-J1 管线缓存命中率实测 ≥80% 断言，冷建耗时 P95 登记
### UNX-F25162 · pipeline layout 与 descriptor set layout 校验面
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25162-J1 layout 不匹配全拒断言（fuzz 500 组），单一错误码断言
### UNX-F25163 · descriptor 更新语义（write/copy）
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25163-J1 descriptor 更新 10^4 次零悬空断言，更新中绘制安全断言
### UNX-F25164 · bindless（descriptor indexing）深化
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25164-J1 可变计数描述符 10^5 槽边界断言，未初始化槽返回显式空断言
### UNX-F25165 · push constant 与 specialization constant 面
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25165-J1 push constant 128B 边界断言，specialization 重编译缓存键断言
### UNX-F25166 · dynamic state 全集语义
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25166-J1 dynamic state 18 项逐项断言，静态/动态冲突检出断言
### UNX-F25167 · render pass/subpass 依赖语义
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25167-J1 依赖图执行序断言（DAG 无环），隐式依赖与显式依赖等价对照断言
### UNX-F25168 · framebuffer 与 attachment 生命周期
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25168-J1 attachment 引用计数断言，销毁竞争 fuzz 500 次零崩溃断言
### UNX-F25169 · input attachment 与 subpass 读语义
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25169-J1 input attachment 读回一致断言 20 例，跨 subpass 越界读零断言
### UNX-F25170 · 采样器 YCbCr 转换扩展位
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25170-J1 YCbCr 采样探针表落账，T2 缺档如实标注断言
### UNX-F25171 · 多 viewport/scissor 与 VR 预埋位
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25171-J1 双 viewport 并行渲染探针断言，16 viewport 上限断言
### UNX-F25172 · depth bounds/stencil 动态控制面
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25172-J1 depth bounds 测试边界断言 30 例，动态 stencil 写掩码断言
### UNX-F25173 · 查询池（timestamp/pipeline stats）VK 侧
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25173-J1 查询池分配/回收 10^4 次零泄漏断言，结果有效性旗标断言
### UNX-F25174 · secondary command buffer 并行录制
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25174-J1 二级录制并行 4 线程 10^3 次零竞争断言，execute 语义一致断言
### UNX-F25175 · VK 错误层与校验层桥接账
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25175-J1 校验层 200 类常见错误拦截映射断言，错码到三要素词典覆盖 100%
### UNX-F25176 · 管线编译离线预取面
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25176-J1 预取管线 50 组首帧零编译阻塞断言（首帧耗时对照下降可观测）
### UNX-F25177 · VK 面与 G3 DXVK 消费锚对账
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25177-J1 DXVK 消费 API 面 30 签名冻结候选 diff=0 断言
### UNX-F25178 · VK 面 fuzz（非法句柄/状态）
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25178-J1 非法状态 fuzz 2×10^3 次全数精确拒断言，零崩溃断言
### UNX-F25179 · VK 深化性能预算（提交开销）
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25179-J1 单次提交 CPU 开销 P95 ≤预算断言，越线红账断言
### UNX-F25180 · B19 批级总成
- 域/批：G2/B19｜纯功能行数：300｜状态：[骨架]｜判据：UNX-F25180-J1 批级断言 ≥25 条全过，VK 管线面冻结哈希入账
