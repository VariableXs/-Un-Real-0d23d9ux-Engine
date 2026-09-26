
// ---------------------------------------------------------------------------
// F014 · 深化批次三：manifest 容忍 BOM + 图标尺寸梯（256>64>48>32 逐级降档、
// 放大不缩小、全坏默认图标）+ 资源语言选择（复用 F015 回退链——一处一事实）
//
// 主册依据（G-A-14【设计细节】）：「manifest 解析容忍 BOM 与命名空间前缀」
// （命名空间前缀由既有 parse_manifest_awareness 承载）；【交互设计】「图标
// 提取优先级：256>64>48>32 逐级降档（4K 重采样走 C-7 管线）」；【状态与异常】
// 「图标资源损坏 → 逐尺寸回退，全坏用默认图标」。pick_group_member 既有面
// （精确档/就近放大），本段补梯级优先与语言选择编排。
// ---------------------------------------------------------------------------

/// UTF-8 BOM（EF BB BF）剥离——manifest 与资源 XML 的入口统一处理。
pub fn strip_bom(data: &[u8]) -> &[u8] {
    if data.len() >= 3 && data[0] == 0xEF && data[1] == 0xBB && data[2] == 0xBF {
        &data[3..]
    } else {
        data
    }
}

/// 图标尺寸梯（主册【交互设计】原文序——一处一事实）。
pub const ICON_LADDER_PX: [u16; 4] = [256, 64, 48, 32];

/// 默认图标哨兵（全坏 → VARIX 通用图标；0 号不是合法尺寸档，作哨兵无碰撞）。
pub const DEFAULT_ICON_SENTINEL: u16 = 0;

/// 图标梯选择：梯内命中精确档按优先级取；梯内全无 → 取最大可用档放大
/// （禁止缩小——小图标放大等于糊）；无任何可用档 → 默认图标哨兵。
pub fn pick_icon_ladder(available: &[u16]) -> u16 {
    if available.is_empty() {
        return DEFAULT_ICON_SENTINEL;
    }
    for want in ICON_LADDER_PX {
        if available.contains(&want) {
            return want;
        }
    }
    let mut best = available[0];
    for &a in available.iter() {
        if a > best {
            best = a;
        }
    }
    best
}

/// 资源语言选择（多语言资源目录中挑生效语言——复用 F015 回退链 zh-CN→zh→
/// en-US→en→中立，一处一事实：链定义只在 mlangres）。
pub fn select_resource_lang(available: &[u16], requested: u16) -> Option<u16> {
    let (chain, n) = super::mlangres::fallback_chain(requested);
    for i in 0..n {
        let want = chain[i];
        if available.contains(&want) {
            return Some(want);
        }
    }
    None
}

/// F014 深化批次三自检。
pub fn run_persrc_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F014-persrc-deep2");
    // 1) BOM 容忍：带 BOM 与剥后数据进同一解析面得到同一判定（不因 BOM 误判）。
    let mut manifest = alloc::vec::Vec::new();
    manifest.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    manifest.extend_from_slice(b"<assembly></assembly>");
    let plain = &manifest[3..];
    cs.add(
        "manifest_bom_tolerated",
        strip_bom(&manifest) == plain && strip_bom(plain) == plain,
        "",
    );
    // 2) 图标梯：256 优先于 64 优先于 48 优先于 32；梯外取最大档放大（16/20
    //    场景取 20——放大不缩小）；全空 → 默认图标哨兵。
    cs.add(
        "icon_ladder_priority_and_upscale_only",
        pick_icon_ladder(&[48, 256, 32]) == 256
            && pick_icon_ladder(&[48, 32]) == 48
            && pick_icon_ladder(&[16, 20]) == 20
            && pick_icon_ladder(&[]) == DEFAULT_ICON_SENTINEL,
        "",
    );
    // 3) 资源语言：回退链复用——en-US 资源满足 zh-CN 请求（回退链第 3 槽）；
    //    无交集如实 None（不猜不冒充）。
    cs.add(
        "resource_lang_via_f015_chain",
        select_resource_lang(&[0x0409, 0x0000], 0x0804) == Some(0x0409)
            && select_resource_lang(&[0x0407], 0x0804).is_none(),
        "",
    );
    cs
}
