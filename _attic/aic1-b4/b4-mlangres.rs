
// ---------------------------------------------------------------------------
// F015 · 深化批次四：语言选择规则设置页展示（当前生效顺序可读面）
//
// 主册依据（G-A-15【交互设计】）：「语言选择规则在设置中心『时间和语言』页
// 可查（展示当前生效顺序）」——回退链是既有语义（一处一事实），本段只做
// **可读化渲染**：链序逐槽出显示名，用户能看见「为什么出的是这个语言」。
// ---------------------------------------------------------------------------

/// 已知语言显示名（设置页可读面；未收录 langid → None，渲染时走十六进制
/// 如实显示，不冒充已知语言）。
pub fn lang_display_name(id: u16) -> Option<&'static str> {
    match id {
        0x0804 => Some("zh-CN"),
        0x0404 => Some("zh-TW"),
        0x0004 => Some("zh"),
        0x0409 => Some("en-US"),
        0x0009 => Some("en"),
        0x0000 => Some("中立"),
        _ => None,
    }
}

/// 渲染当前生效顺序（`zh-CN → 中立 → en-US → 中立` 形态——与 fallback_chain
/// 逐槽一致，含重复槽如实显示：链的真实形状不美化）。
pub fn render_chain_display(requested: u16, buf: &mut [u8]) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let (chain, n) = fallback_chain(requested);
    let mut n_out = 0usize;
    let mut put = |buf: &mut [u8], n_out: &mut usize, s: &[u8]| {
        for &b in s {
            if *n_out < buf.len() {
                buf[*n_out] = b;
                *n_out += 1;
            }
        }
    };
    for i in 0..n {
        if i > 0 {
            put(buf, &mut n_out, b" \xE2\x86\x92 "); // " → "（UTF-8）
        }
        match lang_display_name(chain[i]) {
            Some(name) => put(buf, &mut n_out, name.as_bytes()),
            None => {
                put(buf, &mut n_out, b"0x");
                let v = chain[i];
                for shift in [12u16, 8, 4, 0] {
                    if n_out < buf.len() {
                        buf[n_out] = HEX[((v >> shift) & 0xF) as usize];
                        n_out += 1;
                    }
                }
            }
        }
    }
    n_out
}

/// F015 深化批次四自检。
pub fn run_mlangres_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F015-mlangres-deep3");
    // 1) zh-CN 请求的生效顺序可读渲染：逐槽与 fallback_chain 同形（含中立槽
    //    重复如实显示——链的真实形状）。
    let mut buf = [0u8; 128];
    let n = render_chain_display(0x0804, &mut buf);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    cs.add(
        "chain_display_zh_cn",
        text.starts_with("zh-CN") && text.contains("\u{2192}") && text.contains("中立"),
        "",
    );
    // 2) en-US 请求：链更短（自身 + 中立槽），无 en 槽重复。
    let mut buf2 = [0u8; 128];
    let n2 = render_chain_display(0x0409, &mut buf2);
    let text2 = core::str::from_utf8(&buf2[..n2]).unwrap_or("");
    cs.add(
        "chain_display_en_us",
        text2.starts_with("en-US") && text2.contains("中立") && !text2.contains("zh"),
        "",
    );
    // 3) 未知 langid 走十六进制如实显示（不冒充已知语言）。
    let mut buf3 = [0u8; 128];
    let n3 = render_chain_display(0x0641, &mut buf3);
    let text3 = core::str::from_utf8(&buf3[..n3]).unwrap_or("");
    cs.add("chain_display_unknown_hex", text3.contains("0x0641"), "");
    cs
}
