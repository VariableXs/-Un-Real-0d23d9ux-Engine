
// ---------------------------------------------------------------------------
// F013 · 深化批次九：热键锚既有面（批次二已有 decode_hotkey 全实现——
// HOTKEYF 位 + Ctrl→Alt→Shift 短语序 + F 键/十六进制诚实降级；本段不重复
// 实现，deep8 检查锚既有面钉值：全修饰序、域外诚实降级、零值空短语）。
// ---------------------------------------------------------------------------

/// F013 深化批次九自检（锚批次二 decode_hotkey——零冗余）。
fn run_lnkfile_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F013-lnkfile-deep8");
    // 1) 全修饰钉值：Ctrl+Alt+Shift+K（既有短语的规范修饰序）。
    let raw = (0x4Bu16) | ((HOTKEYF_SHIFT | HOTKEYF_CONTROL | HOTKEYF_ALT) as u16) << 8;
    let h = decode_hotkey(raw);
    cs.add(
        "hotkey_full_modifiers_canonical_order",
        h.vk == 0x4B && h.ctrl && h.shift && h.alt
            && &h.phrase.text[..h.phrase.len] == b"Ctrl+Alt+Shift+K",
        "",
    );
    // 2) 域外诚实降级：非字母数字非 F 键（如 0x2C）→ 十六进制短语，不猜文案。
    let h2 = decode_hotkey(0x002C);
    cs.add(
        "hotkey_out_of_domain_honest_fallback",
        h2.vk == 0x2C && h2.phrase.len > 0,
        "",
    );
    // 3) F 键域（0x70-0x87）与零值空短语：F1 与无热键两钉。
    let h3 = decode_hotkey(0x0070);
    let h0 = decode_hotkey(0);
    cs.add(
        "hotkey_fkey_and_zero",
        h3.phrase.len == 2 && &h3.phrase.text[..2] == b"F1"
            && h0.vk == 0 && h0.phrase.len == 0,
        "",
    );
    cs
}
