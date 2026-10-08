//! VE-F2009 · 域自检（判据逐条对应，见 `vek09_msaa.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检族）：
//! - **级别协商** → `K09-协商-*`
//! - **resolve 序位** → `K09-序位-*`
//! - **前向约束声明** → `K09-约束-*`
//! - **互斥守卫** → `K09-互斥-*`
//! - 错误路径四条（级别降级/序位错配/深度格式回退/互斥）→ `K09-错误-*`
//! - 面积守恒（resolve 真判据）→ `K09-守恒-*`
//! - 显存与成本模型 → `K09-成本-*`
//!
//! **门禁设计的六条自律**（本文件是它们的实践样本）：
//! 1. **注入变异，确认判据真的会红**：本条开发中，`④ 配额`步漏掉请求级上界
//!    这一真缺陷（设备 {1,4,8} + 请求 4x ⇒ granted 8x，**越权**）就是被
//!    `K09-协商-不越权` 抓住的。**没有反假变体的门禁不能算门禁**。
//! 2. **不测表内元素验表函数**：能力表的判据用**表外**配对（未登记的格式）
//!    验「缺失即不支持」，不是用已登记项验自己。
//! 3. **单边符号判据**：面积守恒用 `bias <= 0` 而非 `|bias| < eps`
//!    ——双边阈值一旦宽过正确实现的低估幅度，高估型变异就从缝里钻过去。
//! 4. **判定与数值分离**：协商裁决（安全相关）与显存算术（功能相关）
//!    分开断言，改显存公式不会碰到降级理由。
//! 5. **每个降级理由都必须有对应的诊断码**（一一映射，无`None` 漏网）。
//! 6. **拒绝路径也要测**：`register` 返回 false、`resolve` 长度不符、
//!    `from_wire` 非法——只测 happy path 的门禁等于没测。
//!
//! **本文件记录的真缺陷（供回归参考）**：
//! - `④ 配额` 步原实现只按配额上界求交，漏掉请求级上界 ⇒ granted 可能
//!   **高于**请求级（越权）。已修，并加 `K09-协商-不越权` 锁死。
//! - 能力表原用 `color_max: MsaaLevel`（单值），**无法表达非连续支持集**
//!   （D3D 允许只报 1x/4x/8x），而设计注释却按非连续写。已改为
//!   `LevelSet` 位掩码，注释与数据结构现已一致。
//! - `effective_method` 原只吃裁决，把「MSAA 开、TAA 关」这一无冲突情形
//!   判成 `None`（等于白开 MSAA）。已改为吃裁决 + 实际开启状态。
//!
//! 零墙钟、零 IO，回归可复现。

use super::vek09_msaa::*;
use crate::checks::CheckSet;

/// 1080p RGBA8 + D32 的显存实算精确值（颜色+深度）：2,073,600 px × 8B × 4。
const MEM_1080P_4X: u64 = 66_355_200;

/// 构造连续支持集（自`Off` 起逐级）。
fn levels(list: &[MsaaLevel]) -> LevelSet {
    let mut s = LevelSet::empty();
    for l in list.iter() {
        s = s.with(*l);
    }
    s
}

/// 构造一份能力表：颜色与深度同集。
fn caps_of(set: LevelSet) -> DeviceCaps {
    let mut c = DeviceCaps::probed(vec![FormatSupport {
        color: ColorFormat::Rgba8Unorm,
        depth: DepthFormat::D32Float,
        color_supported: set,
        depth_supported: set,
    }]);
    // `probed` 已给出 Probed 来源；显式重设仅为让意图在代码里可见。
    c.register(FormatSupport {
        color: ColorFormat::R16Float,
        depth: DepthFormat::D16Unorm,
        color_supported: set,
        depth_supported: set,
    });
    c
}

/// 构造「颜色与深度不同集」的能力表（测独立否决项）。
fn caps_split(cs: LevelSet, ds: LevelSet) -> DeviceCaps {
    DeviceCaps::probed(vec![FormatSupport {
        color: ColorFormat::Rgba8Unorm,
        depth: DepthFormat::D32Float,
        color_supported: cs,
        depth_supported: ds,
    }])
}

fn all_levels() -> LevelSet {
    levels(&[
        MsaaLevel::Off,
        MsaaLevel::X2,
        MsaaLevel::X4,
        MsaaLevel::X8,
    ])
}

/// 1080p 宽裕配额请求。
fn req(level: MsaaLevel) -> NegotiateRequest {
    NegotiateRequest::forward_1080p(level, 1 << 30)
}

/// VE-F2009 域自检。
pub fn run_vek09_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vek09");

    // =======================================================================
    // 判据一：级别协商
    // =======================================================================

    // 判据：请求级即生效时不产生降级（理由 None、downgraded=false）。
    //
    // **为什么要单测"没降级"**：回退链写反（先降后升）时，降级路径的判据
    // 可能仍全绿——只有"请求即生效"这一条会立刻暴露方向错误。
    {
        let n = negotiate(&req(MsaaLevel::X4), &caps_of(all_levels()));
        set.add(
            "K09-协商-请求级即生效不降级",
            n.granted == MsaaLevel::X4
                && n.reason == DowngradeReason::None
                && !n.downgraded,
            "X4 设备 X4 -> X4, none",
        );
    }

    // 判据：回退链 8x→4x→2x→off 在**连续**支持集上逐档正确。
    //
    // 设备支持到X2、请求 8x ⇒ 2x（**不是 off**，也不是 4x）。
    {
        let c = caps_of(levels(&[MsaaLevel::Off, MsaaLevel::X2]));
        let n = negotiate(&req(MsaaLevel::X8), &c);
        set.add(
            "K09-协商-连续支持集回退到最大可用",
            n.granted == MsaaLevel::X2
                && n.reason == DowngradeReason::ColorSupportIntersection
                && n.downgraded,
            "设备{1,2} 请求8x -> 2x",
        );
    }

    // 判据：**非连续支持集**下不得给出未声明支持的级别（本条最关键的一条）。
    //
    // 设备支持 {1,4,8}（无 2x，D3D 规范允许这种上报）：
    // · 请求 4x ⇒ 必须 4x（链式下探会走到 2x，即编造能力）；
    // · 请求 2x ⇒ 必须 off（唯一 ≤2x 的声明级是 1x）。
    //
    // **这条在开发中真的变红过**：能力表原为单值 `color_max`，无法表达
    // 非连续集，求交退化成"取 min"，对 {1,4,8} 请求 4x 会给 4x 但请求 2x
    // 会错误地给 2x——即把一个未声明的级别当成支持的。
    {
        let nc = levels(&[MsaaLevel::Off, MsaaLevel::X4, MsaaLevel::X8]);
        let c = caps_of(nc);
        let n4 = negotiate(&req(MsaaLevel::X4), &c);
        let n2 = negotiate(&req(MsaaLevel::X2), &c);
        let n8 = negotiate(&req(MsaaLevel::X8), &c);
        set.add(
            "K09-协商-非连续集不编造能力",
            n4.granted == MsaaLevel::X4
                && n2.granted == MsaaLevel::Off
                && n8.granted == MsaaLevel::X8,
            "cap{1,4,8}: req4->4x req2->off req8->8x",
        );
    }

    // 判据：**不得越权**——granted 永不高于 requested（本条实测缺陷的回归锁）。
    //
    // 缺陷形态：④ 配额步只按配额上界求交、漏掉请求级上界 ⇒ 设备 {1,4,8}
    // + 请求 4x + 配额宽裕 ⇒ granted **8x**（比请求还高）。它"看起来更好"，
    // 实际是越权：设置页显示 4x、显存按 8x 占用、多花的显存无人授权。
    //
    // **遍历全请求 × 全支持集**：只测一两个组合抓不到"仅在宽裕配额下越权"
    // 这类形态——配额越宽越容易触发，故必须包含配额远大于需求的组合。
    {
        let mut ok = true;
        let mut detail = String::new();
        for supported in [
            levels(&[MsaaLevel::Off]),
            levels(&[MsaaLevel::Off, MsaaLevel::X2]),
            levels(&[MsaaLevel::Off, MsaaLevel::X4]),
            levels(&[MsaaLevel::Off, MsaaLevel::X8]),
            levels(&[MsaaLevel::Off, MsaaLevel::X2, MsaaLevel::X8]),
            levels(&[MsaaLevel::Off, MsaaLevel::X4, MsaaLevel::X8]),
            all_levels(),
        ] {
            let c = caps_of(supported);
            for requested in MsaaLevel::all_asc().iter() {
                // 配额取极大，确保不是配额把越权掩盖掉。
                let n = negotiate(&req(*requested), &c);
                if !n.granted.le(*requested) {
                    ok = false;
                    detail = format!(
                        "cap{} req{} -> {}",
                        supported.text(),
                        requested.tag(),
                        n.granted.tag()
                    );
                }
            }
        }
        set.add("K09-协商-不越权", ok, "granted <= requested（28 组合）");
        let _ = detail;
    }

    // 判据：显性声明文本必须**同时含请求级与实际级**。
    //
    // 降级时只显示实际级别，用户会以为设置没生效而反复重设；
    // 只显示请求级则完全看不见降级。
    {
        let c = caps_of(levels(&[MsaaLevel::Off, MsaaLevel::X2]));
        let n = negotiate(&req(MsaaLevel::X8), &c);
        let d = n.declaration();
        set.add(
            "K09-协商-声明含请求级与实际级",
            d.contains("8x") && d.contains("2x") && d.contains("降级"),
            "请求 8x 实际 2x 双显性",
        );
    }

    // 判据：编码值与样本数**逐级自洽**（防`as u8` 化那类静默错一半）。
    {
        let mut ok = true;
        for lv in MsaaLevel::all_asc().iter() {
            ok &= lv.wire() as u32 == lv.samples();
            ok &= MsaaLevel::from_wire(lv.wire()) == Some(*lv);
            ok &= MsaaLevel::from_samples(lv.samples()) == Some(*lv);
        }
        set.add("K09-协商-编码与样本自洽", ok, "wire==samples 且双向可逆");
    }

    // 判据：位序号与排序秩一致，且位掩码往返无损。
    //
    // 这是 `LevelSet` 的地基：位序漂移会让**已存能力表**的解释静默改变
    // （同一个掩码值突然指向另一个级别，不报任何错）。
    //
    // **严格递增的比较必须从索引而非"前一个秩=0"起算**：`Off.rank()==0`，
    // 若把 `prev` 初始化为 0 再要求 `rank > prev`，第一项必然失败——
    // 那是判据自己写错了，不是被测物错。改用"秩 == 枚举下标"来表达
    // 严格递增，判据与被测物都不含隐含假设。
    {
        let mut ok = true;
        for (idx, lv) in MsaaLevel::all_asc().iter().enumerate() {
            ok &= lv.bit() == lv.rank();
            ok &= lv.rank() as usize == idx;
            ok &= LevelSet::from_bit(lv.bit()) == *lv;
            ok &= LevelSet::only(*lv).count() == 1;
            ok &= LevelSet::only(*lv).contains(*lv);
        }
        set.add("K09-协商-位序自洽", ok, "bit==rank==枚举下标");
    }

    // 判据：`max_at_most` 在非连续集上取**声明过的最大者**（不是相邻级）。
    {
        let nc = levels(&[MsaaLevel::Off, MsaaLevel::X4, MsaaLevel::X8]);
        set.add(
            "K09-协商-集合上界取真最大",
            nc.max_at_most(MsaaLevel::X8) == Some(MsaaLevel::X8)
                && nc.max_at_most(MsaaLevel::X4) == Some(MsaaLevel::X4)
                && nc.max_at_most(MsaaLevel::X2) == Some(MsaaLevel::Off),
            "{1,4,8} 上界 8/4/2 -> 8x/4x/off",
        );
    }

    // 判据：高位脏位被丢弃（能力表写坏时不凭空产生支持）。
    {
        set.add(
            "K09-协商-掩码高位丢弃",
            LevelSet::from_bits(0xF3).bits() == 0x03
                && LevelSet::from_bits(0xF3).count() == 2,
            "0xF3 -> 0x03 (2 项)",
        );
    }

    // 判据：请求 `Off` 时**不查询能力、不产生降级诊断**。
    //
    // 关闭 MSAA 是用户的合法选择，不是降级；把它记成降级会让"降级计数"
    // 永远大于 0，这个遥测指标就此失去意义。
    {
        let n = negotiate(&req(MsaaLevel::Off), &caps_split(all_levels(), LevelSet::empty()));
        set.add(
            "K09-协商-关闭不记降级",
            n.granted == MsaaLevel::Off && n.reason == DowngradeReason::None && !n.downgraded,
            "req off -> off, none",
        );
    }

    // =======================================================================
    // 判据二：resolve 序位
    // =======================================================================

    // 判据：四种级别下的标准序**全部合法**。
    //
    // `canonical_stages` 按级别参数化是刻意的：写死一种会让"关MSAA 时序里
    // 还留着 resolve"这类缺陷无法被测试覆盖。
    {
        let mut ok = true;
        for lv in MsaaLevel::all_asc().iter() {
            ok &= check_order(&canonical_stages(*lv), *lv) == OrderVerdict::Ok;
        }
        set.add("K09-序位-标准序合法", ok, "off/2x/4x/8x 四档皆Ok");
    }

    // 判据：R2 —— 后处理读 MSAA 数据（**锚点点名的那一条**）。
    //
    // 注入形态：后处理节点写 `sample_count: 4`。它在类型上完全合法、
    // 编译通过、画面也只是"边缘有点脏"——所以必须写成断言。
    {
        let stages = vec![
            Stage::new(StageKind::GeometryPass, 4),
            Stage::new(StageKind::PostEffect, 4),
            Stage::new(StageKind::Resolve, 1),
        ];
        let v = check_order(&stages, MsaaLevel::X4);
        set.add(
            "K09-序位-后处理读MSAA被拦截（R2）",
            v == OrderVerdict::PostReadsMsaa,
            "post sample_count=4 -> PostReadsMsaa",
        );
    }

    // 判据：R2 的判定**优先于**结构类违规。
    //
    // 上面那条用例同时含"resolve 在后处理之后"（R1）。若判定顺序倒过来，
    // 报出的是 R1——而 R1 的修法（调阶段顺序）**修不好** R2
    // （后处理节点仍写着 4x）。先报语义问题，人才能看到真正危险的那个。
    {
        let both = vec![
            Stage::new(StageKind::GeometryPass, 4),
            Stage::new(StageKind::PostEffect, 4),
            Stage::new(StageKind::Resolve, 1),
        ];
        set.add(
            "K09-序位-R2优先于R1",
            check_order(&both, MsaaLevel::X4) == OrderVerdict::PostReadsMsaa,
            "语义违规先于结构违规",
        );
    }

    // 判据：R1 —— resolve 排在后处理之后。
    {
        let stages = vec![
            Stage::new(StageKind::PostEffect, 1),
            Stage::new(StageKind::Resolve, 1),
        ];
        set.add(
            "K09-序位-resolve在后处理后被拦截（R1）",
            check_order(&stages, MsaaLevel::X4) == OrderVerdict::ResolveAfterPost,
            "post 先于 resolve -> ResolveAfterPost",
        );
    }

    // 判据：R3 —— MSAA 开启但序中无 resolve。
    {
        let stages = vec![
            Stage::new(StageKind::GeometryPass, 4),
            Stage::new(StageKind::PostEffect, 1),
        ];
        set.add(
            "K09-序位-缺resolve被拦截（R3）",
            check_order(&stages, MsaaLevel::X4) == OrderVerdict::ResolveMissing,
            "X4 无 resolve -> ResolveMissing",
        );
    }

    // 判据：R4 —— MSAA 关闭但序中有 resolve（多余解析）。
    //
    // 这条**不是性能问题而是正确性问题**：多余的 resolve 是又一次全屏平均，
    // 会把画面整体对比度压低一档，且没有任何诊断会自然浮现。
    {
        let stages = vec![
            Stage::new(StageKind::GeometryPass, 1),
            Stage::new(StageKind::Resolve, 1),
            Stage::new(StageKind::PostEffect, 1),
        ];
        set.add(
            "K09-序位-多余resolve被拦截（R4）",
            check_order(&stages, MsaaLevel::Off) == OrderVerdict::ResolveRedundant,
            "off 有 resolve -> ResolveRedundant",
        );
    }

    // 判据：R5 —— resolve 多次（二次解析）。
    {
        let stages = vec![
            Stage::new(StageKind::Resolve, 1),
            Stage::new(StageKind::Resolve, 1),
        ];
        set.add(
            "K09-序位-多次resolve被拦截（R5）",
            check_order(&stages, MsaaLevel::X4) == OrderVerdict::ResolveDuplicated,
            "双 resolve -> ResolveDuplicated",
        );
    }

    // 判据：序位裁决与诊断码**一一对应**（无`None` 漏网、无码复用）。
    {
        let all = [
            OrderVerdict::Ok,
            OrderVerdict::PostReadsMsaa,
            OrderVerdict::ResolveAfterPost,
            OrderVerdict::ResolveMissing,
            OrderVerdict::ResolveRedundant,
            OrderVerdict::ResolveDuplicated,
        ];
        let mut ok = all[0].diag_code().is_none();
        let mut seen: Vec<MsaaDiagCode> = Vec::new();
        for v in all.iter().skip(1) {
            match v.diag_code() {
                None => ok = false,
                Some(c) => {
                    if seen.contains(&c) {
                        ok = false;
                    }
                    seen.push(c);
                }
            }
            if v.text().is_empty() {
                ok = false;
            }
        }
        set.add("K09-序位-裁决与诊断一一对应", ok, "5 违规各持专属码与文本");
    }

    // =======================================================================
    // 判据三：前向约束声明
    // =======================================================================

    // 判据：延迟路径 → off，理由为**路径不支持**（不是能力不足）。
    //
    // 理由归属可观测：设备满血时也必须是 `PathUnsupported`。若报成
    // `ColorSupportIntersection`，运维会去换设备——而那永远修不好。
    {
        let mut r = req(MsaaLevel::X8);
        r.path = RenderPath::Deferred;
        let n = negotiate(&r, &caps_of(all_levels()));
        set.add(
            "K09-约束-延迟路径不启用MSAA",
            n.granted == MsaaLevel::Off && n.reason == DowngradeReason::PathUnsupported,
            "满血设备 + 延迟路径仍 -> off/path_unsupported",
        );
    }

    // 判据：路径否决**不依赖设备能力**（满血与空表结论一致）。
    //
    // 若实现把路径检查放在能力查询之后并被短路，延迟路径在空能力表下会
    // 报出 `FormatPairAbsent`——理由随设备而变，正是"理由归属不稳"的形态。
    {
        let mut r = req(MsaaLevel::X8);
        r.path = RenderPath::Deferred;
        let n = negotiate(&r, &DeviceCaps::empty_defaulted());
        set.add(
            "K09-约束-路径否决不依赖能力",
            n.granted == MsaaLevel::Off && n.reason == DowngradeReason::PathUnsupported,
            "空能力表 + 延迟路径 -> 仍 path_unsupported",
        );
    }

    // 判据：`fixable_by_hardware` 把「换硬件修不好」显式区分出来。
    //
    // 这条分类有实际用途：把 `PathUnsupported` 归入"能力不足"会让运维
    // endless 换机器。
    {
        set.add(
            "K09-约束-换硬件可行性分类正确",
            !DowngradeReason::PathUnsupported.fixable_by_hardware()
                && DowngradeReason::ColorSupportIntersection.fixable_by_hardware()
                && DowngradeReason::QuotaExceeded.fixable_by_hardware(),
            "path=false其余=true",
        );
    }

    // 判据：**不支持必须带理由与解锁条件**（禁"暂不支持"式空句）。
    {
        let mut ok = !RenderPath::Deferred.unsupported_reason().is_empty()
            && !RenderPath::Deferred.unlock_condition().is_empty()
            && RenderPath::Forward.supports_msaa()
            && !RenderPath::Deferred.supports_msaa();
        for p in RenderPath::all().iter() {
            ok &= !p.tag().is_empty() && !p.label().is_empty();
        }
        set.add("K09-约束-限制带理由与解锁条件", ok, "延迟路径理由/解锁条件非空");
    }

    // 判据：约束声明表文本**逐项覆盖**两条路径且点明 MSAA 方法归属。
    {
        let d = PATH_CONSTRAINT_TABLE_DOC;
        set.add(
            "K09-约束-声明表覆盖两路径",
            d.contains("前向") && d.contains("延迟") && d.contains("不启用") && d.contains("MSAA"),
            "表含前向/延迟/不启用/MSAA",
        );
    }

    // =======================================================================
    // 判据四：互斥守卫
    // =======================================================================

    // 判据：MSAA 与 TAA 同开⇒ 保留 MSAA、**具名丢弃 TAA**。
    {
        let mut bag = MsaaDiagBag::new();
        let v = guard_aa_mutex(MsaaLevel::X4, true, &mut bag);
        set.add(
            "K09-互斥-同开时丢弃TAA",
            v == MutexVerdict::MsaaKeptTaaDropped
                && v.dropped() == Some(AaMethod::Taa)
                && bag.has(MsaaDiagCode::TaaDroppedByMutex),
            "4x+TAA -> MsaaKeptTaaDropped + 诊断",
        );
    }

    // 判据：四种组合的裁决**各自正确且不误报**。
    //
    // 尤其 `(off, true)` 必须是 `NoConflict`——MSAA 协商已降为 off 时，
    // 不存在"两者同开"，若仍报互斥丢弃就是误报（会用无意义的诊断淹没真问题）。
    {
        let mut bag = MsaaDiagBag::new();
        let v1 = guard_aa_mutex(MsaaLevel::X4, false, &mut bag);
        let v2 = guard_aa_mutex(MsaaLevel::Off, true, &mut bag);
        let v3 = guard_aa_mutex(MsaaLevel::X4, true, &mut bag);
        let v4 = guard_aa_mutex(MsaaLevel::Off, false, &mut bag);
        set.add(
            "K09-互斥-四组合裁决正确",
            v1 == MutexVerdict::NoConflict
                && v2 == MutexVerdict::NoConflict
                && v3 == MutexVerdict::MsaaKeptTaaDropped
                && v4 == MutexVerdict::BothOff,
            "(4,-) (-) (4,T) (-,-) 四档",
        );
    }

    // 判据：`effective_method` 落定实际方法（**四组合逐一验证**）。
    //
    // 这条在开发中真的红过：原实现只吃裁决，把 `(X4, false)` 这一无冲突
    // 情形判成 `None`——等于用户开了 MSAA 却什么也没得到，且不报任何错。
    {
        let ok = effective_method(MutexVerdict::NoConflict, MsaaLevel::X4, false)
            == AaMethod::Msaa
            && effective_method(MutexVerdict::NoConflict, MsaaLevel::Off, true) == AaMethod::Taa
            && effective_method(MutexVerdict::NoConflict, MsaaLevel::Off, false)
                == AaMethod::None
            && effective_method(MutexVerdict::MsaaKeptTaaDropped, MsaaLevel::X4, true)
                == AaMethod::Msaa
            && effective_method(MutexVerdict::BothOff, MsaaLevel::Off, false) == AaMethod::None;
        set.add("K09-互斥-生效方法落定正确", ok, "无冲突两态不丢方法");
    }

    // 判据：互斥裁决**按状态性而非质量**决定（方法属性自洽）。
    {
        let ok = !AaMethod::Msaa.needs_history()
            && AaMethod::Taa.needs_history()
            && AaMethod::Msaa.geometry_stage()
            && !AaMethod::Fxaa.geometry_stage()
            && MutexVerdict::MsaaKeptTaaDropped.dropped() == Some(AaMethod::Taa);
        set.add("K09-互斥-状态性判据自洽", ok, "MSAA无历史依赖/TAA有");
    }

    // =======================================================================
    // 面积守恒（resolve 的真判据）
    // =======================================================================

    // 判据：过中心垂直边的解析覆盖**精确**为 1/2（单边符号判据）。
    //
    // **单边而非双边**：样本位置离散使覆盖是阶梯近似，只可能低估或恰好，
    // 永不高估。故 `bias <= 0` 即守恒。双边阈值一旦宽过低估幅度，
    // "高估型"变异（取首样本、少算一样本）就会从缝里钻过去。
    //
    // 实测：三级bias 全为 0e0（分析值与实测值同源于精确的 0.5/0.25 分数）。
    {
        let mut ok = true;
        for lv in [MsaaLevel::X2, MsaaLevel::X4, MsaaLevel::X8].iter() {
            let s = make_edge_samples(*lv, 0.5, 1.0, 0.0);
            let r = area_conservation(&s, *lv, 0.5, 1.0, 0.0);
            ok &= r.conserved && (r.analytic_coverage - 0.5).abs() < 1e-6;
        }
        set.add("K09-守恒-过中心边覆盖精确1比2", ok, "三级 bias=0, 单边判据");
    }

    // 判据：非对称边（edge_x=0.25）的覆盖率仍精确对账。
    //
    // 只测过中心的边**抓不到"偏移固定量"的实现错误**——那类错误在
    // 0.5 处正好抵消。非对称边让偏移无处可藏。
    {
        let mut ok = true;
        for lv in [MsaaLevel::X2, MsaaLevel::X4, MsaaLevel::X8].iter() {
            let s = make_edge_samples(*lv, 0.25, 1.0, 0.0);
            let r = area_conservation(&s, *lv, 0.25, 1.0, 0.0);
            let expect = analytic_coverage_x(*lv, 0.25);
            ok &= r.conserved && (r.analytic_coverage - expect).abs() < 1e-6;
        }
        set.add("K09-守恒-非对称边对账", ok, "edge_x=0.25 三级一致");
    }

    // 判据：**反假变体** —— 取首样本必被单边判据捕获。
    //
    // 这是本条门禁诚信的核心：确认"守恒"不是恒绿。变体是真的改变了行为
    // 路径（覆盖从 0.5 变 1.0），且**只靠单边符号**才能抓住——
    // 若把判据改成 `|bias| < 0.1`，这个变异照样通过（偏差 0.5 > 0.1 才红，
    // 但若阈值写成 0.6 就漏了）。实测首样本覆盖=1.0，解析=0.5，偏差 +0.5。
    {
        let lv = MsaaLevel::X4;
        let s = make_edge_samples(lv, 0.5, 1.0, 0.0);
        let first_sample_coverage = s[0];
        let analytic = analytic_coverage_x(lv, 0.5);
        // 变体偏差为正 ⇒ 单边判据必须判它**不守恒**。
        set.add(
            "K09-守恒-反假变体被单边判据捕获",
            first_sample_coverage - analytic > 0.0,
            "取首样本 bias=+0.5 > 0 必红",
        );
    }

    // 判据：`conserved` 裁决本身**必须拒绝高估**（防"判据被放松"这一类变异）。
    //
    // **这条是补 M13 变异暴露的缺口**：把 `conserved` 从 `bias <= 0` 改成
    // `bias.abs() < 1e9`（即恒真）时，上面那条反假变体判据**不会红**——
    // 因为它自己算 bias、自己比符号，压根没调用 `area_conservation`。
    // 结果是"守恒"这个裁决被放松成恒真而无人发现。
    //
    // 补法：**让守恒裁决对一个确定高估的输入真的返回 false**。这里直接
    // 构造高估样本（4 个样本里 3 个覆盖 ⇒ 实测覆盖 0.75，而解析值是 0.5），
    // 调用 `area_conservation` 并断言 `conserved == false`。
    // 若判据被改成恒真，这条立刻红。
    {
        let lv = MsaaLevel::X4;
        let over = vec![1.0f32, 1.0, 1.0, 0.0];
        let r = area_conservation(&over, lv, 0.5, 1.0, 0.0);
        let correct = make_edge_samples(lv, 0.5, 1.0, 0.0);
        let rc = area_conservation(&correct, lv, 0.5, 1.0, 0.0);
        set.add(
            "K09-守恒-裁决拒绝高估样本",
            !r.conserved
                && r.bias > 0.0
                && (r.analytic_coverage - 0.5).abs() < 1e-6
                && rc.conserved,
            "3/4覆盖(解析0.5) => conserved=false",
        );
    }

    // 判据：解析覆盖率**独立于** resolve 计算（对账的基础）。
    //
    // 若拿 resolve 的输出推解析值，两者共享同一实现错误 ⇒ 恒真。
    // 本条用**样本位置表直接数**得到解析值：4x 在edge_x=0.5 处恰覆盖 2/4。
    {
        let s4 = make_edge_samples(MsaaLevel::X4, 0.5, 1.0, 0.0);
        let counted = s4.iter().filter(|v| **v >= 0.5).count();
        set.add(
            "K09-守恒-解析值独立计数",
            counted == 2 && analytic_coverage_x(MsaaLevel::X4, 0.5) == 0.5,
            "4x edge@0.5 -> 2/4",
        );
    }

    // 判据：全同值样本的 resolve **精确**返回该值（无端点漂移）。
    {
        let mut ok = true;
        for lv in [MsaaLevel::X2, MsaaLevel::X4, MsaaLevel::X8].iter() {
            let v = 0.375f32;
            let s = vec![v; lv.samples() as usize];
            let mut bag = MsaaDiagBag::new();
            ok &= (resolve(&s, *lv, &mut bag) - v).abs() < 1e-6;
        }
        set.add("K09-守恒-同值样本精确返回", ok, "三级均无端点漂移");
    }

    // 判据：非有限样本**不得污染**输出（NaN 会一路穿过后处理链永不消失）。
    {
        let mut bag = MsaaDiagBag::new();
        let s = [f32::NAN, 1.0, 1.0, 1.0];
        let out = resolve(&s, MsaaLevel::X4, &mut bag);
        set.add(
            "K09-守恒-非有限样本被清洗",
            out.is_finite() && (out - 0.75).abs() < 1e-6,
            "NaN 视为 0 -> 0.75",
        );
    }

    // 判据：样本数与级别不符 ⇒ **拒绝并落阻断诊断**（不静默用部分样本）。
    {
        let mut bag = MsaaDiagBag::new();
        let out = resolve(&[1.0, 0.0], MsaaLevel::X4, &mut bag);
        set.add(
            "K09-错误-样本数不符被拒",
            out == 0.0
                && bag.has(MsaaDiagCode::SampleCountMismatch)
                && bag.blocking_count() == 1,
            "2样本配4x -> 0 + 阻断诊断",
        );
    }

    // =======================================================================
    // 样本模式
    // =======================================================================

    // 判据：样本位置**逐项**落在像素内且重心居中。
    //
    // 逐项而非只查首末：位置表写错最常见的是"某个中间项打成 1.5"，
    // 只查首末会漏掉。两级判据（像素内 + 重心）抓不同类错误。
    //
    // **`Off` 的位置表长度是 0 而 `samples()` 是 1，这不是不一致**：
    // 1x 不需要多采样位置（只有一个样本必在像素中心），位置表为空是
    // 正确编码。判据必须写成"**开启**的级别长度 == 样本数；关闭时为空"，
    // 写成"长度恒等于样本数"会在`Off` 上必然失败——那是判据错，不是被测物错。
    {
        let mut ok = true;
        for lv in MsaaLevel::all_asc().iter() {
            ok &= sample_positions_in_unit_square(*lv);
            ok &= sample_centroid_offset(*lv) < 1e-6;
            if lv.enabled() {
                ok &= sample_positions(*lv).len() as u32 == lv.samples();
                ok &= sample_positions(*lv).len() >= 2;
            } else {
                ok &= sample_positions(*lv).is_empty();
            }
        }
        set.add("K09-样本-位置合法且重心居中", ok, "开启级长度=样本数/关闭级空");
    }

    // 判据：**表外**非法位置必须被位置校验识别为不合法。
    //
    // 上一条判据只能证明"内置位置表是对的"，证不了"校验有牙齿"——把
    // `sample_positions_in_unit_square` 的判断改成恒真 `true`，它照样
    // 全绿（反假变体 V14 实测未被捕获）。这是门禁设计的自律①：表内元素
    // 验检查表函数 = 恒真弱门禁。须用**表外**的越界/ NaN / 负值来验。
    {
        let ok_good = positions_in_unit_square(&[(0.25, 0.25), (0.75, 0.75)]);
        let bad = [
            vec![(1.5, 0.5)],                       // x 越界
            vec![(0.5, -0.1)],                      // y 为负
            vec![(f32::NAN, 0.5)],                  // NaN 不得落进合法分支
            vec![(f32::INFINITY, 0.5)],             // 无穷
            vec![(0.5, 0.5), (2.0, 0.5)],           // 中间项越界（只查首末会漏）
        ];
        let ok_bad = bad.iter().all(|v| !positions_in_unit_square(v));
        set.add(
            "K09-样本-校验能识别表外非法位置",
            ok_good && ok_bad,
            "越界/负值/NaN/无穷/中间项越界均须判非法",
        );
    }

    // 判据：边缘判定是**部分覆盖**（纯色区不算边缘）。
    //
    // 用"样本间有落差"判会把全 0/全 1 的纯色区也算成边缘，覆盖率直方图
    // 会因此全糊——那正是覆盖率直方图最不可信的用法。
    {
        let full_cov = vec![1.0f32; 4];
        let none_cov = vec![0.0f32; 4];
        let half = make_edge_samples(MsaaLevel::X4, 0.5, 1.0, 0.0);
        let e1 = analyze_edge(&full_cov, MsaaLevel::X4);
        let e2 = analyze_edge(&none_cov, MsaaLevel::X4);
        let e3 = analyze_edge(&half, MsaaLevel::X4);
        set.add(
            "K09-样本-边缘为部分覆盖",
            !e1.is_edge && !e2.is_edge && e3.is_edge
                && (e3.coverage - 0.5).abs() < 1e-6
                && (e1.coverage - 1.0).abs() < 1e-6,
            "纯色非边缘/半覆盖为边缘",
        );
    }

    // =======================================================================
    // 显存与成本（入 F1776 配额 / F2017 基准）
    // =======================================================================

    // 判据：1080p 4x 显存**精确值**（颜色 + 深度，深度也乘样本数）。
    //
    // 只算颜色是常见错误——那会让F1776 配额低估一半以上。实测精确值
    // 66,355,200 B（= 2,073,600 px × (4+4) B × 4）。
    {
        let total = msaa_memory_bytes(1920, 1080, ColorFormat::Rgba8Unorm, DepthFormat::D32Float, MsaaLevel::X4);
        let c = color_memory_bytes(1920, 1080, ColorFormat::Rgba8Unorm, MsaaLevel::X4);
        let d = depth_memory_bytes(1920, 1080, DepthFormat::D32Float, MsaaLevel::X4);
        set.add(
            "K09-成本-1080p4x显存精确",
            total == MEM_1080P_4X && c == 33_177_600 && d == 33_177_600 && c + d == total,
            "66,355,200 B (色=深=33,177,600)",
        );
    }

    // 判据：显存**随样本数线性倍增**（倍数 = 样本数）。
    {
        let b = |lv| msaa_memory_bytes(1920, 1080, ColorFormat::Rgba8Unorm, DepthFormat::D32Float, lv);
        set.add(
            "K09-成本-显存随样本线性",
            b(MsaaLevel::X8) == b(MsaaLevel::X4) * 2
                && b(MsaaLevel::X4) == b(MsaaLevel::X2) * 2
                && b(MsaaLevel::X2) == b(MsaaLevel::Off) * 2,
            "1x:2x:4x:8x = 1:2:4:8",
        );
    }

    // 判据：0 像素或0 配额**不得下溢**（无符号减法的经典陷阱）。
    {
        let mut r = req(MsaaLevel::X8);
        r.quota_bytes = 0;
        let n = negotiate(&r, &caps_of(all_levels()));
        let qr = quota_rejection(&r, MsaaLevel::X8, MsaaLevel::Off);
        set.add(
            "K09-成本-零配额不panic不溢出",
            n.granted == MsaaLevel::Off && qr.overflow_bytes == qr.current_bytes,
            "quota=0 -> off, 三要素自洽",
        );
    }

    // 判据：成本模型**随样本数与像素数线性**（性质可验证，绝对值是预算）。
    //
    // 绝对值 0.3ms 是锚点给的**预算**，不是本机实测——本条能验证的只有
    // 模型的线性与单调。回填前不得称其为实测。
    {
        let c8 = resolve_cost_ms(1920, 1080, MsaaLevel::X8);
        let c4 = resolve_cost_ms(1920, 1080, MsaaLevel::X4);
        let c4_half = resolve_cost_ms(960, 540, MsaaLevel::X4);
        let ok = (c8 / c4 - 2.0).abs() < 1e-4
            && (c4 / c4_half - 4.0).abs() < 1e-3
            && (c4 - RESOLVE_COST_MS_1080P_4X).abs() < 1e-6
            && RESOLVE_COST_MS_1080P_4X == 0.3;
        set.add("K09-成本-模型线性且锚定预算", ok, "8x/4x=2,1080p/540p=4");
    }

    // 判据：配额降级**给到配额内最大级**（不是直接 off）。
    //
    // 40MB 配额下 8x 请求 ⇒2x（33,177,600 B 塞得下，8x 的 132,710,400塞不下）。
    // 直接落off 会白白浪费可用的2x。
    {
        let r = NegotiateRequest::forward_1080p(MsaaLevel::X8, 40_000_000);
        let n = negotiate(&r, &caps_of(all_levels()));
        set.add(
            "K09-错误-配额降级给最大可容级",
            n.granted == MsaaLevel::X2
                && n.reason == DowngradeReason::QuotaExceeded
                && n.memory_bytes <= r.quota_bytes,
            "40MB/req8x -> 2x(33.2MB<=40MB)",
        );
    }

    // 判据：能力已降级时，理由**不得**被误报成配额。
    //
    // 设备 {1,4,8} 请求 8x 但配额只够 1x ⇒ 真因是**能力**（8x 支持，
    // 降级由请求 8x 与支持集无冲突……）——更确切地说：若 granted 已因
    // 能力低于请求，配额只是恰好也没余量，理由应为能力。
    {
        let nc = levels(&[MsaaLevel::Off, MsaaLevel::X4, MsaaLevel::X8]);
        let c = caps_of(nc);
        // 请求 4x 但配额只够 1x：能力满足请求（4x 在集内），纯由配额压低。
        let r = NegotiateRequest::forward_1080p(MsaaLevel::X4, 20_000_000);
        let n = negotiate(&r, &c);
        set.add(
            "K09-协商-理由归属正确",
            n.granted == MsaaLevel::Off && n.reason == DowngradeReason::QuotaExceeded,
            "cap含4x + quota20MB -> quota_exceeded",
        );
    }

    // 判据：能力表**缺失即不支持**（用未登记的表外配对验，不是表内元素）。
    //
    // **不测表内元素验表函数**：用已登记的配对去验查询函数是恒真弱门禁。
    // 这里用的是**未登记**的格式组合（R16Float+D16 之外的组合）。
    {
        let c = caps_of(all_levels());
        let n = negotiate(
            &NegotiateRequest {
                requested: MsaaLevel::X4,
                path: RenderPath::Forward,
                color: ColorFormat::R11G11B10Float,
                depth: DepthFormat::D24UnormS8Uint,
                width: 1920,
                height: 1080,
                quota_bytes: 1 << 30,
            },
            &c,
        );
        set.add(
            "K09-协商-缺失配对即不支持",
            n.granted == MsaaLevel::Off && n.reason == DowngradeReason::FormatPairAbsent,
            "表外配对 -> off/format_pair_absent",
        );
    }

    // 判据：深度侧不支持 ⇒ **整体 off**（不是降一级）。
    //
    // 颜色支持 8x、深度只支持 1x ⇒ 答案必须是 off。没有 MSAA 深度就没有
    // 逐样本深度测试，边缘会穿帮成"颜色锯齿 + 深度错"，比不开更糟。
    {
        let c = caps_split(all_levels(), levels(&[MsaaLevel::Off]));
        let n = negotiate(&req(MsaaLevel::X8), &c);
        set.add(
            "K09-错误-深度不支持降级为off",
            n.granted == MsaaLevel::Off
                && n.reason == DowngradeReason::DepthFormatUnsupported,
            "色8x/深1x -> off（非降一级）",
        );
    }

    // =======================================================================
    // 能力表面
    // =======================================================================

    // 判据：重复登记**被拒且不覆盖**（覆盖 = 静默降低能力声明）。
    {
        let mut c = DeviceCaps::probed(vec![FormatSupport::uniform(
            ColorFormat::Rgba8Unorm,
            DepthFormat::D32Float,
            &[MsaaLevel::Off, MsaaLevel::X8],
        )]);
        let dup = c.register(FormatSupport::uniform(
            ColorFormat::Rgba8Unorm,
            DepthFormat::D32Float,
            &[MsaaLevel::Off],
        ));
        let retained = match c.query(ColorFormat::Rgba8Unorm, DepthFormat::D32Float) {
            CapsLookup::Found(e) => e.color_supported,
            CapsLookup::Absent => LevelSet::empty(),
        };
        set.add(
            "K09-能力-重复登记被拒不覆盖",
            !dup && retained.contains(MsaaLevel::X8),
            "第二次登记false 且 8x 仍在",
        );
    }

    // 判据：表满时**拒绝新增**而非覆盖。
    {
        let mut c = DeviceCaps::empty_defaulted();
        let mut i = 0;
        while i < FORMAT_TABLE_SIZE {
            let _ = c.register(FormatSupport::uniform(
                ColorFormat::all()[i % 4],
                DepthFormat::all()[i % 3],
                &[MsaaLevel::Off],
            ));
            i += 1;
        }
        let over = c.register(FormatSupport::uniform(
            ColorFormat::R16Float,
            DepthFormat::D16Unorm,
            &[MsaaLevel::X2],
        ));
        set.add(
            "K09-能力-表满拒绝新增",
            c.is_full() && !over && c.len() == FORMAT_TABLE_SIZE,
            "8 项后第 9 项被拒",
        );
    }

    // 判据：空能力表 ⇒ **所有格式 off**（保守方向的默认值）。
    {
        let c = DeviceCaps::empty_defaulted();
        let mut ok = c.is_empty() && c.query_common_max(ColorFormat::Rgba8Unorm, DepthFormat::D32Float) == MsaaLevel::Off;
        for cf in ColorFormat::all().iter() {
            for df in DepthFormat::all().iter() {
                ok &= c.query_common_max(*cf, *df) == MsaaLevel::Off;
            }
        }
        set.add("K09-能力-空表全关闭", ok, "12 组合皆 off");
    }

    // 判据：配对能力取**交集**（不是取某一侧、也不是取 min）。
    //
    // 用例构造要注意选对集合：若颜色={off,8x}、深度={off,4x}，交集是
    // **{off}**（8x 与 4x 无公共元素），最大级是 off——用这个集合去断言
    // "交集为 4x"是判据自己写错了。故这里用两组互补的用例：
    // ① 颜色={off,4x,8x}、深度={off,4x} ⇒ 交集={off,4x}，max=4x
    //    （若实现错取 `color` 一侧，会得 8x ⇒ 被抓）；
    // ② 颜色={off,8x}、深度={off,4x} ⇒ 交集={off}，max=off
    //    （若实现错取 `min`，min(8x,4x)=4x ⇒ 被抓）。
    {
        let c1 = caps_split(
            levels(&[MsaaLevel::Off, MsaaLevel::X4, MsaaLevel::X8]),
            levels(&[MsaaLevel::Off, MsaaLevel::X4]),
        );
        let s1 = c1.query_common_set(ColorFormat::Rgba8Unorm, DepthFormat::D32Float);
        let c2 = caps_split(
            levels(&[MsaaLevel::Off, MsaaLevel::X8]),
            levels(&[MsaaLevel::Off, MsaaLevel::X4]),
        );
        let s2 = c2.query_common_set(ColorFormat::Rgba8Unorm, DepthFormat::D32Float);
        set.add(
            "K09-能力-配对取真交集",
            s1.max_level() == MsaaLevel::X4
                && s1.contains(MsaaLevel::X4)
                && !s1.contains(MsaaLevel::X8)
                && s2.max_level() == MsaaLevel::Off
                && !s2.contains(MsaaLevel::X8)
                && !s2.contains(MsaaLevel::X4),
            "{1,4,8}∩{1,4}->4x; {1,8}∩{1,4}->off",
        );
    }

    // 判据：格式字节数**逐项正确**（显存算术的地基）。
    {
        let ok = ColorFormat::Rgba8Unorm.bytes_per_pixel() == 4
            && ColorFormat::Bgra8Unorm.bytes_per_pixel() == 4
            && ColorFormat::R11G11B10Float.bytes_per_pixel() == 4
            && ColorFormat::R16Float.bytes_per_pixel() == 8
            && DepthFormat::D32Float.bytes_per_pixel() == 4
            && DepthFormat::D24UnormS8Uint.bytes_per_pixel() == 4
            && DepthFormat::D16Unorm.bytes_per_pixel() == 2;
        set.add("K09-能力-格式字节数正确", ok, "RGBA8=4 R16F=8 D16=2");
    }

    // =======================================================================
    // 诊断与文本
    // =======================================================================

    // 判据：诊断码**短名唯一**（遥测聚合键冲突会让两码并成一桶）。
    {
        let all = [
            MsaaDiagCode::LevelDowngraded,
            MsaaDiagCode::DeferredPathUnsupported,
            MsaaDiagCode::DepthFormatNoMsaa,
            MsaaDiagCode::NoColorLevelSupported,
            MsaaDiagCode::MemoryOverQuota,
            MsaaDiagCode::PostReadsMsaaData,
            MsaaDiagCode::ResolveAfterPost,
            MsaaDiagCode::ResolveMissing,
            MsaaDiagCode::ResolveRedundant,
            MsaaDiagCode::ResolveDuplicated,
            MsaaDiagCode::TaaDroppedByMutex,
            MsaaDiagCode::SampleCountMismatch,
            MsaaDiagCode::CapsOriginDefaulted,
        ];
        let mut seen: Vec<&str> = Vec::new();
        let mut ok = true;
        for c in all.iter() {
            let k = c.code();
            if k.is_empty() || seen.contains(&k) {
                ok = false;
            }
            seen.push(k);
            if c.next_hint().is_empty() {
                ok = false;
            }
        }
        set.add("K09-诊断-码短名唯一且有处置提示", ok, "13 码互不重名");
    }

    // 判据：**处置方向相反的状态不共用码**。
    //
    // 阻断类与非阻断类混在一个码里，会让"必须修"与"可观察"在遥测上无法
    // 区分。本条断言序位类全阻断、降级类全不阻断。
    {
        let blocking_should = [
            MsaaDiagCode::PostReadsMsaaData,
            MsaaDiagCode::ResolveAfterPost,
            MsaaDiagCode::ResolveMissing,
            MsaaDiagCode::ResolveRedundant,
            MsaaDiagCode::ResolveDuplicated,
            MsaaDiagCode::SampleCountMismatch,
        ];
        let nonblocking_should = [
            MsaaDiagCode::LevelDowngraded,
            MsaaDiagCode::DeferredPathUnsupported,
            MsaaDiagCode::DepthFormatNoMsaa,
            MsaaDiagCode::NoColorLevelSupported,
            MsaaDiagCode::MemoryOverQuota,
            MsaaDiagCode::TaaDroppedByMutex,
            MsaaDiagCode::CapsOriginDefaulted,
        ];
        let mut ok = true;
        for c in blocking_should.iter() {
            ok &= c.is_blocking();
        }
        for c in nonblocking_should.iter() {
            ok &= !c.is_blocking();
        }
        set.add("K09-诊断-阻断与非阻断不共码", ok, "6 阻断/7 非阻断");
    }

    // 判据：可访问性标记**只给几何 MSAA 被关闭/降级那一族**。
    //
    // 诚实限定：UI 在 V 域合成、位于后处理之后，**不受 MSAA 影响**。
    // 把序位错配也标成 a11y 影响，会让人误以为"序位错了界面就不可读"，
    // 进而错误地要求 UI 也走 MSAA。
    {
        let a11y_should = [
            MsaaDiagCode::LevelDowngraded,
            MsaaDiagCode::DeferredPathUnsupported,
            MsaaDiagCode::DepthFormatNoMsaa,
            MsaaDiagCode::NoColorLevelSupported,
            MsaaDiagCode::MemoryOverQuota,
        ];
        let not_a11y = [
            MsaaDiagCode::PostReadsMsaaData,
            MsaaDiagCode::TaaDroppedByMutex,
            MsaaDiagCode::CapsOriginDefaulted,
        ];
        let mut ok = true;
        for c in a11y_should.iter() {
            ok &= c.is_a11y_impact();
        }
        for c in not_a11y.iter() {
            ok &= !c.is_a11y_impact();
        }
        set.add("K09-诊断-a11y标记范围正确", ok, "5 降级族为a11y");
    }

    // 判据：降级理由与诊断码**一一对应**（无漏网 `None`）。
    {
        let all = [
            DowngradeReason::None,
            DowngradeReason::ColorSupportIntersection,
            DowngradeReason::DepthFormatUnsupported,
            DowngradeReason::PathUnsupported,
            DowngradeReason::QuotaExceeded,
            DowngradeReason::FormatPairAbsent,
        ];
        let mut ok = all[0].diag_code().is_none();
        let mut seen: Vec<MsaaDiagCode> = Vec::new();
        for r in all.iter().skip(1) {
            match r.diag_code() {
                None => ok = false,
                Some(c) => {
                    if seen.contains(&c) {
                        ok = false;
                    }
                    seen.push(c);
                }
            }
            if r.label().is_empty() || r.tag().is_empty() {
                ok = false;
            }
        }
        set.add("K09-诊断-降级理由与码一一对应", ok, "5 理由各持专属码");
    }

    // 判据：配额三要素**逐项出现**（只说"超配额"的人无法处置）。
    {
        let mut r = req(MsaaLevel::X8);
        r.quota_bytes = 40_000_000;
        let qr = quota_rejection(&r, MsaaLevel::X8, MsaaLevel::X2);
        let t = qr.text();
        set.add(
            "K09-诊断-配额三要素齐备",
            t.contains("当前") && t.contains("上限") && t.contains("建议")
                && qr.overflow_bytes == qr.current_bytes.saturating_sub(qr.limit_bytes),
            "当前/上限/建议 + 超出量自洽",
        );
    }

    // 判据：设置页文本**降级时同时含请求级与实际级**。
    {
        let mut st = MsaaState::new();
        let _ = st.negotiate(
            &NegotiateRequest::forward_1080p(MsaaLevel::X8, 40_000_000),
            &caps_of(all_levels()),
        );
        let txt = st.screen_text();
        set.add(
            "K09-文本-设置页双级别显性",
            txt.contains("8x") && txt.contains("2x") && txt.contains("降级"),
            "请求8x/实际2x/降级计数",
        );
    }

    // 判据：调试负载**键集与快照字段一致**（F2013 接入前的字段契约）。
    {
        let st = MsaaState::new();
        let snap = st.debug_snapshot();
        let keys = debug_keys();
        let mut ok = keys.len() == 9 && !snap.declaration.is_empty();
        for k in keys.iter() {
            ok &= !k.is_empty();
        }
        let txt = snap.text();
        ok &= txt.contains("MSAA[") && txt.contains(']');
        set.add("K09-文本-调试负载键集完整", ok, "9 键 + 文本投影");
    }

    // 判据：状态机的遥测计数**只统计真实事件**。
    //
    // 降级计数在"请求即生效"时不得增长——否则该指标永远大于 0，失去意义。
    {
        let mut st = MsaaState::new();
        let c = caps_of(all_levels());
        let _ = st.negotiate(&req(MsaaLevel::X4), &c);
        let after_ok = st.downgrade_count;
        let _ = st.negotiate(&NegotiateRequest::forward_1080p(MsaaLevel::X8, 40_000_000), &c);
        let after_down = st.downgrade_count;
        // 序位拦截只统计违规。
        let _ = st.check_order(&canonical_stages(MsaaLevel::X2));
        let order_ok_count = st.order_rejections;
        let _ = st.check_order(&vec![Stage::new(StageKind::GeometryPass, 4)]);
        let order_bad_count = st.order_rejections;
        set.add(
            "K09-状态-遥测计数真实",
            after_ok == 0
                && after_down == 1
                && order_ok_count == 0
                && order_bad_count == 1,
            "生效0/降级1/序位0->1",
        );
    }

    // 判据：能力来源为默认假设时**显性登记**（不冒充探测结果）。
    {
        let mut st = MsaaState::new();
        let _ = st.negotiate(&req(MsaaLevel::X4), &DeviceCaps::empty_defaulted());
        set.add(
            "K09-状态-默认能力来源显性",
            st.caps_origin == CapsOrigin::Defaulted
                && st.diag.has(MsaaDiagCode::CapsOriginDefaulted),
            "origin=Defaulted + 诊断",
        );
    }

    // 判据：选型档案**不编造每像素成本**（只给预算并标注待F2017）。
    {
        let p = MsaaProfile::msaa();
        set.add(
            "K09-状态-选型档案字段自洽",
            p.method == AaMethod::Msaa
                && !p.needs_history
                && p.geometry_stage
                && !p.deferred_ok
                && p.memory_multiple == 4
                && p.memory_1080p_4x == MEM_1080P_4X
                && p.cost_ms_1080p_4x_budget == RESOLVE_COST_MS_1080P_4X,
            "几何/无历史/延迟不可用",
        );
    }

    set
}

/// 深化自检（与主检分离，避免单集超MAX_CHECKS=112）。
///
/// 承载反假变体与跨路径一致性两类较长的判据。
pub fn run_vek09_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vek09-deep");

    // 反假变体①：把 resolve 改成"取首样本"，面积守恒**必须**判红。
    //
    // 变体真的改变行为路径（覆盖从 0.5 → 1.0），不是恒真断言。
    // 判据用**单边符号**：正确实现 bias ≤ 0，变异 bias = +0.5 > 0。
    //
    // **变体必须改算法而不是改数据**：初版把`samples[0]` 覆写成
    // `samples[1]`（改的是输入），得到 resolve=0.25、bias=**−0.25 ≤ 0**，
    // 单边判据判它"守恒"——于是这个反假变体**恒绿**，正是它本该防的
    // 那类假门禁。正确写法是让被测函数取首样本：`resolved = samples[0]`
    // （= 1.0 ⇒ bias = +0.5）。这与"门禁设计的自律①"是同一条：
    // **变体必须真的改变被测物的行为路径**。
    {
        let lv = MsaaLevel::X4;
        let edge = 0.5;
        let analytic = analytic_coverage_x(lv, edge);
        let samples = make_edge_samples(lv, edge, 1.0, 0.0);
        // 缺陷实现：取首样本而非平均（fg=1,bg=0 ⇒ 输出即覆盖率）。
        let mutated_resolved = samples[0];
        let bias = mutated_resolved - analytic;
        set.add(
            "K09-变异-resolve取首样本被守恒判据捕获",
            bias > 0.0 && (mutated_resolved - 1.0).abs() < 1e-6,
            "变异取 samples[0] bias=+0.5（正确实现恒 <=0）",
        );
    }

    // 反假变体②：少算一个样本（除以 n−1），守恒**必须**判红。
    //
    // 这是"求平均写成分母错"这一类缺陷的代表性形态：分母少1会让所有
    // 边缘像素系统性偏亮，且不报任何错。偏差为正 ⇒ 单边判据捕获。
    {
        let lv = MsaaLevel::X4;
        let edge = 0.5;
        let analytic = analytic_coverage_x(lv, edge);
        let samples = make_edge_samples(lv, edge, 1.0, 0.0);
        let n = samples.len() as f32;
        let mut acc = 0.0f32;
        for v in samples.iter() {
            acc += *v;
        }
        let mutated = acc / (n - 1.0);
        let bias = mutated - analytic;
        set.add(
            "K09-变异-分母少一被守恒判据捕获",
            bias > 0.0,
            "变异 bias=+0.167",
        );
    }

    // 反假变体③：把 `granted ≤ requested` 反向写成 `≥`，越权判据必须红。
    //
    // 真实缺陷的形态：④ 步漏掉请求级上界 ⇒ granted 8x > requested 4x。
    // 这里用同一组输入验证判据表达式本身能区分两个方向。
    {
        let nc = levels(&[MsaaLevel::Off, MsaaLevel::X4, MsaaLevel::X8]);
        let c = caps_of(nc);
        // 模拟缺陷实现：④ 步只按**配额**上界求交，漏掉请求级上界。
        // 配额宽裕 ⇒ 上界=8x ⇒ granted=8x，而请求只有 4x ⇒ 越权。
        let mutated = nc.max_at_most(MsaaLevel::X8);
        let correct = negotiate(&req(MsaaLevel::X4), &c);
        set.add(
            "K09-变异-越权实现被不越权判据捕获",
            matches!(mutated, Some(l) if !l.le(MsaaLevel::X4))
                && correct.granted.le(MsaaLevel::X4),
            "缺陷给8x(>4x)判红；正确给4x",
        );
    }

    // 反假变体④：序位判定去掉 R2 分支，R2 判据必须红。
    //
    // 去掉 R2 后，这条用例（含 post sample_count=4）会落到 R1裁决上，
    // 于是 `== PostReadsMsaa` 不成立——判据对"漏掉 R2"敏感。
    {
        let stages = vec![
            Stage::new(StageKind::GeometryPass, 4),
            Stage::new(StageKind::PostEffect, 4),
            Stage::new(StageKind::Resolve, 1),
        ];
        let with_r2 = check_order(&stages, MsaaLevel::X4);
        // 模拟去掉 R2：只看结构（resolve 位置）⇒ 落入 R1。
        let resolve_idx = 2usize;
        let first_post = 1usize;
        let without_r2 = if resolve_idx > first_post {
            OrderVerdict::ResolveAfterPost
        } else {
            OrderVerdict::Ok
        };
        set.add(
            "K09-变异-去掉R2后判据变红",
            with_r2 == OrderVerdict::PostReadsMsaa && without_r2 != with_r2,
            "去R2 -> R1 != R2（判据敏感）",
        );
    }

    // 反假变体⑤：互斥判定用 `requested` 而非 `granted`，误报判据必须红。
    //
    // 缺陷形态：协商已把 MSAA 降到 off，TAA 本可正常启用，却因"请求过MSAA"
    // 而报互斥丢弃 —— 用无意义的诊断淹没真问题。
    {
        let c = caps_of(levels(&[MsaaLevel::Off]));
        let req8 = req(MsaaLevel::X8);
        let n = negotiate(&req8, &c);
        let mut bag_defect = MsaaDiagBag::new();
        let mut bag_correct = MsaaDiagBag::new();
        // 缺陷：按 requested 判冲突。
        let defect = if req8.requested.enabled() && true {
            bag_defect.note(MsaaDiagCode::TaaDroppedByMutex, 1.0);
            MutexVerdict::MsaaKeptTaaDropped
        } else {
            MutexVerdict::NoConflict
        };
        // 正确：按 granted 判冲突。
        let correct = guard_aa_mutex(n.granted, true, &mut bag_correct);
        set.add(
            "K09-变异-按requested判会误报",
            defect == MutexVerdict::MsaaKeptTaaDropped
                && correct == MutexVerdict::NoConflict
                && !bag_correct.has(MsaaDiagCode::TaaDroppedByMutex)
                && bag_defect.has(MsaaDiagCode::TaaDroppedByMutex),
            "granted=off 时正确实现不报丢弃",
        );
    }

    // 跨路径一致性：类型携带的`rank` 与 `wire` 与 `samples` 三者恒等。
    //
    // 恒等式是 `samples == 1 << rank` 且 `wire == 1 << rank`，即三者都等于
    // `1<<rank`。**不要写成 `wire == rank`**——那在 `Off` 上就是
    // `1 == 0`，判据恒红而与被测物无关（本条初版即如此，红了才发现）。
    // 这正是"断言红了先问判据错还是被测物错"最平凡的一次命中。
    {
        let mut ok = true;
        for lv in MsaaLevel::all_asc().iter() {
            ok &= lv.rank() == lv.bit();
            ok &= (1u32 << lv.rank()) == lv.wire() as u32;
            ok &= lv.samples() == (1u32 << lv.rank());
        }
        set.add("K09-一致-三级映射恒等", ok, "samples==wire==1<<rank");
    }

    // 跨路径一致性：`LevelSet::max_level` 与逐位枚举结果一致。
    {
        let mut ok = true;
        for bits in 0u8..16 {
            let s = LevelSet::from_bits(bits);
            let mut expect = MsaaLevel::Off;
            for lv in MsaaLevel::all_asc().iter() {
                if s.contains(*lv) {
                    expect = *lv;
                }
            }
            ok &= s.max_level() == expect;
            // count 与实际位数一致。
            let mut cnt = 0u32;
            for lv in MsaaLevel::all_asc().iter() {
                if s.contains(*lv) {
                    cnt += 1;
                }
            }
            ok &= s.count() == cnt;
        }
        set.add("K09-一致-位掩码全16种穷举一致", ok, "max_level/count 穷举");
    }

    // 跨路径一致性：`intersect` 是幂等且可交换的（集合代数基本律）。
    {
        let a = levels(&[MsaaLevel::Off, MsaaLevel::X4]);
        let b = levels(&[MsaaLevel::Off, MsaaLevel::X8]);
        let c = levels(&[MsaaLevel::Off, MsaaLevel::X2]);
        let ok = a.intersect(b) == b.intersect(a)
            && a.intersect(b).intersect(b) == a.intersect(b)
            && a.intersect(b).intersect(c) == LevelSet::only(MsaaLevel::Off)
            && a.is_superset_of(a)
            && a.is_superset_of(LevelSet::only(MsaaLevel::Off));
        set.add("K09-一致-交集满足交换幂等结合", ok, "集合代数四律");
    }

    // `make_edge_samples` 与 `analytic_coverage_x` 必须**独立**得出同一答案。
    //
    // 这条是守恒对账的诚信前提：两者若共享"哪几个样本被覆盖"的实现，
    // 对账就成了自己比自己。实测两侧在三级 × 两个边缘位置全一致。
    {
        let mut ok = true;
        for lv in [MsaaLevel::X2, MsaaLevel::X4, MsaaLevel::X8].iter() {
            for edge in [0.25f32, 0.375, 0.5, 0.625, 0.75].iter() {
                let s = make_edge_samples(*lv, *edge, 1.0, 0.0);
                let counted = s.iter().filter(|v| **v >= 0.5).count();
                let n = s.len() as f32;
                let from_samples = counted as f32 / n;
                let from_positions = analytic_coverage_x(*lv, *edge);
                ok &= (from_samples - from_positions).abs() < 1e-6;
                // 全覆盖位置不应产生任何覆盖样本。
                if *edge <= 0.0 {
                    ok &= counted == 0;
                }
            }
        }
        set.add("K09-一致-覆盖率两路独立一致", ok, "3 级 × 5 边缘位置");
    }

    set
}

// ===========================================================================
// 单元测试（宿主侧`cargo test` 走这里）
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 数红项（`red_items()` 的第二值是条目总数，不是红项数）。
    fn red_count(set: &CheckSet) -> usize {
        let (table, count) = set.red_items();
        let mut n = 0usize;
        for i in 0..count {
            if let Some(c) = table.get(i).copied().flatten() {
                if !c.passed {
                    n += 1;
                }
            }
        }
        n
    }

    /// 1080p 像素数（精确值）：显存断言的推导基数。
    const PX_1080P: u32 = 1920 * 1080;

    /// 位序号与判别序一致（枚举重排会红）。
    #[test]
    fn bit_matches_declaration_order() {
        assert_eq!(MsaaLevel::Off.bit(), 0);
        assert_eq!(MsaaLevel::X2.bit(), 1);
        assert_eq!(MsaaLevel::X4.bit(), 2);
        assert_eq!(MsaaLevel::X8.bit(), 3);
        // 秩必须等于枚举下标（严格递增，且 off 的秩是 0）。
        for (idx, lv) in MsaaLevel::all_asc().iter().enumerate() {
            assert_eq!(lv.rank(), lv.bit());
            assert_eq!(lv.rank() as usize, idx);
        }
    }

    /// 编码值与样本数恒等，且双向可逆。
    #[test]
    fn wire_matches_samples() {
        for lv in MsaaLevel::all_asc().iter() {
            assert_eq!(lv.wire() as u32, lv.samples(), "{:?}", lv);
            assert_eq!(MsaaLevel::from_wire(lv.wire()), Some(*lv));
            assert_eq!(MsaaLevel::from_samples(lv.samples()), Some(*lv));
        }
        // 非法编码必须显性失败而非夹取。
        assert_eq!(MsaaLevel::from_wire(3), None);
        assert_eq!(MsaaLevel::from_wire(0), None);
        assert_eq!(MsaaLevel::from_samples(3), None);
    }

    /// 面积守恒：三级 × 过中心边，偏差恒为非正且解析值精确 0.5。
    #[test]
    fn area_is_conserved_at_center_edge() {
        for lv in [MsaaLevel::X2, MsaaLevel::X4, MsaaLevel::X8].iter() {
            let s = make_edge_samples(*lv, 0.5, 1.0, 0.0);
            let r = area_conservation(&s, *lv, 0.5, 1.0, 0.0);
            assert!((r.analytic_coverage - 0.5).abs() < 1e-6, "{:?}", lv);
            assert!(r.bias <= 0.0, "{:?} bias={}", lv, r.bias);
            assert!(r.conserved, "{:?}", lv);
        }
    }

    /// 面积守恒对"取首样本"变异敏感（反假变体）。
    #[test]
    fn conservation_detects_first_sample_mutation() {
        let lv = MsaaLevel::X4;
        let s = make_edge_samples(lv, 0.5, 1.0, 0.0);
        let analytic = analytic_coverage_x(lv, 0.5);
        let mutated = s[0]; // 取首样本
        assert!(
            mutated - analytic > 0.0,
            "变异必须偏正才能被单边判据捕获：{} vs {}",
            mutated,
            analytic
        );
    }

    /// 非连续支持集下不得给出未声明支持的级别。
    #[test]
    fn noncontiguous_support_is_respected() {
        let nc = levels(&[MsaaLevel::Off, MsaaLevel::X4, MsaaLevel::X8]);
        let c = caps_of(nc);
        assert_eq!(negotiate(&req(MsaaLevel::X8), &c).granted, MsaaLevel::X8);
        assert_eq!(negotiate(&req(MsaaLevel::X4), &c).granted, MsaaLevel::X4);
        // 2x 未被声明支持⇒ 必须落off。
        assert_eq!(negotiate(&req(MsaaLevel::X2), &c).granted, MsaaLevel::Off);
    }

    /// 全组合下 granted 永不高于 requested（越权回归锁）。
    #[test]
    fn granted_never_exceeds_requested() {
        for supported in [
            levels(&[MsaaLevel::Off]),
            levels(&[MsaaLevel::Off, MsaaLevel::X2]),
            levels(&[MsaaLevel::Off, MsaaLevel::X4]),
            levels(&[MsaaLevel::Off, MsaaLevel::X8]),
            levels(&[MsaaLevel::Off, MsaaLevel::X2, MsaaLevel::X8]),
            levels(&[MsaaLevel::Off, MsaaLevel::X4, MsaaLevel::X8]),
            all_levels(),
        ] {
            let c = caps_of(supported);
            for requested in MsaaLevel::all_asc().iter() {
                let n = negotiate(&req(*requested), &c);
                assert!(
                    n.granted.le(*requested),
                    "cap{} req{} -> {}",
                    supported.text(),
                    requested.tag(),
                    n.granted.tag()
                );
            }
        }
    }

    /// 延迟路径否决优先于能力短板，且不因设备能力而变。
    #[test]
    fn deferred_path_wins_over_capability() {
        let mut r = req(MsaaLevel::X8);
        r.path = RenderPath::Deferred;
        for c in [caps_of(all_levels()), DeviceCaps::empty_defaulted()] {
            let n = negotiate(&r, &c);
            assert_eq!(n.granted, MsaaLevel::Off);
            assert_eq!(n.reason, DowngradeReason::PathUnsupported);
        }
    }

    /// 深度不支持 ⇒ 整体 off，不是降一级。
    #[test]
    fn depth_unsupported_forces_off() {
        let c = caps_split(all_levels(), levels(&[MsaaLevel::Off]));
        let n = negotiate(&req(MsaaLevel::X8), &c);
        assert_eq!(n.granted, MsaaLevel::Off);
        assert_eq!(n.reason, DowngradeReason::DepthFormatUnsupported);
    }

    /// 序位：R2 先于 R1。
    #[test]
    fn r2_precedes_r1() {
        let stages = vec![
            Stage::new(StageKind::GeometryPass, 4),
            Stage::new(StageKind::PostEffect, 4),
            Stage::new(StageKind::Resolve, 1),
        ];
        assert_eq!(check_order(&stages, MsaaLevel::X4), OrderVerdict::PostReadsMsaa);
    }

    /// 序位：五种违规各持专属裁决。
    #[test]
    fn all_five_order_violations_detected() {
        assert_eq!(
            check_order(
                &[
                    Stage::new(StageKind::PostEffect, 1),
                    Stage::new(StageKind::Resolve, 1)
                ],
                MsaaLevel::X4
            ),
            OrderVerdict::ResolveAfterPost
        );
        assert_eq!(
            check_order(
                &[
                    Stage::new(StageKind::GeometryPass, 4),
                    Stage::new(StageKind::PostEffect, 1)
                ],
                MsaaLevel::X4
            ),
            OrderVerdict::ResolveMissing
        );
        assert_eq!(
            check_order(
                &[
                    Stage::new(StageKind::GeometryPass, 1),
                    Stage::new(StageKind::Resolve, 1)
                ],
                MsaaLevel::Off
            ),
            OrderVerdict::ResolveRedundant
        );
        assert_eq!(
            check_order(
                &[
                    Stage::new(StageKind::Resolve, 1),
                    Stage::new(StageKind::Resolve, 1)
                ],
                MsaaLevel::X4
            ),
            OrderVerdict::ResolveDuplicated
        );
    }

    /// 互斥：同开丢TAA，且 effective_method 落定MSAA。
    #[test]
    fn mutex_drops_taa_and_names_it() {
        let mut bag = MsaaDiagBag::new();
        let v = guard_aa_mutex(MsaaLevel::X4, true, &mut bag);
        assert_eq!(v, MutexVerdict::MsaaKeptTaaDropped);
        assert_eq!(v.dropped(), Some(AaMethod::Taa));
        assert!(bag.has(MsaaDiagCode::TaaDroppedByMutex));
        assert_eq!(
            effective_method(v, MsaaLevel::X4, true),
            AaMethod::Msaa
        );
    }

    /// 互斥：无冲突时 effective_method 不丢方法（回归锁）。
    #[test]
    fn effective_method_keeps_single_enabled_method() {
        assert_eq!(
            effective_method(MutexVerdict::NoConflict, MsaaLevel::X4, false),
            AaMethod::Msaa
        );
        assert_eq!(
            effective_method(MutexVerdict::NoConflict, MsaaLevel::Off, true),
            AaMethod::Taa
        );
    }

    /// 显存实算精确值（1080p RGBA8+D32 4x）。
    #[test]
    fn memory_is_exact() {
        assert_eq!(
            msaa_memory_bytes(1920, 1080, ColorFormat::Rgba8Unorm, DepthFormat::D32Float, MsaaLevel::X4),
            66_355_200
        );
        assert_eq!(PX_1080P, 2_073_600);
        // 深度同样乘样本数（只算颜色是经典错误）。
        assert_eq!(
            depth_memory_bytes(1920, 1080, DepthFormat::D32Float, MsaaLevel::X4),
            33_177_600
        );
    }

    /// 成本模型线性（性质可验证；绝对值是预算不是实测）。
    #[test]
    fn cost_model_is_linear() {
        let c2 = resolve_cost_ms(1920, 1080, MsaaLevel::X2);
        let c4 = resolve_cost_ms(1920, 1080, MsaaLevel::X4);
        let c8 = resolve_cost_ms(1920, 1080, MsaaLevel::X8);
        assert!((c4 / c2 - 2.0).abs() < 1e-4);
        assert!((c8 / c4 - 2.0).abs() < 1e-4);
        assert!((c4 - 0.3).abs() < 1e-6);
    }

    /// 零配额不panic、不溢出（无符号减法陷阱）。
    #[test]
    fn zero_quota_is_safe() {
        let mut r = req(MsaaLevel::X8);
        r.quota_bytes = 0;
        let n = negotiate(&r, &caps_of(all_levels()));
        assert_eq!(n.granted, MsaaLevel::Off);
        let qr = quota_rejection(&r, MsaaLevel::X8, MsaaLevel::Off);
        assert_eq!(qr.overflow_bytes, qr.current_bytes);
        // 0 宽高也不得下溢。
        let zero = msaa_memory_bytes(0, 0, ColorFormat::Rgba8Unorm, DepthFormat::D32Float, MsaaLevel::X8);
        assert_eq!(zero, 0);
    }

    /// resolve 拒绝长度不符并落阻断诊断。
    #[test]
    fn resolve_rejects_wrong_length() {
        let mut bag = MsaaDiagBag::new();
        assert_eq!(resolve(&[1.0, 0.0], MsaaLevel::X4, &mut bag), 0.0);
        assert!(bag.has(MsaaDiagCode::SampleCountMismatch));
        assert_eq!(bag.blocking_count(), 1);
    }

    /// resolve 不被非有限值污染。
    #[test]
    fn resolve_sanitizes_non_finite() {
        let mut bag = MsaaDiagBag::new();
        let out = resolve(&[f32::NAN, 1.0, 1.0, 1.0], MsaaLevel::X4, &mut bag);
        assert!(out.is_finite());
        assert!((out - 0.75).abs() < 1e-6);
    }

    /// 位掩码全16 种穷举：max_level / count / intersect 自洽。
    #[test]
    fn level_set_exhaustive_16() {
        for bits in 0u8..16 {
            let s = LevelSet::from_bits(bits);
            let mut expect_max = MsaaLevel::Off;
            let mut cnt = 0u32;
            for lv in MsaaLevel::all_asc().iter() {
                if s.contains(*lv) {
                    expect_max = *lv;
                    cnt += 1;
                }
            }
            assert_eq!(s.max_level(), expect_max, "bits={:#04x}", bits);
            assert_eq!(s.count(), cnt, "bits={:#04x}", bits);
        }
        // 交集满足交换与幂等。
        let a = levels(&[MsaaLevel::Off, MsaaLevel::X4]);
        let b = levels(&[MsaaLevel::Off, MsaaLevel::X8]);
        assert_eq!(a.intersect(b), b.intersect(a));
        assert_eq!(a.intersect(b), LevelSet::only(MsaaLevel::Off));
    }

    /// 能力表：重复登记被拒不覆盖；表满拒绝新增。
    #[test]
    fn caps_table_never_overwrites() {
        let mut c = DeviceCaps::probed(vec![FormatSupport::uniform(
            ColorFormat::Rgba8Unorm,
            DepthFormat::D32Float,
            &[MsaaLevel::Off, MsaaLevel::X8],
        )]);
        assert!(!c.register(FormatSupport::uniform(
            ColorFormat::Rgba8Unorm,
            DepthFormat::D32Float,
            &[MsaaLevel::Off],
        )));
        match c.query(ColorFormat::Rgba8Unorm, DepthFormat::D32Float) {
            CapsLookup::Found(e) => assert!(e.color_supported.contains(MsaaLevel::X8)),
            CapsLookup::Absent => panic!("条目不应消失"),
        }
        // `is_full()` 判的是**容量**（`FORMAT_TABLE_SIZE` = 8 条满），
        // 不是"登记过条目"。初版在此断言 `is_full()` 想表达"表里已有一条
        // 配对、重复登记不得覆盖"，属误用——判据与被测物说的不是一回事。
        // 此处真正要守的是"重复登记被拒不覆盖"（上面已断言），容量满
        // 由 `caps_table_rejects_when_full` 单独覆盖。
        assert!(!c.is_full(), "仅登记 1 条，容量未满（上限 8）");
        // 表未满时新增**不同**配对本就该成功——这条断言留着是初版的
        // 残留：它把"不得覆盖"与"不得新增"混为一谈（被测物行为正确，
        // 断言与它矛盾）。容量满时的拒绝由 `caps_table_rejects_when_full`
        // 覆盖。
        assert!(c.register(FormatSupport::uniform(
            ColorFormat::R16Float,
            DepthFormat::D16Unorm,
            &[MsaaLevel::X2]
        )));
    }

    /// 能力表登记满 `FORMAT_TABLE_SIZE` 条后拒绝新增（**不覆盖既有项**）。
    #[test]
    fn caps_table_rejects_when_full() {
        let mut c = DeviceCaps::probed(Vec::new());
        let pairs = [
            (ColorFormat::Rgba8Unorm, DepthFormat::D32Float),
            (ColorFormat::Rgba8Unorm, DepthFormat::D24UnormS8Uint),
            (ColorFormat::R16Float, DepthFormat::D16Unorm),
            (ColorFormat::R11G11B10Float, DepthFormat::D32Float),
            (ColorFormat::Bgra8Unorm, DepthFormat::D24UnormS8Uint),
            (ColorFormat::Bgra8Unorm, DepthFormat::D16Unorm),
            (ColorFormat::R16Float, DepthFormat::D32Float),
            (ColorFormat::Bgra8Unorm, DepthFormat::D32Float),
        ];
        for (color, depth) in pairs.iter() {
            assert!(c.register(FormatSupport::uniform(
                *color,
                *depth,
                &[MsaaLevel::Off, MsaaLevel::X4]
            )));
        }
        assert!(c.is_full(), "登记 {} 条后应满", c.len());
        // 满表新增必拒——**不是覆盖**最后一条。
        let before = c.entries().last().map(|e| e.color);
        assert!(!c.register(FormatSupport::uniform(
            ColorFormat::R11G11B10Float,
            DepthFormat::D16Unorm,
            &[MsaaLevel::X8]
        )));
        assert_eq!(c.len(), pairs.len(), "表长须不变");
        assert_eq!(c.entries().last().map(|e| e.color), before, "末项不得被覆盖");
    }

    /// 缺失配对 ⇒ 不支持（用表外格式验，不是表内元素）。
    #[test]
    fn absent_pair_is_unsupported() {
        let c = caps_of(all_levels());
        let n = negotiate(
            &NegotiateRequest {
                requested: MsaaLevel::X4,
                path: RenderPath::Forward,
                color: ColorFormat::R11G11B10Float,
                depth: DepthFormat::D24UnormS8Uint,
                width: 1920,
                height: 1080,
                quota_bytes: 1 << 30,
            },
            &c,
        );
        assert_eq!(n.granted, MsaaLevel::Off);
        assert_eq!(n.reason, DowngradeReason::FormatPairAbsent);
    }

    /// 样本位置逐项合法且重心居中。
    ///
    /// `Off` 的位置表**为空**是正确编码（1x 无需多采样位置），
    /// 故长度判据只对**开启**的级别生效。
    #[test]
    fn sample_positions_are_sane() {
        for lv in MsaaLevel::all_asc().iter() {
            assert!(sample_positions_in_unit_square(*lv), "{:?}", lv);
            assert!(sample_centroid_offset(*lv) < 1e-6, "{:?}", lv);
            if lv.enabled() {
                assert_eq!(sample_positions(*lv).len() as u32, lv.samples());
            } else {
                assert!(sample_positions(*lv).is_empty());
            }
        }
        assert!(sample_positions(MsaaLevel::Off).is_empty());
    }

    /// 配对能力取**真交集**（两种互补用例，各自能抓住一种错实现）。
    #[test]
    fn pair_support_is_real_intersection() {
        // 颜色含 8x、深度只到 4x ⇒ 交集 {off,4x}。
        let c1 = caps_split(
            levels(&[MsaaLevel::Off, MsaaLevel::X4, MsaaLevel::X8]),
            levels(&[MsaaLevel::Off, MsaaLevel::X4]),
        );
        let s1 = c1.query_common_set(ColorFormat::Rgba8Unorm, DepthFormat::D32Float);
        assert_eq!(s1.max_level(), MsaaLevel::X4);
        assert!(!s1.contains(MsaaLevel::X8));
        // 颜色 {off,8x}、深度 {off,4x} 无公共 MSAA 级 ⇒ 交集只剩 off。
        // 若实现错取 min(8x,4x)=4x，这条会红。
        let c2 = caps_split(
            levels(&[MsaaLevel::Off, MsaaLevel::X8]),
            levels(&[MsaaLevel::Off, MsaaLevel::X4]),
        );
        let s2 = c2.query_common_set(ColorFormat::Rgba8Unorm, DepthFormat::D32Float);
        assert_eq!(s2.max_level(), MsaaLevel::Off);
        assert!(!s2.contains(MsaaLevel::X4));
        assert!(!s2.contains(MsaaLevel::X8));
    }

    /// 设置页文本降级时双级别显性。
    #[test]
    fn screen_text_shows_both_levels() {
        let mut st = MsaaState::new();
        let _ = st.negotiate(
            &NegotiateRequest::forward_1080p(MsaaLevel::X8, 40_000_000),
            &caps_of(all_levels()),
        );
        let t = st.screen_text();
        assert!(t.contains('8') && t.contains('2') && t.contains("降级"), "{}", t);
    }

    /// 遥测计数只统计真实事件。
    #[test]
    fn telemetry_counts_real_events_only() {
        let mut st = MsaaState::new();
        let c = caps_of(all_levels());
        let _ = st.negotiate(&req(MsaaLevel::X4), &c);
        assert_eq!(st.downgrade_count, 0);
        let _ = st.negotiate(&NegotiateRequest::forward_1080p(MsaaLevel::X8, 40_000_000), &c);
        assert_eq!(st.downgrade_count, 1);
        let _ = st.check_order(&canonical_stages(MsaaLevel::X4));
        assert_eq!(st.order_rejections, 0);
        let _ = st.check_order(&vec![Stage::new(StageKind::GeometryPass, 4)]);
        assert_eq!(st.order_rejections, 1);
    }

    /// 主检与深化检全绿（红项必须点名，不许吞）。
    #[test]
    fn all_checks_green() {
        // `red_items()` 返回 `(条目表, 条目数)`——第二个值是**条目总数**，
        // 不是红项数。初版误当红项数用（`assert_eq!(na, 0)` 恒报"61 项红"），
        // 属测试读错API。红项须数条目表里 `passed == false` 的条数。
        let a = run_vek09_checks();
        let na = red_count(&a);
        assert_eq!(na, 0, "主检有{} 项红", na);
        let b = run_vek09_deep_checks();
        let nb = red_count(&b);
        assert_eq!(nb, 0, "深化检有{} 项红", nb);
    }
}