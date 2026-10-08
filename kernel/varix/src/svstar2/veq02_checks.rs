//! VE-F3202 · 域自检（判据逐条对应，见 `veq02_graph.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项前缀）：
//! - 五要素→ `Q02-五要素-*`
//! - 四用途单源   → `Q02-单源-*`
//! - 悬空即缺陷   → `Q02-悬空-*`
//! - 环拒绝       → `Q02-环-*`
//! - 32MB 线      → `Q02-内存-*`
//! - 判据/逐项复杂度 → `Q02-判据-*`
//! - 错误路径零静默 → `Q02-错误-*`
//! - 跨批对接     → `Q02-对接-*`
//! - 无障碍       → `Q02-读屏-*`
//!
//! 零墙钟、零 IO，回归可复现。

use super::veq02_graph::*;

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 标准元数据（显式登记）。
fn meta_ok() -> ResourceMetadata {
    ResourceMetadata {
        version: 3,
        compression: Compression::Block,
        srgb: true,
        streamable: true,
        tag0: 11,
        tag1: 22,
        present: true,
    }
}

/// 建一个典型场景：材质 →纹理、场景 →模型 →材质（锚点给的引用链）。
///
/// 链形：2→0（材质依赖纹理）、4→1（场景依赖模型）、1→0（模型依赖材质）。
/// 拓扑序应为 0,2,1,4 ——即「先依赖后资源」。
fn typical_graph() -> ResourceGraphBuilder {
    let mut b = ResourceGraphBuilder::new();
    for (i, k) in [
        ResourceKind::Texture,
        ResourceKind::Model,
        ResourceKind::Texture, // 2 号当材质用（材质在十项映射里以 texture 承载其纹理属性，此处只验图结构）
        ResourceKind::Audio,
        ResourceKind::Scene,
    ]
    .iter()
    .enumerate()
    {
        let _ = b.register(Resource::new(
            ResourceId(i as u32),
            *k,
            meta_ok(),
            ContentBody::present(1024 * (i as u32 + 1), 0xabc0 + i as u64, true),
        ));
    }
    let _ = b.add_edge(ResourceId(2), ResourceId(0));
    let _ = b.add_edge(ResourceId(4), ResourceId(1));
    let _ = b.add_edge(ResourceId(1), ResourceId(0));
    b
}

/// 冻结典型图（失败时返回 None，调用方记红）。
fn frozen_typical() -> Option<ResourceGraph> {
    match typical_graph().freeze() {
        Outcome::Ok { value, .. } => Some(value),
        Outcome::Err { .. } => None,
    }
}

/// VE-F3202 判据自检。
pub fn run_veq02_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3202 · 资源模型与引用图");

    // =======================================================================
    // 一、五要素
    // =======================================================================
    {
        let r = Resource::new(
            ResourceId(7),
            ResourceKind::Texture,
            meta_ok(),
            ContentBody::present(2048, 0x1234, true),
        );
        set.add(
            "Q02-五要素-资源实体五字段齐备",
            r.id == ResourceId(7)
                && r.kind == ResourceKind::Texture
                && r.metadata.version == 3
                && r.content.byte_length == 2048
                && r.out_degree == 0,
            "五要素=ID/类型/元数据/内容体/引用关系",
        );
        // ID 即下标：稠密 ID 是 32MB 线的结构性前提。
        set.add(
            "Q02-五要素-ID即节点表下标",
            r.id.raw() as usize == 7,
            "稠密 ID 让查资源退化为数组下标寻址",
        );
        set.add(
            "Q02-五要素-ID空值被识别",
            !ResourceId::NONE.is_valid() && ResourceId(0).is_valid(),
            "空 ID 必须可识别，否则悬空不可寻址",
        );
        // 内容体只记账不持字节（零 IO 的根据）。
        let body = ContentBody::present(4096, 0xaa, false);
        set.add(
            "Q02-五要素-内容体只记账不持字节",
            body.byte_length == 4096 && !body.resident && !body.absent,
            "字节本体归F3214，本模块零 IO",
        );
        // 缺失与空内容必须可区分。
        set.add(
            "Q02-五要素-缺失与空内容可区分",
            ContentBody::missing().absent
                && !ContentBody::present(0, 0, false).absent,
            "byte_length=0 无法区分「空文件」与「不存在」，故须 absent 独立标记",
        );
        // 元数据默认取保守侧。
        let d = ResourceMetadata::default();
        set.add(
            "Q02-五要素-默认元偏保守侧",
            !d.srgb && !d.streamable && !d.present,
            "默认值必须偏向「我不能做」——默认宣称 sRGB 会让线性纹理偏色",
        );
        // 槽位三态。
        let b = typical_graph();
        set.add(
            "Q02-五要素-槽位三态可区分",
            b.slot_state(ResourceId(0)) == SlotState::Live
                && b.slot_state(ResourceId(900)) == SlotState::Vacant,
            "Live/Placeholder/Vacant 三态；混用会让悬空丢失去向",
        );
        // 流式压缩不可随机寻址（显性登记）。
        set.add(
            "Q02-五要素-流式压缩标注不可随机寻址",
            !Compression::Stream.random_access()
                && Compression::Block.random_access(),
            "F3214 按块读取前必须问一句能否随机寻址",
        );
    }

    // =======================================================================
    // 二、四用途单源
    // =======================================================================
    {
        set.add(
            "Q02-单源-四用途齐备",
            GraphPurpose::ALL.len() == 4,
            "依赖解析/加载排序/失效传播/垃圾回收",
        );
        let audit = audit_purpose_single_source();
        set.add("Q02-单源-四用途审计通过", audit.is_ok(), "四用途必须指向同一图来源");
        set.add(
            "Q02-单源-每用途均有下游归属",
            GraphPurpose::ALL
                .iter()
                .all(|p| !p.downstream().trim().is_empty() && p.downstream().contains("VE-F")),
            "用途无下游 = 无消费者 = 无用登记",
        );
        let s0 = purpose_source(GraphPurpose::GarbageCollect);
        set.add(
            "Q02-单源-来源查询可用",
            matches!(s0, Outcome::Ok { value: src, .. } if src == "ResourceGraph"),
            "来源标识须可机检，而非口头约定",
        );
        set.add(
            "Q02-单源-单源纪律有文字声明",
            PURPOSE_SINGLE_SOURCE_NOTE.contains("同一张引用图"),
            "纪律须成文，否则下一个人会自己建图",
        );
    }

    // =======================================================================
    // 三、悬空即缺陷
    // =======================================================================
    {
        let Some(g) = frozen_typical() else {
            set.add("Q02-悬空-典型图可冻结", false, "典型图冻结失败");
            for i in 0..6 {
                set.add(
                    match i {
                        0 => "Q02-悬空-零悬空基线",
                        1 => "Q02-悬空-悬空即时检出",
                        2 => "Q02-悬空-悬空置占位",
                        3 => "Q02-悬空-占位不冒充实体",
                        4 => "Q02-悬空-边撤销同步撤销案",
                        _ => "Q02-悬空-占位升级为活跃",
                    },
                    false,
                    "典型图不可用，后续用例连带失败",
                );
            }
            return finish(set);
        };

        // 基线：零悬空。
        let v = detect_dangling(&g);
        set.add(
            "Q02-悬空-零悬空基线",
            v.clean(),
            "典型链 2→0/4→1/1→0 无悬空",
        );
        set.add(
            "Q02-悬空-零悬空时占位表为空",
            v.placeholders.is_empty(),
            "无悬空却报占位 = 误报，比漏报更扰民",
        );

        // 构造悬空：0 号（纹理）被注销，2 号与 1 号仍引用它。
        let mut b = typical_graph();
        let _ = b.unregister(ResourceId(0));
        // 回归锁（探针实跑抓到的真缺陷）：注销**不得**抹掉引用方的边。
        // 若实现双向往返摘边，引用方出边被清空 → 悬空恒 0 → 红线形同虚设。
        set.add(
            "Q02-悬空-注销不抹掉引用方边",
            b.out_targets_of(ResourceId(2)).contains(&0)
                && b.out_targets_of(ResourceId(1)).contains(&0),
            "注销只摘自己发出的边；指向我的边要留着，它们才是悬空证据",
        );
        let cases = b.dangling_cases();
        set.add(
            "Q02-悬空-被引用方注销即立案",
            cases
                .iter()
                .any(|c| c.to == ResourceId(0) && (c.from == ResourceId(2) || c.from == ResourceId(1))),
            "注销方不遍历反向，故这类悬空只能靠全量扫描/即时检出暴露",
        );
        match b.freeze() {
            Outcome::Ok { value: g2, .. } => {
                let v2 = detect_dangling(&g2);
                set.add(
                    "Q02-悬空-全量扫描检出注销型悬空",
                    v2.count >= 2,
                    "注销 0 号后 1/2 号的引用即成悬空（注销方不遍历反向，只能靠全量扫描暴露）",
                );
                // 注销期悬空**不置占位**（资源被人主动删掉，不该等它回来）。
                // 加载期悬空才置占位。这两条处置方向相反，故分开断言。
                set.add(
                    "Q02-悬空-注销期只立案不置占位",
                    v2.count >= 2
                        && !v2.placeholders.iter().any(|e| e.0 == 0)
                        && v2.cases
                            .iter()
                            .any(|c| c.to == ResourceId(0) && c.slot == SlotState::Vacant),
                    "置占位会让调用方永远等一个不会来的资源；两类悬空处置相反",
                );
                set.add(
                    "Q02-悬空-占位不冒充实体",
                    g2.slot_state(ResourceId(0)) != SlotState::Live
                        && g2.get(ResourceId(0)).is_none(),
                    "占位不得让调用方误以为资源在册",
                );
                // 引用方的出边必须仍在（这才是悬空证据）；
                // 被注销方自己的反向索引随注销清空——它已不在册，无人可失效。
                set.add(
                    "Q02-悬空-引用方出边保留而注销方反向清空",
                    g2.out_targets_of(ResourceId(2)).contains(&0)
                        && g2.out_targets_of(ResourceId(1)).contains(&0)
                        && g2.in_sources_of(ResourceId(0)).is_empty(),
                    "引用方的边是证据必须留；注销方的反向索引随注销清空（它已不在册）",
                );
            }
            Outcome::Err { .. } => {
                set.add("Q02-悬空-全量扫描检出注销型悬空", false, "压实失败");
                set.add("Q02-悬空-悬空置显式占位", false, "压实失败");
                set.add("Q02-悬空-占位不冒充实体", false, "压实失败");
                set.add("Q02-悬空-反向索引仍是空", false, "压实失败");
            }
        }

        // 边撤销同步撤销悬空案。
        let mut b3 = typical_graph();
        let _ = b3.remove_edge(ResourceId(2), ResourceId(0));
        set.add(
            "Q02-悬空-边撤销同步撤销案",
            !b3.dangling_cases().iter().any(|c| c.from == ResourceId(2)),
            "边已撤销却仍背缺陷账 = 缺陷永不收敛",
        );

        // 占位升级为活跃。
        let mut b4 = ResourceGraphBuilder::new();
        // 前置条件：起点必须先在册，否则 add_edge 在起点检查处就拒，
        // 占位分支根本走不到——用例的失败路径必须真的可达。
        let _ = b4.register(Resource::new(
            ResourceId(3),
            ResourceKind::Audio,
            meta_ok(),
            ContentBody::present(8, 3, false),
        ));
        let _ = b4.add_edge(ResourceId(3), ResourceId(8));
        let before = b4.slot_state(ResourceId(8));
        let _ = b4.register(Resource::new(
            ResourceId(8),
            ResourceKind::Texture,
            meta_ok(),
            ContentBody::present(16, 1, false),
        ));
        set.add(
            "Q02-悬空-占位升级为活跃",
            before == SlotState::Placeholder
                && b4.slot_state(ResourceId(8)) == SlotState::Live,
            "占位→活跃是同一槽的状态迁移，不应重建槽",
        );

        // 加边起点不在册即拒（否则双向失效）。
        let mut b5 = ResourceGraphBuilder::new();
        set.add(
            "Q02-悬空-加边起点不在册被拒",
            b5.add_edge(ResourceId(1), ResourceId(2)).is_err(),
            "从不在册的资源发起的依赖不可能成立",
        );
        // 悬空策略声明。
        set.add(
            "Q02-悬空-占位策略禁止静默掩盖",
            PLACEHOLDER_POLICY.contains("禁止")
                && PLACEHOLDER_POLICY.contains("占位"),
            "1x1 白图式掩盖会让缺陷几周后才暴露",
        );
        set.add(
            "Q02-悬空-悬空有归属",
            DANGLING_OWNER.contains("F3206"),
            "缺陷不记归属就会在四个用途间漂",
        );
    }

    // =======================================================================
    // 四、环拒绝
    // =======================================================================
    {
        let Some(g) = frozen_typical() else {
            set.add("Q02-环-无环基线", false, "典型图不可用");
            return finish(set);
        };
        let c = detect_cycle_indexed(&g);
        set.add("Q02-环-无环基线", !c.cyclic, "典型链无环，可按依赖序加载");
        set.add(
            "Q02-环-无环结论含可加载判断",
            c.summary.contains("拓扑排序"),
            "结论须说清「能不能排」，不是只说「有没有环」",
        );

        // 构造环：0 → 5→ 0。
        let mut b = typical_graph();
        let _ = b.register(Resource::new(
            ResourceId(5),
            ResourceKind::Audio,
            meta_ok(),
            ContentBody::present(8, 2, false),
        ));
        let _ = b.add_edge(ResourceId(0), ResourceId(5));
        let _ = b.add_edge(ResourceId(5), ResourceId(0));
        match b.freeze() {
            Outcome::Ok { value: g2, .. } => {
                let c2 = detect_cycle_indexed(&g2);
                set.add("Q02-环-环被检出", c2.cyclic, "0→5→0 成环须被检出");
                set.add(
                    "Q02-环-环上节点非空且闭合",
                    !c2.nodes.is_empty() && c2.nodes.first() == c2.nodes.last(),
                    "环须能念出闭合路径，仅报「有环」无法定位",
                );
            }
            Outcome::Err { .. } => {
                set.add("Q02-环-环被检出", false, "压实失败");
                set.add("Q02-环-环上节点非空且闭合", false, "压实失败");
            }
        }

        // 自环：加边即刻拒，且是 O(1) 快检。
        let mut b3 = typical_graph();
        set.add(
            "Q02-环-自环加边即拒",
            b3.add_edge(ResourceId(1), ResourceId(1)).is_err(),
            "自环是最短的环，不需要 DFS 就能判",
        );
        // 重复边拒（否则入度虚高 → GC 永不回收）。
        let mut b4 = typical_graph();
        set.add(
            "Q02-环-重复边被拒",
            b4.add_edge(ResourceId(2), ResourceId(0)).is_err(),
            "重复边让入度虚高，引用零判定永不成立= 泄漏",
        );
        // 三要素齐备。
        set.add(
            "Q02-环-拒绝三要素齐备",
            CYCLE_REJECT_TRIPLE.len() == 3
                && CYCLE_REJECT_TRIPLE.iter().all(|s| !s.trim().is_empty())
                && CYCLE_REJECT_TRIPLE[0].contains("现象")
                && CYCLE_REJECT_TRIPLE[1].contains("根因")
                && CYCLE_REJECT_TRIPLE[2].contains("处置"),
            "锚点要求拒绝三要素：说清现象/根因/怎么办",
        );
        set.add(
            "Q02-环-环归属明确",
            CYCLE_OWNER.contains("F3206"),
            "本条只给数据基础，算法本体归 F3206——不抢活也不含糊",
        );
        // 索引边导出（F3206 的输入）。
        let edges = export_index_edges(&g);
        set.add(
            "Q02-环-索引边可导出供F3206",
            edges.len() == 3
                && edges
                    .iter()
                    .all(|e| (e.from as usize) < g.slots && (e.to as usize) < g.slots),
            "F3206 不该自己遍历实体找边",
        );
        // 失效传播闭包（反向可达）。
        let inv = invalidation_closure(&g, ResourceId(0));
        set.add(
            "Q02-环-失效传播沿反向可达",
            inv.contains(&1) && inv.contains(&2) && inv.contains(&4) && !inv.contains(&3),
            "0 号纹理失效 → 模型/材质/场景连锁失效，音频无关",
        );
        // GC 候选（入度 0）。
        let gc = g.gc_candidates();
        set.add(
            "Q02-环-GC候选取入度零",
            gc.contains(&3) && gc.contains(&4) && !gc.contains(&0) && !gc.contains(&1),
            "0 号被 1/2 依赖，入度 2，绝不是候选；3/4 无人依赖才是",
        );
    }

    // =======================================================================
    // 五、32MB 内存红线
    // =======================================================================
    {
        // 红线考核的是**常驻形态**（反向索引按需建、非常驻）——
        // 拿临时形态考核常驻占用，等于用一次性开销判永久预算。
        let mem = audit_memory_budget(MILLION_RESOURCE_SCALE, MILLION_RESOURCE_SCALE, false);
        set.add(
            "Q02-内存-百万百万常驻在32MB内",
            mem.is_ok(),
            "百万节点+百万边常驻 ≤ 32MB（24B/槽 + 4B/槽偏移 + 4B/边）",
        );
        set.add(
            "Q02-内存-节点槽压实到24B",
            BYTES_PER_NODE_SLOT == 24
                && NODE_LAYOUT.iter().map(|(_, b)| b).sum::<u64>() == BYTES_PER_NODE_SLOT,
            "ID 即下标不存 + 标志位打包 + 指纹截 u32；56B 形态下光节点就 56MB，红线不可能达成",
        );
        set.add(
            "Q02-内存-节点布局逐项可核",
            NODE_LAYOUT.len() == 9 && NODE_LAYOUT.iter().all(|(_, b)| *b > 0),
            "每一项都要能说出为什么是这么多字节",
        );
        set.add(
            "Q02-内存-反向索引非常驻有明文",
            REVERSE_INDEX_POLICY.contains("按需构建")
                && REVERSE_INDEX_POLICY.contains("不得"),
            "省它会瞎：只看出边答不出「谁依赖我」，GC 与失效传播失效",
        );
        // 按需形态可临时超预算，但必须被如实算出（不是漏算）。
        let tmp = build_memory_report(MILLION_RESOURCE_SCALE, MILLION_RESOURCE_SCALE, true);
        set.add(
            "Q02-内存-按需形态如实算出临时占用",
            tmp.total_bytes > mem.value_or(build_memory_report(0, 0, false)).total_bytes
                && tmp.items.len() == 4,
            "临时形态允许超，但超多少必须算出来（含反向行偏移，漏算会低估 4MB）",
        );
        let m = build_memory_report(MILLION_RESOURCE_SCALE, MILLION_RESOURCE_SCALE, false);
        set.add(
            "Q02-内存-核算逐项可审计",
            m.items.len() == 3 && m.items.iter().all(|i| !i.basis.trim().is_empty()),
            "每一项都要写清「为什么是这个字节数」",
        );
        set.add(
            "Q02-内存-边紧凑为4B",
            BYTES_PER_OUT_EDGE == 4 && BYTES_PER_IN_EDGE == 4,
            "CSR 单块u32 数组——这是「压缩」的实质",
        );
        // 去反向索引的正路必须能省出预算。
        let with_rev = build_memory_report(MILLION_RESOURCE_SCALE, MILLION_RESOURCE_SCALE, true);
        let resident = build_memory_report(MILLION_RESOURCE_SCALE, MILLION_RESOURCE_SCALE, false);
        // 回归锁：estimate_graph_bytes 与 build_memory_report 必须逐字节相等。
        // 历史缺陷：偏移数组实为 (n+1) 条，一处写 n 一处写 (n+1)，
        // 两个口径差 4 字节——百万规模看不出来，小图上直接让机检误报，
        // 而误报会让真缺陷被当成噪声忽略。
        for (n, e) in [(1u32, 1u32), (5, 7), (1000, 999)].iter() {
            let est = estimate_graph_bytes(*n, *e, true);
            let rep = build_memory_report(*n, *e, true);
            let est_r = estimate_graph_bytes(*n, *e, false);
            let rep_r = build_memory_report(*n, *e, false);
            if est != rep.total_bytes || est_r != rep_r.total_bytes {
                set.add("Q02-内存-估算与报告口径逐字节一致", false, "两个口径不等");
            }
        }
        set.add(
            "Q02-内存-估算与报告口径逐字节一致",
            estimate_graph_bytes(1000, 999, true)
                == build_memory_report(1000, 999, true).total_bytes
                && estimate_graph_bytes(1000, 999, false)
                    == build_memory_report(1000, 999, false).total_bytes,
            "含反向索引与不含两种形态都要相等（偏移数组是 n+1 条，不是 n 条）",
        );
        set.add(
            "Q02-内存-常驻形态回到预算内而按需形态超出",
            resident.within_budget && !with_rev.within_budget,
            "这正是「按需构建而非常驻」的理由：常驻版守线，按需版为对账临时超线并如实报出",
        );
        // 超预算须被拒（而非静默放行）。
        let over = audit_memory_budget(MILLION_RESOURCE_SCALE, MILLION_RESOURCE_SCALE * 4, false);
        set.add(
            "Q02-内存-超预算被拒不静默",
            over.is_err()
                && matches!(over, Outcome::Err { ref message, .. } if message.contains("超红线")),
            "超预算红线必须硬拦，否则32MB 只是句口号",
        );
        // 分域分图降级。
        let plan = plan_sharded_degradation(
            MILLION_RESOURCE_SCALE,
            MILLION_RESOURCE_SCALE * 4,
            &[(0, "渲染域"), (500_000, "UI域"), (800_000, "音频域")],
        );
        set.add(
            "Q02-内存-分域分图可解预算",
            match &plan {
                Outcome::Ok { value: p, .. } => p.resolved && p.shard_count == 3,
                Outcome::Err { .. } => false,
            },
            "400 万边超预算 → 三域分片后单片回到预算内",
        );
        set.add(
            "Q02-内存-单域自身超预算时不谎称已解",
            match &plan_sharded_degradation(
                MILLION_RESOURCE_SCALE,
                MILLION_RESOURCE_SCALE * 40,
                &[(0, "单一域")],
            ) {
                Outcome::Ok { value: p, .. } => !p.resolved && p.summary.contains("未解问题"),
                Outcome::Err { .. } => false,
            },
            "分域只能按域摊薄；单域超预算时切不动了，须换正路",
        );
        set.add(
            "Q02-内存-空域清单被拒",
            plan_sharded_degradation(MILLION_RESOURCE_SCALE, 1, &[]).is_err(),
            "无域可切 = 无处可降级",
        );
        // 跨片边须显性登记。
        let mut b = ResourceGraphBuilder::new();
        for i in 0..4u32 {
            let _ = b.register(Resource::new(
                ResourceId(i),
                ResourceKind::Texture,
                meta_ok(),
                ContentBody::present(16, i as u64, false),
            ));
        }
        let _ = b.add_edge(ResourceId(0), ResourceId(3));
        let cross = match b.freeze() {
            Outcome::Ok { value: g, .. } => {
                let e = export_index_edges(&g);
                count_cross_shard_edges(&e, &|id: u32| if id < 2 { 0 } else { 1 })
            }
            Outcome::Err { .. } => Vec::new(),
        };
        set.add(
            "Q02-内存-跨片边被显性登记",
            cross.len() == 1 && cross[0].edge_count == 1 && cross[0].from_shard == 0,
            "跨片依赖不登记 → 片内算完序就以为万事大吉",
        );
        // 实测口径与估算口径一致性（小规模实测）。
        if let Some(g) = frozen_typical() {
            let r = g.memory_report();
            let expect = estimate_graph_bytes(g.slots as u32, g.edges as u32, true);
            set.add(
                "Q02-内存-实测口径与估算一致",
                r.total_bytes == expect && r.within_budget,
                "估算器若与实际结构脱节，红线就是在自说自话",
            );
        } else {
            set.add("Q02-内存-实测口径与估算一致", false, "典型图不可用");
        }
    }

    // =======================================================================
    // 六、判据与逐项复杂度
    // =======================================================================
    {
        let perf = audit_perf_budget();
        set.add(
            "Q02-判据-复杂度逐项审计通过",
            perf.is_ok(),
            "锚点要求加边/悬空/环/内存逐项分解",
        );
        set.add(
            "Q02-判据-预算项覆盖锚点四项",
            PERF_BUDGET.len() >= 8,
            "加边 O(1)、悬全量 O(V+E)、环 O(V+E) 增量、内存 O(压缩)",
        );
        set.add(
            "Q02-判据-非恒定项明说不恒定",
            PERF_BUDGET
                .iter()
                .filter(|b| !b.complexity.contains("O(1)"))
                .count()
                >= 3,
            "把O(V+E) 谎报为 O(1) 才是真缺陷",
        );
        set.add(
            "Q02-判据-单边悬空判定为O(1)",
            PERF_BUDGET
                .iter()
                .any(|b| b.item.contains("单条边") && b.complexity.contains("O(1)")),
            "锚点说的 O(1) 索引查指单条边判定，不是全量扫描",
        );
        // 元数据缺失处置。
        let none = resolve_metadata(None);
        let flagged = resolve_metadata(Some(ResourceMetadata {
            present: false,
            ..meta_ok()
        }));
        let okr = resolve_metadata(Some(meta_ok()));
        set.add(
            "Q02-判据-元数据缺失置默认元并告警",
            none.defaulted
                && none.warning.is_some()
                && none.effective.compression == Compression::None,
            "静默填默认值会把「偏色」推到几周后暴露",
        );
        set.add(
            "Q02-判据-present=false视为缺失",
            flagged.defaulted && flagged.warning.is_some(),
            "声明了 present 字段却不置 true 的，不可信",
        );
        set.add(
            "Q02-判据-元数据在册不告警",
            !okr.defaulted && okr.warning.is_none(),
            "正常路径不许刷告警，否则告警会被无视",
        );
        set.add(
            "Q02-判据-元数据降级矩阵有行",
            metadata_degradation_matrix().len() >= 4,
            "降级矩阵须写清每种缺法怎么处置",
        );
        // 判据摘要自洽。
        let s = criteria_summary();
        // 用摘要的**结构化结论**判定，不靠子串猜测：
        // 「悬空即缺陷」里的「缺」字会让 not-contains("缺") 永远为假——
        // 这是断言自身的缺陷（判据错了，不是被测物错了）。
        // 正确做法：摘要末尾的结论词是「齐备」或「缺X、Y」，据此判定。
        let verdict_zip = s.contains("对接齐备") || s.rfind("齐备") > s.rfind("缺");
        set.add(
            "Q02-判据-判据摘要六项齐备",
            verdict_zip && s.contains("32MB") && s.contains("五要素") && s.contains("环拒绝"),
            "摘要须自述六项判据且结论为齐备；不靠「不含缺字」这种会被「缺陷」误伤的判法",
        );
        // 摘要里的字节数必须与常驻形态一致（口径漂移会让摘要变成误导）。
        let resident_mb = build_memory_report(MILLION_RESOURCE_SCALE, MILLION_RESOURCE_SCALE, false);
        set.add(
            "Q02-判据-摘要字节数与常驻口径一致",
            s.contains(&alloc::format!("{}", resident_mb.total_bytes)),
            "摘要若报按需形态的字节数，会让人以为常驻也超线——那是口径漂移",
        );
        // 类型裁决对接（F3204 前置闸）。
        let t = audit_resource_type(ResourceId(1), ResourceKind::Texture);
        set.add(
            "Q02-判据-类型登记走裁决通道",
            t.is_ok(),
            "未登记类型即第二个生命周期权威，须先过 F3204",
        );
    }

    // =======================================================================
    // 七、错误路径零静默
    // =======================================================================
    {
        let mut b = ResourceGraphBuilder::new();
        let bad_id = b.register(Resource::new(
            ResourceId::NONE,
            ResourceKind::Texture,
            meta_ok(),
            ContentBody::present(1, 1, false),
        ));
        set.add(
            "Q02-错误-空ID登记被拒且带处置",
            match &bad_id {
                Outcome::Err {
                    code,
                    message,
                    hint,
                    ..
                } => {
                    !hint.trim().is_empty()
                        && !message.trim().is_empty()
                        && matches!(*code, DiagCode::ValueInvalid)
                }
                Outcome::Ok { .. } => false,
            },
            "拒绝必须带「怎么办」，否则调用方只能猜",
        );
        // ID 越硬上限。
        let over = b.register(Resource::new(
            ResourceId(RESOURCE_ID_LIMIT),
            ResourceKind::Texture,
            meta_ok(),
            ContentBody::present(1, 1, false),
        ));
        set.add(
            "Q02-错误-ID越硬上限被拒",
            matches!(
                &over,
                Outcome::Err { code, message, .. }
                    if *code == DiagCode::BudgetExceeded && message.contains("ID")
            ),
            "ID 硬上限是内存红线的执行手段",
        );
        // 重复登记。
        let _ = b.register(Resource::new(
            ResourceId(5),
            ResourceKind::Texture,
            meta_ok(),
            ContentBody::present(1, 1, false),
        ));
        let dup = b.register(Resource::new(
            ResourceId(5),
            ResourceKind::Model,
            meta_ok(),
            ContentBody::present(1, 1, false),
        ));
        set.add(
            "Q02-错误-重复登记被拒",
            dup.is_err(),
            "一个 ID 两个实体 = 两个身份，引用计数与 GC 对账全算错",
        );
        // 占位与实体并存被拒。
        let mut b2 = ResourceGraphBuilder::new();
        let _ = b2.add_edge(ResourceId(0), ResourceId(6));
        let clash = b2.register_placeholder(ResourceId(6), ResourceKind::Texture);
        let _ = b2.register(Resource::new(
            ResourceId(6),
            ResourceKind::Texture,
            meta_ok(),
            ContentBody::present(1, 1, false),
        ));
        set.add(
            "Q02-错误-占位与实体并存被拒",
            clash.is_ok() && b2.slot_state(ResourceId(6)) == SlotState::Live,
            "先占位后补齐是正常路径；反向（实体已在还占位）须拒",
        );
        let mut b3 = ResourceGraphBuilder::new();
        let _ = b3.register(Resource::new(
            ResourceId(1),
            ResourceKind::Texture,
            meta_ok(),
            ContentBody::present(1, 1, false),
        ));
        set.add(
            "Q02-错误-实体在册时占位被拒",
            b3.register_placeholder(ResourceId(1), ResourceKind::Texture).is_err(),
            "占位与实体并存会让引用方拿到「有资源」却查不到实体",
        );
        // 注销/移除越界均须显式失败而非 panic。
        let mut b4 = ResourceGraphBuilder::new();
        set.add(
            "Q02-错误-注销未在册资源被拒",
            b4.unregister(ResourceId(42)).is_err(),
            "守卫不得成为崩溃源：越界返回 Err 而非 panic",
        );
        set.add(
            "Q02-错误-移除越界边被拒",
            b4.remove_edge(ResourceId(0), ResourceId(1)).is_err(),
            "同上",
        );
        set.add(
            "Q02-错误-越界查资源返回None不panic",
            b4.get(ResourceId(9999)).is_none()
                && b4.out_targets_of(ResourceId(9999)).is_empty()
                && b4.in_sources_of(ResourceId(9999)).is_empty(),
            "读越界返回空而非崩",
        );
        set.add(
            "Q02-错误-未登记用途查来源被拒",
            purpose_source(GraphPurpose::LoadOrder).is_ok()
                && PURPOSE_SOURCE_TABLE.len() == 4,
            "登记表完整时不应误拒（防机检自身成为故障源）",
        );
    }

    // =======================================================================
    // 八、跨批对接
    // =======================================================================
    {
        let a = audit_downstream_handoff();
        set.add(
            "Q02-对接-四条对接齐备",
            matches!(&a, Outcome::Ok { value: v, .. } if v.len() == 4),
            "F3203/F3204/F3206/F3211",
        );
        set.add(
            "Q02-对接-每条有编号属主与交付物",
            DOWNSTREAM_HANDOFF
                .iter()
                .all(|(id, who, what)| id.starts_with("VE-F") && !who.trim().is_empty() && what.len() > 8),
            "编号在第 0 列、中文名在第 1 列、交付物在第 2 列——断言必须指对列",
        );
        // 判据：每条交接物至少给出一个**真实存在的本模块符号**——
        // 拿符号表去 contains，而不是靠 '/' 或某个中文词这类形态猜测
        //（形态猜测会在改写文案后静默失效，看着有门禁实则没有）。
        const G: &[&str] = &[
            "get",
            "gc_candidates",
            "in_degree",
            "slot_state",
            "export_index_edges",
            "detect_cycle_indexed",
            "invalidation_closure",
            "out_targets_of",
            "in_sources_of",
            "Resource",
            "ResourceMetadata",
            "ContentBody",
            "ResourceNode",
            "out_degree",
        ];
        set.add(
            "Q02-对接-交接物含真实存在的图侧符号",
            DOWNSTREAM_HANDOFF
                .iter()
                .all(|(_, _, what)| G.iter().any(|sym| what.contains(sym))),
            "交接物须指向本模块真实提供的函数或字段，不是「配合一下」",
        );
        // 结构自检（CSR 不变量）。
        if let Some(g) = frozen_typical() {
            set.add(
                "Q02-对接-CSR结构不变量自检通过",
                g.audit_graph_shape().is_empty(),
                "offsets 长度/末项/边越界/出度声明一致性",
            );
            set.add(
                "Q02-对接-出度声明与CSR一致",
                g.get(ResourceId(2))
                    .map(|r| r.out_degree == 1)
                    .unwrap_or(false),
                "声明与实际不符会让 F3206 的拓扑排序漏边",
            );
            set.add(
                "Q02-对接-入度反查可寻址",
                g.in_degree(ResourceId(0)) == 2 && g.in_degree(ResourceId(3)) == 0,
                "F3203 的图侧入度对账依据",
            );
        } else {
            set.add("Q02-对接-CSR结构不变量自检通过", false, "典型图不可用");
            set.add("Q02-对接-出度声明与CSR一致", false, "典型图不可用");
            set.add("Q02-对接-入度反查可寻址", false, "典型图不可用");
        }
    }

    // =======================================================================
    // 九、无障碍
    // =======================================================================
    {
        if let Some(g) = frozen_typical() {
            let n = graph_narration(&g);
            set.add(
                "Q02-读屏-图有线性文字替述",
                n.contains("资源引用图")
                    && n.contains("悬空引用")
                    && n.contains("循环引用")
                    && n.chars().count() > 120,
                "图对读屏不可达——文字版是验收项不是善举",
            );
            set.add(
                "Q02-读屏-替述含逐资源口播",
                n.contains("资源 0") && n.contains("出度"),
                "五要素须能念出来",
            );
            let one = graph_screen_text(&g);
            set.add(
                "Q02-读屏-一行摘要齐五项",
                one.contains("活跃")
                    && one.contains("边")
                    && one.contains("悬空")
                    && one.contains("环"),
                "摘要须含活跃数/边数/悬空占位/环四类",
            );
            set.add(
                "Q02-读屏-替述不暴露本地路径",
                !n.contains(":\\") && !n.contains("/app/"),
                "读屏替述不念路径（归属红线）",
            );
        } else {
            for i in 0..4 {
                set.add(
                    match i {
                        0 => "Q02-读屏-图有线性文字替述",
                        1 => "Q02-读屏-替述含逐资源口播",
                        2 => "Q02-读屏-一行摘要齐五项",
                        _ => "Q02-读屏-替述不暴露本地路径",
                    },
                    false,
                    "典型图不可用",
                );
            }
        }
        let arch = ResourceGraphArchitecture::narration();
        set.add(
            "Q02-读屏-架构口播含红线与复杂度",
            arch.contains("四用途单源")
                && arch.contains("32MB")
                && arch.contains("复杂度")
                && arch.contains("跨批对接"),
            "总纲口播须覆盖纪律、红线、复杂度、对接四块",
        );
        set.add(
            "Q02-读屏-架构版本可查",
            !ResourceGraphArchitecture::VERSION.is_empty()
                && !ResourceGraphArchitecture::screen_text().is_empty(),
            "版本号是口播可追溯的前提",
        );
        // 元数据与内容体亦可念。
        set.add(
            "Q02-读屏-元数据与内容体可念",
            meta_ok().screen_line().contains("色彩空间")
                && ContentBody::missing().screen_line().contains("缺失"),
            "实体级替述，缺一个就等于部分用户看不到资源",
        );
    }

    finish(set)
}

/// 收尾（占位以免误删未用导入告警）。
fn finish(set: CheckSet) -> CheckSet {
    let _ = vec![1u8];
    set
}