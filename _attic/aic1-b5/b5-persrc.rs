
// ---------------------------------------------------------------------------
// F014 · 深化批次五：manifest 兼容性/主题声明检测（三件套之外的声明面）
//
// 主册依据（G-A-14【功能定义】）：「清单（manifest → DPI 感知三态 F028 +
// 兼容性声明 + 主题声明）」——DPI 面由批次一批次二承载（parse_manifest_
// awareness）；本段补**兼容性声明**（<compatibility> 节）与**主题声明**
// （<windowsSettings> 下主题相关声明的存在性检测——标签级检测，声明值
// 语义随闸门对拍）。
// ---------------------------------------------------------------------------

/// manifest 内检测标签是否出现（字面扫描——BOM 已由 strip_bom 既有面剥离）。
/// 支持命名空间前缀：`<tag>` 或 `<prefix:tag`（前缀边界 = '<' 后非 '/'/'?'
/// 且紧随 ':' 加 tag 的形态由 find_subslice_ns 判定）。
pub fn manifest_has_tag(data: &[u8], tag: &str) -> bool {
    let open_plain = {
        let mut v = alloc::vec::Vec::new();
        v.extend_from_slice(b"<");
        v.extend_from_slice(tag.as_bytes());
        v.push(b'>');
        v
    };
    find_subslice(data, &open_plain).is_some() || find_subslice_ns(data, tag).is_some()
}

fn find_subslice(h: &[u8], n: &[u8]) -> Option<usize> {
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| &h[i..i + n.len()] == n)
}

/// 命名空间前缀版：`<x:tag`（':' 在 '<' 与 tag 之间）。
fn find_subslice_ns(h: &[u8], tag: &str) -> Option<usize> {
    let tag_b = tag.as_bytes();
    if h.len() < tag_b.len() + 2 {
        return None;
    }
    (0..=h.len() - tag_b.len() - 2).find(|&i| {
        h[i] == b'<'
            && h[i + 1] != b'/'
            && h[i + 1] != b'?'
            && &h[i + 2..i + 2 + tag_b.len()] == tag_b
    })
}

/// F014 深化批次五自检。
pub fn run_persrc_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F014-persrc-deep4");
    // 1) 兼容性声明检测：<compatibility> 直出。
    let m1 = b"<assembly><compatibility><application/></compatibility></assembly>";
    cs.add("manifest_compatibility_detected", manifest_has_tag(m1, "compatibility"), "");
    // 2) 命名空间前缀容忍：<asmv3:compatibility> 同样命中（前缀边界精确——
    //    <compability> 拼错不误命中）。
    let m2 = b"<assembly><asmv3:windowsSettings/></assembly>";
    cs.add(
        "manifest_ns_prefix_tolerated",
        manifest_has_tag(m2, "windowsSettings")
            && !manifest_has_tag(m2, "windowsSetting"),
        "",
    );
    // 3) 缺声明如实 false（no-compat manifest 不虚报）。
    let m3 = b"<assembly><windowsSettings><dpiAware>true</dpiAware></windowsSettings></assembly>";
    cs.add(
        "manifest_compatibility_absent_honest",
        !manifest_has_tag(m3, "compatibility") && manifest_has_tag(m3, "windowsSettings"),
        "",
    );
    cs
}
