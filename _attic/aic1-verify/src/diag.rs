//! 临时诊断测试（AI-C1 · 用后即删）：展开 CheckSet 层失败的具体原因。
#[cfg(test)]
mod diag {
    use crate::compatstar::condrv::{VtParser, VtAction, vt_test_suite};
    use crate::compatstar::persrc::{parse_version_info, pick_best_icon, IconEntry};

    #[test]
    fn vt_suite_mismatch_detail() {
        let suite = vt_test_suite();
        for (idx, (input, expected)) in suite.iter().enumerate() {
            let mut p = VtParser::new();
            let mut last: Option<VtAction> = None;
            for &b in *input {
                if let Some(a) = p.feed(b) {
                    last = Some(a);
                }
            }
            let ok = match (&last, expected) {
                (Some(VtAction::Print(a)), VtAction::Print(b)) => a == b,
                (Some(VtAction::MoveCursor { row: r1, col: c1, dx: x1, dy: y1 }), VtAction::MoveCursor { row: r2, col: c2, dx: x2, dy: y2 }) => r1 == r2 && c1 == c2 && x1 == x2 && y1 == y2,
                (Some(VtAction::EraseDisplay(m1)), VtAction::EraseDisplay(m2)) => m1 == m2,
                (Some(VtAction::EraseLine(m1)), VtAction::EraseLine(m2)) => m1 == m2,
                (Some(VtAction::SelectGraphicRendition(p1, n1)), VtAction::SelectGraphicRendition(p2, n2)) => p1[..*n1] == p2[..*n2],
                (Some(VtAction::SetTitle(_)), VtAction::SetTitle(_)) => true,
                (Some(VtAction::Unknown), VtAction::Unknown) => true,
                (Some(VtAction::CarriageReturn), VtAction::CarriageReturn) => true,
                (Some(VtAction::LineFeed), VtAction::LineFeed) => true,
                _ => false,
            };
            if !ok {
                println!("MISMATCH case {} input={:?} got={:?} want={:?}", idx, core::str::from_utf8(input), last, expected);
            }
        }
    }

    #[test]
    fn version_parse_detail() {
        let mut ver = Vec::new();
        for (k, v) in [("ProductName", "VarixPad"), ("FileVersion", "2.3.1"), ("CompanyName", "Open Source")] {
            for &c in k.as_bytes() {
                ver.extend_from_slice(&[c, 0]);
            }
            ver.extend_from_slice(&[0, 0, 0, 0]);
            for &c in v.as_bytes() {
                ver.extend_from_slice(&[c, 0]);
            }
            ver.extend_from_slice(&[0, 0, 0, 0]);
        }
        let vi = parse_version_info(&ver);
        println!("product={:?} ver={:?} company={:?}", vi.product_name(), vi.file_version_str(), vi.company_name());
    }

    #[test]
    fn rva_to_off_detail() {
        use crate::compatstar::peblend::*;
        let (bytes, dir_rva, dir_size) = {
            // 复刻 build_with_imports 的返回
            let mut v = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
            let name_off = 0x428usize;
            let thunk_off = 0x438usize;
            let hint_off = 0x450usize;
            let dll = b"KERNEL32.dll";
            v[name_off..name_off + dll.len()].copy_from_slice(dll);
            v[name_off + dll.len()] = 0;
            let hint_rva = hint_off as u32;
            v[thunk_off..thunk_off + 8].copy_from_slice(&(hint_rva as u64).to_le_bytes());
            v[thunk_off + 8..thunk_off + 16]
                .copy_from_slice(&(0x8000_0000_0000_0000u64 | 42).to_le_bytes());
            let sym = b"CreateFileW";
            v[hint_off..hint_off + 2].copy_from_slice(&0u16.to_le_bytes());
            v[hint_off + 2..hint_off + 2 + sym.len()].copy_from_slice(sym);
            v[hint_off + 2 + sym.len()] = 0;
            let name_rva = name_off as u32;
            let thunk_rva = thunk_off as u32;
            let desc_off = 0x400usize;
            v[desc_off..desc_off + 4].copy_from_slice(&thunk_rva.to_le_bytes());
            v[desc_off + 4..desc_off + 8].copy_from_slice(&0u32.to_le_bytes());
            v[desc_off + 8..desc_off + 12].copy_from_slice(&0u32.to_le_bytes());
            v[desc_off + 12..desc_off + 16].copy_from_slice(&name_rva.to_le_bytes());
            v[desc_off + 16..desc_off + 20].copy_from_slice(&thunk_rva.to_le_bytes());
            (v, desc_off as u32, 40u32)
        };
        let img = parse(&bytes).unwrap();
        println!("sections={} s0.rva={:#x} s0.filesz={:#x} s0.raw={:#x} img_size={:#x}",
            img.section_count,
            img.sections()[0].rva, img.sections()[0].filesz,
            img.sections()[0].raw_offset, img.size_of_image);
        println!("rva_to_off(dir_rva={:#x}) = {:?}", dir_rva, rva_to_off(&img, dir_rva as u64));
        let imports = parse_imports(&bytes, &img, dir_rva, dir_size);
        println!("parse_imports = {:?}", imports.as_ref().map(|v| v.len()));
    }

    #[test]
    fn icon_pick_detail() {
        let entries = [
            IconEntry { width: 32, height: 32, bit_count: 32, bytes: 4096 },
            IconEntry { width: 64, height: 64, bit_count: 32, bytes: 16384 },
            IconEntry { width: 256, height: 256, bit_count: 32, bytes: 262144 },
        ];
        let (b, up) = pick_best_icon(&entries, 64);
        println!("target=64 -> bytes={:?} up={}", b.map(|x| x.bytes), up);
        let (b2, up2) = pick_best_icon(&entries, 48);
        println!("target=48 -> bytes={:?} up={}", b2.map(|x| x.bytes), up2);
    }

    #[test]
    fn lnk_sample2_detail() {
        use crate::compatstar::lnkfile::*;
        let lnk = build_lnk(
            LF_HAS_REL_PATH | LF_HAS_ICON_LOCATION | LF_IS_UNICODE,
            b"",
            b"C:\\Apps\\\x50\x00\x51\x00.exe",
            b"",
            b"",
            Some((b"C:\\Apps\\icons.dll", 7)),
        );
        match parse_lnk(&lnk) {
            Ok(p) => println!("target={:?} icon={:?} idx={} hk={}", core::str::from_utf8(&p.target[..p.target_len]), core::str::from_utf8(&p.icon_path[..p.icon_path_len]), p.icon_index, p.hotkey),
            Err(e) => println!("ERR={:?}", e),
        }
    }
}

#[cfg(test)]
mod diag2 {
    use crate::compatstar::peblend::*;
    use crate::compatstar::pebind::*;
    use crate::compatstar::envsess::*;

    #[test]
    fn peblend_adversarial_count() {
        // 复刻 run_peblend_checks 的对抗集，逐项打印接受/拒绝
        let mut rejected = 0u32;
        let mut n = 0u32;
        for x in [0usize, 8, 0x3F] { n += 1; if parse(&vec![0u8; x]).is_err() { rejected += 1; } }
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
        bad[0] = b'X'; n += 1; if parse(&bad).is_err() { rejected += 1; }
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
        bad[0x3C..0x40].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); n += 1; if parse(&bad).is_err() { rejected += 1; }
        let off = 0x80usize + 4;
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
        bad[off] = b'Q'; n += 1; if parse(&bad).is_err() { rejected += 1; }
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
        bad[off + 4..off + 6].copy_from_slice(&0x014Cu16.to_le_bytes()); n += 1; if parse(&bad).is_err() { rejected += 1; }
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
        bad[off + 24..off + 26].copy_from_slice(&0x10Bu16.to_le_bytes()); n += 1; if parse(&bad).is_err() { rejected += 1; }
        n += 1; if parse(&build_static_pe(SUBSYSTEM_GUI, 4096, 0, false)).is_err() { rejected += 1; }
        for sub in [0u16, 5, 7, 10, 16] { n += 1; if parse(&build_static_pe(sub, 4096, 1, false)).is_err() { rejected += 1; } }
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
        bad[off + 24 + 16..off + 24 + 20].copy_from_slice(&0xF000u32.to_le_bytes()); n += 1; if parse(&bad).is_err() { rejected += 1; }
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, true);
        let dd = off + 24 + 112;
        bad[dd + 44..dd + 48].copy_from_slice(&((RELOC_BUDGET_BYTES as u32) + 1).to_le_bytes()); n += 1; if parse(&bad).is_err() { rejected += 1; }
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 1, false);
        bad[off + 24 + 240 + 16..off + 24 + 240 + 20].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); n += 1; if parse(&bad).is_err() { rejected += 1; }
        let mut bad = build_static_pe(SUBSYSTEM_GUI, 4096, 2, false);
        bad[off + 6..off + 8].copy_from_slice(&96u16.to_le_bytes()); n += 1; if parse(&bad).is_err() { rejected += 1; }
        println!("base: {}/{}", rejected, n);
    }

    #[test]
    fn peblend_reloc_detail() {
        let img = parse(&build_static_pe(SUBSYSTEM_GUI, 4096, 1, false)).unwrap();
        let mut mapped = vec![0u8; 0x11_0000];
        let blk: [u8; 16] = {
            let mut b = [0u8; 16];
            b[0..4].copy_from_slice(&0x1000u32.to_le_bytes());
            b[4..8].copy_from_slice(&16u32.to_le_bytes());
            b[8..10].copy_from_slice(&((REL_HIGHLOW as u16) << 12 | 0x000).to_le_bytes());
            b[10..12].copy_from_slice(&((REL_DIR64 as u16) << 12 | 0x020).to_le_bytes());
            b
        };
        mapped[0x2000..0x2010].copy_from_slice(&blk);
        mapped[0x1000..0x1004].copy_from_slice(&0x0040_0100u32.to_le_bytes());
        mapped[0x1020..0x1028].copy_from_slice(&0x0040_0200u64.to_le_bytes());
        let mut image = img;
        image.reloc_dir = Some((0x2000, 16));
        match apply_relocations(&mut mapped, &image, 0x10_0000) {
            Ok(rep) => {
                let u32v = u32::from_le_bytes(mapped[0x1000..0x1004].try_into().unwrap());
                let u64v = u64::from_le_bytes(mapped[0x1020..0x1028].try_into().unwrap());
                println!("rep={:?} u32={:#x} u64={:#x}", rep, u32v, u64v);
            }
            Err(e) => println!("ERR={:?}", e),
        }
    }

    #[test]
    fn pebind_rate_detail() {
        let keys: Vec<BindKey> = (0..200u64).map(|i| BindKey::new(0x1000 + i, 0x5555 + i, 1)).collect();
        let mut mixed_keys = keys.clone();
        for i in 0..40usize {
            mixed_keys[i] = BindKey::new(mixed_keys[i].module_hash, mixed_keys[i].symbol_hash, 2);
        }
        let mut cache2 = BindTable::new(2);
        let mut wal2 = Wal::new();
        let _ = resolve_cost_us(&mut cache2, &mixed_keys, &mut wal2);
        cache2.reset_stats();
        let _ = resolve_cost_us(&mut cache2, &mixed_keys, &mut wal2);
        println!("hits={} misses={} rate={:?}", cache2.hits(), cache2.misses(), cache2.hit_rate_permille());
    }

    #[test]
    fn envsess_cycle_detail() {
        let mut t3 = EnvTable::new();
        let _ = t3.set("X", b"<%Y>", Scope::User);
        let _ = t3.set("Y", b"(%X)", Scope::User);
        let expanded = t3.expand("%X%");
        println!("expanded={:?} trunc={}", expanded, t3.cycle_truncations);
    }
}
