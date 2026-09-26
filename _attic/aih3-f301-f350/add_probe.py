# -*- coding: utf-8 -*-
"""临时探针：实证 LengthGrade 分级与 MemoryAudit 预警带。"""
import io

p = "kernel/varix/src/h3star/copypath.rs"
probe = '''
#[cfg(test)]
mod probe_len {
    use super::*;
    #[test]
    fn probe_length_grades() {
        let p1 = "C:/a.vx";
        let p2 = alloc::format!("C:/{}", "\\u{957f}".repeat(120));
        let p3 = alloc::format!("C:/{}", "\\u{957f}".repeat(200));
        let w = PathNormalizer::win(&p2);
        println!(
            "GRADES p1={:?} p2len={} grade2={:?} p3len={} grade3={:?}",
            LengthGrade::grade(p1),
            w.chars().count(),
            LengthGrade::grade(&p2),
            PathNormalizer::win(&p3).chars().count(),
            LengthGrade::grade(&p3)
        );
        println!(
            "NEAR 50={} 120={} 500={}",
            MemoryAudit::near_limit(50, 4000),
            MemoryAudit::near_limit(120, 4000),
            MemoryAudit::near_limit(500, 4000)
        );
    }
}
'''
with io.open(p, "a", encoding="utf-8", newline="\n") as f:
    f.write(probe)
print("probe added")
