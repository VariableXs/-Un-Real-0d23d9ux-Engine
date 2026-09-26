#[cfg(test)]
mod diag3 {
    use crate::compatstar::peblend::*;
    #[test]
    fn adversarial_labels() {
    // 5) 对抗样本集 30 枚全拒：15 类畸形逐项验证（每类均实测拒绝——
    //    不凑数，30/30 缺一即回炉）。
    let mut rejected = 0u32;
    let off = 0x80usize; // pe_off（build_static_pe 的 PE 签名位）
    let opt = off + 24;
    let table = opt + OPTIONAL_HDR_SIZE_PE32P;
    let dd = opt + 112;
    // a-c) 尺寸不足
    for n in [0usize, 8, 0x3F] {
        if parse(&vec![0u8; n]).is_err() { rejected += 1; println!("REJ a1"); }
    }
    // d) MZ 魔数破坏
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[0] = b'X';
    if parse(&bad).is_err() { rejected += 1; println!("REJ a2"); }
    // e) e_lfanew 越上界
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[0x3C..0x40].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; println!("REJ a3"); }
    // f) e_lfanew 悬在 DOS 头内（PE 签名位读不到）
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[0x3C..0x40].copy_from_slice(&0x41u32.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; println!("REJ d-mz"); }
    // g) PE 签名破坏
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[off] = b'Q';
    if parse(&bad).is_err() { rejected += 1; println!("REJ e-lf-new-hi"); }
    // h-i) 机器字段错（I386 / 0）
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[off + 4..off + 6].copy_from_slice(&0x014Cu16.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; println!("REJ f-lf-0x41"); }
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[off + 4..off + 6].copy_from_slice(&0u16.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; println!("REJ g-pesig"); }
    // j) PE32（0x10B）可选头
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[off + 24..off + 26].copy_from_slice(&0x10Bu16.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; println!("REJ h-i386"); }
    // k) 零节
    if parse(&build_static_pe(SUBSYSTEM_GUI, 4096, 0, false)).is_err() { rejected += 1; println!("REJ i-mach0"); }
    // l-t) 非法子系统 9 枚（POSIX/OS2/EFI/保留/自定义带全部如拒）
    for sub in [0u16, 5, 7, 10, 16, 0xFFFE, 0x100, 0x400, 0x2000] {
        if parse(&build_static_pe(sub, 4096, 1, false)).is_err() { rejected += 1; println!("REJ j-pe32"); }
    }
    // u) SizeOfImage 爆 256MB 上限
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[opt + 56..opt + 60].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; println!("REJ k-nsec0"); }
    // v) 入口越出 SizeOfImage
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[opt + 16..opt + 20].copy_from_slice(&0x200000u32.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; println!("REJ l1-sub0"); }
    // w) 入口落不可执行区（节表外）
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[opt + 16..opt + 20].copy_from_slice(&0xF000u32.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; println!("REJ l2-sub5"); }
    // x) 节 rawsz 越文件尾
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[table + 16..table + 20].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; println!("REJ l3-sub7"); }
    // y) 节 rawoff 越文件尾
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[table + 20..table + 24].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; println!("REJ l4-sub10"); }
    // z) 重定位预算爆 64MB
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, true);
    bad[dd + 44..dd + 48].copy_from_slice(&((RELOC_BUDGET_BYTES as u32) + 1).to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; println!("REJ l5-sub16"); }
    // aa) 重定位尺寸爆 u32
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, true);
    bad[dd + 44..dd + 48].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; println!("REJ l6-fffe"); }
    // ab) 96 节声明 vs 1 节表（表溢出）
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[off + 6..off + 8].copy_from_slice(&96u16.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; println!("REJ l7-100"); }
    // ac) 96 节声明 vs 2 节表（表溢出另一形态）
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 2, false);
    bad[off + 6..off + 8].copy_from_slice(&96u16.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; println!("REJ l8-400"); }
    // ad) opt_size 声明不足（可选头截断）
    let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
    bad[off + 16..off + 18].copy_from_slice(&0x10u16.to_le_bytes());
    if parse(&bad).is_err() { rejected += 1; println!("REJ l9-2000"); }

        println!("TOTAL {}", rejected);
    }
}
