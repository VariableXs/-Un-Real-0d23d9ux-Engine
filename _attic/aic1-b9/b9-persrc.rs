
// ---------------------------------------------------------------------------
// F014 · 深化批次九：组图标锚既有面（批次二已有 parse_group_icon 全实现——
// GRPICONDIR 头 + 14B 条目、type 非 1 拒、截断拒；本段不重复实现，deep8
// 检查锚既有面钉值：0 宽高=256 惯例在 RT_ICON 层、icon_id 三级目录联动）。
// ---------------------------------------------------------------------------

/// F014 深化批次九自检（锚批次二 parse_group_icon——零冗余）。
fn run_persrc_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F014-persrc-deep8");
    // 构造两目条目：32×32 8bpp（icon_id 1）+ 0×0（=256 惯例）32bpp（icon_id 2）。
    let mut d: alloc::vec::Vec<u8> = alloc::vec![0u8; 6 + 2 * 14];
    d[2] = 1;
    d[4] = 2;
    d[6] = 32;
    d[7] = 32;
    d[10] = 1; // planes
    d[12] = 8; // bit_count
    d[14] = 0xE8;
    d[15] = 0x02; // bytes_in_res = 744
    d[18] = 1; // icon_id
    d[26] = 32; // 第二条 bit_count
    d[32] = 2; // icon_id @ o+12 = 32
    // 1) 全字段解出；0 宽高条目按 RT_ICON 层惯例读 0（=256 的编码位）。
    let parsed = parse_group_icon(&d);
    cs.add(
        "group_icon_anchor_full_fields",
        matches!(parsed, Some(ref e) if e.len() == 2
            && e[0].width == 32 && e[0].height == 32
            && e[0].bit_count == 8 && e[0].bytes_in_res == 744 && e[0].icon_id == 1
            && e[1].width == 0 && e[1].height == 0 && e[1].icon_id == 2),
        "",
    );
    // 2) 类型位非 1 如实拒（锚既有行为）。
    let mut dc = d.clone();
    dc[2] = 2;
    cs.add(
        "group_icon_anchor_rejects_cursor",
        parse_group_icon(&dc).is_none(),
        "",
    );
    // 3) 空目录（count=0）如实返回空表（既有行为）；空输入如实拒。
    let mut dz = d.clone();
    dz[4] = 0;
    dz[5] = 0;
    cs.add(
        "group_icon_anchor_empty_and_short",
        matches!(parse_group_icon(&dz), Some(ref e) if e.is_empty())
            && parse_group_icon(&[]).is_none(),
        "",
    );
    cs
}
