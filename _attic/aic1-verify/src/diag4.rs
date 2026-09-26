// 临时逐案诊断（AI-C1 · 用后即删）：30 个对抗样本逐个打印接受/拒绝。
#[cfg(test)]
mod diag4 {
    use crate::compatstar::peblend::*;

    fn mk() -> Vec<u8> {
        build_static_pe(SUBSYSTEM_GUI, 4096, 1, false)
    }

    #[test]
    fn each_case() {
        let opt = 0x80 + 24;
        let table = opt + OPTIONAL_HDR_SIZE_PE32P;
        let dd = opt + 112;
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("a-len0", vec![0u8; 0]),
            ("a-len8", vec![0u8; 8]),
            ("a-len3f", vec![0u8; 0x3F]),
            ("d-mz", { let mut b = mk(); b[0] = b'X'; b }),
            ("e-lf-hi", { let mut b = mk(); b[0x3C..0x40].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); b }),
            ("f-lf41", { let mut b = mk(); b[0x3C..0x40].copy_from_slice(&0x41u32.to_le_bytes()); b }),
            ("g-pesig", { let mut b = mk(); b[0x80] = b'Q'; b }),
            ("h-i386", { let mut b = mk(); b[0x84..0x86].copy_from_slice(&0x014Cu16.to_le_bytes()); b }),
            ("i-mach0", { let mut b = mk(); b[0x84..0x86].copy_from_slice(&0u16.to_le_bytes()); b }),
            ("j-pe32", { let mut b = mk(); b[0x98..0x9A].copy_from_slice(&0x10Bu16.to_le_bytes()); b }),
            ("k-nsec0", build_static_pe(SUBSYSTEM_GUI, 4096, 0, false)),
            ("l-sub0", build_static_pe(0, 4096, 1, false)),
            ("l-sub5", build_static_pe(5, 4096, 1, false)),
            ("l-sub7", build_static_pe(7, 4096, 1, false)),
            ("l-sub10", build_static_pe(10, 4096, 1, false)),
            ("l-sub16", build_static_pe(16, 4096, 1, false)),
            ("l-subfffe", build_static_pe(0xFFFE, 4096, 1, false)),
            ("l-sub100", build_static_pe(0x100, 4096, 1, false)),
            ("l-sub400", build_static_pe(0x400, 4096, 1, false)),
            ("l-sub2000", build_static_pe(0x2000, 4096, 1, false)),
            ("u-sizeofimg", { let mut b = mk(); b[opt + 56..opt + 60].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); b }),
            ("v-entry-out", { let mut b = mk(); b[opt + 16..opt + 20].copy_from_slice(&0x200000u32.to_le_bytes()); b }),
            ("w-entry-nonexec", { let mut b = mk(); b[opt + 16..opt + 20].copy_from_slice(&0xF000u32.to_le_bytes()); b }),
            ("x-rawsz", { let mut b = mk(); b[table + 16..table + 20].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); b }),
            ("y-rawoff", { let mut b = mk(); b[table + 20..table + 24].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); b }),
            ("z-reloc-budget", { let mut b = build_static_pe(SUBSYSTEM_GUI, 4096, 1, true); b[dd + 44..dd + 48].copy_from_slice(&((RELOC_BUDGET_BYTES as u32) + 1).to_le_bytes()); b }),
            ("aa-reloc-u32", { let mut b = build_static_pe(SUBSYSTEM_GUI, 4096, 1, true); b[dd + 44..dd + 48].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); b }),
            ("ab-96v1", { let mut b = mk(); b[0x86..0x88].copy_from_slice(&96u16.to_le_bytes()); b }),
            ("ac-96v2", { let mut b = build_static_pe(SUBSYSTEM_GUI, 4096, 2, false); b[0x86..0x88].copy_from_slice(&96u16.to_le_bytes()); b }),
            ("ad-optsize", { let mut b = mk(); b[0x90..0x92].copy_from_slice(&0x10u16.to_le_bytes()); b }),
        ];
        for (name, b) in cases {
            let r = parse(&b);
            println!("CASE {} -> {}", name, if r.is_err() { "REJ" } else { "ACCEPTED" });
        }
    }
}
