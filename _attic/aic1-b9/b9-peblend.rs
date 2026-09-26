
// ---------------------------------------------------------------------------
// F002 · 深化批次九：数据目录全 16 项扫描（PE Spec 3.4.6——数据目录计数
// 与每项 (RVA, Size) 的「指向文件外」判负扫描：装载安全面在目录粒度上
// 的第一道闸）。
// ---------------------------------------------------------------------------

/// 目录项扫描结果（诚实计数面）。
pub struct DirScanReport {
    pub present: u32,
    pub out_of_image: u32,
    pub zero_entries: u32,
}

/// 扫描数据目录（dirs 为 (rva,size) 对；镜像大小 image_size 判越界——
/// rva 或 rva+size 越出镜像即 out_of_image；rva==0 为缺席项）。
pub fn scan_data_dirs(dirs: &[(u32, u32)], image_size: u32) -> DirScanReport {
    let mut r = DirScanReport { present: 0, out_of_image: 0, zero_entries: 0 };
    for &(rva, size) in dirs {
        if rva == 0 {
            r.zero_entries += 1;
            continue;
        }
        r.present += 1;
        if (rva as u64) + (size as u64) > image_size as u64 {
            r.out_of_image += 1;
        }
    }
    r
}

/// F002 深化批次九自检。
fn run_peblend_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F002-peblend-deep8");
    // 典型 16 项：4 项在位（1 越界）、12 项缺席。
    let dirs: alloc::vec::Vec<(u32, u32)> = {
        let mut v = alloc::vec![(0u32, 0u32); 16];
        v[0] = (0x1000, 0x40); // 导出
        v[1] = (0x2000, 0x80); // 导入
        v[5] = (0xF000, 0x1000); // BASE reloc——恰好触底
        v[6] = (0x9000, 0x2000); // debug——越界（rva+size > 0xA000 假设）
        v
    };
    let rep = scan_data_dirs(&dirs, 0xA000);
    cs.add(
        "dir_scan_counts",
        rep.present == 4
            && rep.zero_entries == 12
            && rep.out_of_image == 1, // 只有 debug 项越界；reloc 恰好在界内
        "",
    );
    // 2) 恰好触底不算越界（rva+size == image_size 是合法终态——半开语义）。
    let exact: alloc::vec::Vec<(u32, u32)> = {
        let mut v = alloc::vec![(0u32, 0u32); 16];
        v[0] = (0x9000, 0x1000);
        v
    };
    let rep2 = scan_data_dirs(&exact, 0xA000);
    cs.add(
        "dir_scan_exact_fit_in_bounds",
        rep2.present == 1 && rep2.out_of_image == 0,
        "",
    );
    // 3) size 溢出不 panic（u64 合算——rva 接近 u32 顶也不误判）。
    let wild: alloc::vec::Vec<(u32, u32)> = {
        let mut v = alloc::vec![(0u32, 0u32); 16];
        v[0] = (0xFFFF_F000, 0x2000);
        v
    };
    let rep3 = scan_data_dirs(&wild, 0x1000);
    cs.add(
        "dir_scan_overflow_safe",
        rep3.out_of_image == 1,
        "",
    );
    cs
}
