
// ---------------------------------------------------------------------------
// F013 · 深化批次六：图标索引负数语义（资源 ID 图标定位）
//
// 主册依据（G-A-13【功能定义】）：「图标位置」字段——IconLocation 的索引
// 语义：非负 = 组图标序号（F014 pick_group_member 消费）；**负数 = 资源 ID**
// （按 ID 直取，PE 资源面语义——Windows 同约定）。
// ---------------------------------------------------------------------------

/// 图标索引语义判别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconIndexSemantics {
    /// 非负：组图标序号（第 N 枚）。
    GroupOrdinal(u16),
    /// 负数：按资源 ID 直取（绝对值 = ID）。
    ResourceId(u16),
    /// 无图标位（0 或缺失 → 第一枚/默认——0 即序号 0）。
    Zeroth,
}

/// 解析图标索引（i16——负数语义的载体）。
pub fn decode_icon_index(idx: i16) -> IconIndexSemantics {
    match idx {
        0 => IconIndexSemantics::Zeroth,
        n if n > 0 => IconIndexSemantics::GroupOrdinal(n as u16),
        n => IconIndexSemantics::ResourceId((-(n as i32)) as u16),
    }
}

/// F013 深化批次六自检。
pub fn run_lnkfile_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F013-lnkfile-deep5");
    // 1) 正数 = 组序号；0 = 第零枚/默认。
    cs.add(
        "icon_index_positive_and_zero",
        decode_icon_index(3) == IconIndexSemantics::GroupOrdinal(3)
            && decode_icon_index(0) == IconIndexSemantics::Zeroth,
        "",
    );
    // 2) 负数 = 资源 ID 直取（-5 → ID 5——绝对值语义）。
    cs.add(
        "icon_index_negative_resource_id",
        decode_icon_index(-5) == IconIndexSemantics::ResourceId(5),
        "",
    );
    // 3) i16 全域往返：序号/资源 ID 互不误判（边界 32767/-32768）。
    cs.add(
        "icon_index_full_range",
        decode_icon_index(32767) == IconIndexSemantics::GroupOrdinal(32767)
            && decode_icon_index(-32768) == IconIndexSemantics::ResourceId(32768),
        "",
    );
    cs
}
