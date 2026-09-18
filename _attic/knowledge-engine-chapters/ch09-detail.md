
---

# 卷二 · 细节规范篇

> 卷一回答"是什么、为什么"。卷二回答"具体长什么样"——把关键子系统的设计推到可以直接开工的深度。

---

# 附录 A 知识库开放格式 v1 完整规范

## A.0 规范的地位

本规范是知识库格式的**唯一权威定义**。任何实现（Rust 核心、C++ 插件、第三方程序、脚本工具）与本规范冲突时，以本规范为准。规范的每一次修订必须同步更新 knowledge-fmt crate 的测试向量——**规范、实现、测试三位一体，不可单独漂移**。

## A.1 库目录结构（正式版）

```
<库名>/
├── manifest.json          # 库级元数据（见 A.2）
├── nodes/
│   ├── 00000001.kseg      # 段文件，8 位十进制段号
│   ├── 00000002.kseg
│   └── ...
├── index/                 # 衍生索引，全部可重建
│   ├── shard-00.kidx      # ID 高位分片索引（见 A.6）
│   ├── shard-01.kidx
│   └── ...
├── blobs/                 # 大附件（v1.1 起用）
│   └── <sha256>           # 内容寻址
└── journal/               # 存储引擎自身日志（非用户数据）
```

规则：
- `nodes/` 与 `manifest.json` 是数据的全部真相；
- `index/`、`journal/` 任何时刻可删除重建，不丢数据；
- 库目录可以整体拷贝、压缩、放入版本控制——没有隐藏状态。

## A.2 manifest.json

```json
{
  "format_version": 1,
  "library_name": "我的知识世界",
  "created_at": 1789600000,
  "seed_language": "zh-CN",
  "stats": {
    "node_count_approx": 48213,
    "edge_count_approx": 96207,
    "tombstone_count_approx": 309
  },
  "settings": {
    "wander_temperature": 0.7,
    "hot_layer_limit_mb": 2048
  },
  "extensions": {}
}
```

- `format_version` 整数，递增不跳号；
- `stats` 是近似值（缓存），仅供展示，任何实现不得依赖其精确性；
- `extensions` 是开放扩展位——第三方实现可以在此写入自己的元数据，键名必须带命名空间前缀（如 `com.example.xxx`）。**未知的扩展键必须被忽略而不是报错**（前向兼容铁则）。

## A.3 kseg 段文件字节级定义

所有整数小端序。

```
偏移  大小  字段
0     8    magic = "KNOWSEGV1"（ASCII）
8     4    segment_id (u32)
12    8    created_at (u64, Unix 秒)
20    4    record_count (u32) —— 仅在段关闭时回填有效；活跃段此字段为 0xFFFFFFFF
24    4    header_crc32（覆盖偏移 0..23）
28    ...  records...
尾部  8    footer: 每 1024 条一个 {record_index u32, file_offset u64} 稀疏检查点数组，
           后随 checkpoint_count (u32) 与 footer_crc32 (u32)
```

记录结构：

```
偏移  大小  字段
0     4    body_len (u32)
4     4    crc32（覆盖 body 全部）
8     1    type (u8)
           0x01 = NodeRecord
           0x02 = EdgeRecord
           0x03 = TombstoneRecord
           0x04 = MetaRecord（预留）
9     4    seq (u32) —— 库内全局递增序号，崩溃恢复时用于定序
13    ...  body（varint 与长度前缀字符串构成，见 A.4）
```

- 写入顺序：先写 body_len + crc + type + seq，再写 body。**半条记录（写了一半崩溃）以 body_len 与实际剩余字节数不符判定，打开时安全截断**；
- 活跃段末尾允许 `0x00 padding` 对齐到 16 字节——解码器必须跳过全零尾部。

## A.4 记录 body 编码

字符串编码：`varint 字节长 + UTF-8 字节`。可选字符串：先一个 varint，0 表示 None，1 表示 Some（后跟字符串）。

**NodeRecord body：**
```
id            : 16 字节（u128 小端）
flags         : varint 位标志（bit0 有 my_title, bit1 有 my_body,
                bit2 有 def_title, bit3 有 def_body, bit4..保留）
my_title      : 可选字符串
my_body       : 可选字符串
def_title     : 可选字符串
def_body      : 可选字符串
created_at    : varint（Unix 秒）
head_ext      : 可选（预留扩展 TLV：varint tag + varint len + bytes）
```

**EdgeRecord body：**
```
from_id       : 16 字节
to_id         : 16 字节
weight        : 1 字节（u8, 0..255）
flags         : varint（bit0 bidir, bit1 marked/gold, bit2..保留）
note          : 可选字符串
created_at    : varint
```

**TombstoneRecord body：**
```
target_id     : 16 字节
was_node      : 1 字节（1=节点墓碑, 0=边墓碑）
deleted_at    : varint
```

**版本化原则**：body 的解析以"读尽即合法"为准则——未来 v2 在尾部追加字段时，v1 解码器读到 v1 已知长度即可停（由 flags/TLV 承载），未知 TLV 必须跳过不报错。**解析器永远宽容，写入器永远守规**（Postel 法则的存储版）。

## A.5 JSON 双向导出

导出格式（每个 kseg 对应一个 JSON 数组文件）：

```json
[
  {
    "type": "node",
    "id": "a1b2c3d4e5f60718a1b2c3d4e5f60718",
    "my_title": "混乱的账本",
    "def_title": "熵",
    "created_at": 1789600123,
    "links_hint": 3
  }
]
```

- u128 以 32 位小写十六进制字符串表示；
- 导出/导入必须通过字节级往返测试：`binary → json → binary` 逐字节一致（段顺序、记录顺序、padding 除外）；
- 导出是**无损**的：墓碑、金线标记、连接备注全部保留。

## A.6 索引文件 kidx（衍生品，规范从简）

索引是衍生品，规范只约束两点：其一，分片键 = id 最高 8 位（256 shard）；其二，任一 shard 可独立重建（扫描全 nodes/ 即可）。内部结构（B+ 树页布局）是实现自由域——**把自由留给实现，把稳定留给边界**。

## A.7 迁移器矩阵

| 从\到 | v1 | v2（未来） |
|---|---|---|
| v1 | — | 自动迁移（打开时） |
| v2 | 显式降级导出（可能丢新特性，需确认） | — |

规则：迁移必须发生在内存中的副本上，成功落盘前原文件不动（先写新的 nodes_v2/ 目录，校验通过后原子替换 manifest）。**任何一步失败，库保持原版本完好**。

---

# 附录 B knowledge-core 关键算法伪码

## B.1 邻域有限三度图（浮岛微缩图用）

```
fn neighborhood3(seed: NodeId, budget: usize = 96) -> SubGraph:
    frontier = [seed]
    visited = {seed}
    result = empty SubGraph
    depth = 0
    while frontier.not_empty() and depth < 3 and result.size < budget:
        next = []
        for node in frontier:
            edges = index.adjacent(node)          # 常数级（≤512 上限）
            edges.sort_by(weight desc)
            for e in edges.take(16):              # 每节点每层最多展开 16 条
                if e.other not in visited:
                    visited.add(e.other)
                    next.push(e.other)
                    result.add(node, e, e.other)
        frontier = next
        depth += 1
    return result                                  # 最坏 O(budget) = O(96)
```

性质断言（进测试）：任意输入规模下，返回图规模 ≤ budget；执行时间上界恒定。

## B.2 漫游步进

```
fn wander_step(current: NodeId, state: WanderState) -> NodeId:
    neighbors = index.adjacent(current)             # ≤512
    if neighbors.is_empty():
        return index.random_hot_node()              # 孤岛跳转：回到活跃区
    candidates = []
    for e in neighbors:
        base    = e.weight as f64 / 255.0
        fresh   = freshness(e.other, state.clock)   # 久未访问 → 高
        novel   = cross_domain_bonus(current, e.other)  # 跨学科边 → 最高
        inert   = 1.2 if direction_matches(state.heading, e.other)
                  else 1.0                          # 惯性：顺路 20% 倾向
        temp    = state.temperature                     # 用户可调 0..1
        score   = (base * 0.4 + fresh * 0.3 + novel * 0.3) * inert
        candidates.push((e.other, score.pow(1.0 / max(temp, 0.05))))
    return sample_softmax(candidates)               # 温度低=确定性高
```

- `cross_domain_bonus`：当前节点与候选的学科标签集（标签也是节点，沿连接上溯 2 度即可得）交集为空时 ×1.8；
- 温度参数即 manifest 里的 `wander_temperature`——温度是漫游唯一的"旋钮"，且它不是任务参数（不是"难度"），是情绪参数（"想不想走远一点"）。

## B.3 隐式保存与恢复

```
写入路径（用户无感知）：
  双击空白 → 内存中创建 Node（仅 my_body 或 my_title）
          → 追加 NodeRecord 到活跃 kseg（无 fsync，依赖 OS 页缓存）
          → 500ms 内合并为事务组，组满或 2s 定时 → fsync
恢复路径（打开时）：
  逐段扫描 footer 检查点 → 从检查点向后重放记录
  → 遇半条记录截断 → 墓碑回放 → 索引重建（后台）
  → 期间 UI 可用（热层先展示检查点时刻的世界）
```

最坏丢失窗口 ≤2 秒的输入——对"种念头"场景足够工业级（同类基准：终端 shell 历史、SQLite 默认 synchronous=NORMAL）。

## B.4 热度与新鲜度

```
freshness(id, now) = 1 / (1 + ln(1 + (now - last_visited[id]) / 86400))
heat[id]           = 指数移动平均(visit 事件, 半衰期 7 天)
逐出排序键         = heat / (1 + graph_distance(current_focus))
```

全部 O(1) 更新，无全局重算—— Again：**邻域有限纪律贯穿一切**。

---

# 附录 C 渲染管线详解

## C.1 每帧数据流

```
core 状态（上次帧以来变更集 Δ）
   → build_render_graph(Δ)          外壳装配层，增量构建
   → RenderGraph {                  SOA 布局
       nodes:  pos[3], radius, color_idx, lod, flags
       edges:  from_idx, to_idx, weight, gold, alpha
       cards:  transform, text_atlas_slot, style_idx
       ambient: fog_params, light_rig, grade_params
     }
   → passes:
       1. density  (compute)  节点密度场 → L0 星云纹理
       2. simulate (compute)  力导向增量步 + 弹簧动效积分
       3. geo      (graphics) 地貌网格 + 阴影 pass（PCSS）
       4. graph    (graphics) L0 光斑 / L1 instancing / L2 卡片
       5. post     (graphics) Bloom → 雾合成 → 体积光(条件) → ACES → FXAA
       6. ui       (graphics) egui 绘制
   → present (vsync / mailbox)
```

## C.2 instancing 细节

- L1 节点：单 draw call，实例属性走 per-instance storage buffer；10 万实例的 buffer 更新用**环形 staging**（三缓冲轮转，永不同帧读写同一 region）；
- 边：折线 strip 打包成 triangle list（每边 6 顶点），weight 映射 alpha 与宽度，金线走单独高亮 pass（加色混合 + Bloom 强化）；
- 拾取：GPU pick pass（颜色编码离屏渲染，1 像素读回）——万级以上规模不走路 raycast，保证悬停零延迟。

## C.3 文字渲染

- SDF 字体图集：2048² 图集，预烘焙含中文字符集一/二级常用字；字符缺失回退到运行时烘焙队列（异步，期间显示占位光条）；
- 卡片（L2）文本排版在 CPU（egui 风格 glyph run），结果缓存 by (node_id, width_bucket, text_hash)；
- 任何文字在缩放中不重排——重排只发生在"停稳"后的 150ms 静默期，避免缩放过程中的文字跳动。

## C.4 帧预算哨兵实现

```
每帧：各 pass 计时（timestamp query）
滑动 120 帧窗口 P95：
  geo 段超支 → 光影档位 -1（PCSS→硬阴影→无阴影）
  graph 段超支 → L1 密度 -1 档（远处聚合阈值收紧）
  post 段超支 → 体积光关闭 → Bloom mip 减半
恢复条件：连续 300 帧 P95 低于预算的 85% → 逐档恢复
日志：降级/恢复事件写入本地日志（可诊断，不打扰用户）
```

## C.5 美术基准场景

渲染引擎的验收场景固定为三个（进入快照测试）：

1. **星海**：100 万 L1 节点 + 30 万边 + 50 个 L0 光斑，中景镜头，80 帧含全光影；
2. **雾林**：哲学地貌——高度雾 + 体积光 + 200 张 L2 卡片近景，验证文字与光影共存质感；
3. **海岸**：物理/哲学交叉地带的边界过渡——密度场渐变 + 金线高亮 + 念头灯火 Bloom。

每个场景一份 golden image + 一份帧时间剖面基线。

---

# 附录 D 外壳 A 交互细节规格

## D.1 手势与快捷键总表

| 操作 | 鼠标 | 键盘 |
|---|---|---|
| 平移 | 拖拽空白 | 方向键 / WASD |
| 缩放 | 滚轮（指针为中心） | +/− |
| 种念头 | 双击空白 | N |
| 建连接 | 从节点边缘拖到另一节点 | 选中两个后 L |
| 金线标记 | Alt+点击边 | 选中边后 G |
| 形态切换 | 右上四粒光点 | 1/2/3/4 |
| 撤销/重做 | — | Ctrl+Z / Ctrl+Shift+Z |
| 漫游开始/停 | — | 空格（在漫游态） |
| 全库搜索 | — | Ctrl+K（浮岛式，非模态） |

- 全部快捷键可改（设置·键位页），默认方案遵守"单手可达"；
- 拖拽建立连接时，目标节点高亮呼吸；松手在空白处 = 取消并就地种一颗"未完成的想法"节点（**取消也是产出**——这是零仪式哲学的边角体现）。

## D.2 概念浮岛布局

浮岛出现位置：源节点上方 24px，优先避开链接方向密度高的一侧。宽度 380px 固定，高度随内容 120~400px。结构从上到下：我的语言（22px ember 暗亮体）→ 我的理解（14px ink）→ 分隔细线 → 官方定义（12px ink-dim，折叠，默认收起）→ 三度邻域微缩图（96px 高，点击任一邻点即跳转）。浮岛出现动画：从源节点位置生长（scale 0.92→1 + fade 180ms），消失 120ms。**浮岛永不抢焦点**：不拦截键盘，文字光标继续留在原处。

## D.3 空状态设计

每个形态的第一次打开（库为空时）：

- 图谱态：中央一颗种子灯 + 一句话："双击任何地方，种下第一个念头。" 下方极小字："或者按 2 进入漫游，先随便看看。"——**空状态的首要推荐动作是漫游**，这是产品价值观的直接表达：先体验，再生产；
- 阅读态：一卷空书，页脚："这卷书会由你的念头自己写出来。"
- 书写态：一张白页，光标已在闪烁，无任何工具栏。

---

# 附录 E 外壳 C 内核接口规格

## E.1 内核侧 API（apps/mod.rs 注册形态）

```
// 内核内建应用：knowledge（no_std）
pub struct KnowledgeApp {
    store: KSegReader<'static>,      // 只读段读取器（映射内核 fs）
    index: MemBTree,                 // 启动时从检查点加载
}

pub fn search(&self, q: &str, limit: usize) -> Vec<HeadHit>;
pub fn node(&self, id: u128) -> Option<NodeView>;   // 惰性加载正文
pub fn adjacent(&self, id: u128) -> Option<EdgeList>;
pub fn stats(&self) -> LibraryStats;
```

- 无写路径（v1 内核外壳只读；写回走"导出记录文件 + 桌面端合并"，与任务 15 的用户态通道一致）；
- 内存预算：索引 ≤48MB + 段读取窗口 ≤16MB（内核侧配置）；
- 文本检索：小写归一 + 前缀/子串两级（100 万节点规模下，朴素扫描 + 轻头过滤的实测预算已足；更大会升级到内存倒排，接口不变）。

## E.2 与 syscall 表的关系

不新增 syscall——内核外壳通过现有 syscall 表的文件读取族完成加载，检索服务对用户态程序以 IPC 服务（srv）暴露：`knowledge.search(query) -> hits`。未来 WASM 作品读取知识库，同样经由该 IPC 面 + 能力位授权，**多语言作品与内核共享同一个最小知识接口**。

---

# 附录 F 测试向量与哲学断言清单（最终版）

## F.1 knowledge-fmt 向量（每条都是文件级 fixture）

1. 空 manifest → 打开成功，空世界；
2. 单节点（仅 my_title）→ 往返一致；
3. 十万节点段 + 随机截断三处 → 恢复后 node_count 与墓碑数精确断言；
4. 二进制↔JSON 往返 → 字节一致（除 padding）；
5. v1 文件 + manifest 标 v2 → 给出版本提示，不崩溃；
6. extensions 未知键 → 忽略且导出时保留原样。

## F.2 哲学断言（CI 红线，永不豁免）

1. `knowledge-core` 公开 API 不含 validate/correct/score/complete 词根；
2. 数据模型无 learned/mastered/done 字段（模型快照测试）；
3. 渲染层无 unvisited 可视化路径；
4. 保存路径无模态对话框（交互回归）；
5. 我的层与官方层在导出物中同权限同文件（导出内容断言）；
6. 漫游态无统计产出（漫游会话结束事件不携带任何计数负载）。

*（卷二完）*
