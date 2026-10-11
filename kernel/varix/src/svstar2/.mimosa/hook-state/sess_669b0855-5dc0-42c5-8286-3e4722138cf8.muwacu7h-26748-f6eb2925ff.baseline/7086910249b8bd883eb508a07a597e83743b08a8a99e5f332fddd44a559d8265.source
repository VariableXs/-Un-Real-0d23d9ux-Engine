//! VE-F0007 · 域自检（判据逐条对应，见 `vea07_caps.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 标准位图 → `A07-位图-*`
//! - 供应商扩展位隔离声明 → `A07-隔离-*`
//! - 分级降级 + 效果损失量化 → `A07-分级-*`
//! - 实测校准（误报支持→实测校准）→ `A07-校准-*`
//! - 缓存失效 + 驱动更新联动 → `A07-缓存-*`
//! - V01 能力协商复用 → `A07-对接-*`
//! - 位图读屏可达 → `A07-读屏-*`

use super::vea07_caps::*;
use crate::checks::CheckSet;

use alloc::{format, vec};

/// 标准测试输入：自报 5 项、实测 4 项（其中 1 项虚报 + 1 项漏报）。
fn input() -> CapQueryInput {
    let mut claimed = StandardBitmap::default();
    for k in ["raster2d", "raster3d", "compute", "hdr", "ray_trace"] {
        claimed.set(k, true);
    }
    let mut measured = StandardBitmap::default();
    for k in ["raster2d", "raster3d", "compute", "tensor"] {
        measured.set(k, true);
    }
    CapQueryInput {
        slot: "0000:01:00.0".to_string(),
        driver_version: "551.23".to_string(),
        claimed,
        measured,
        vendor: VendorExtensions::pack(0x10DE, 0b0000_0000_0000_0101),
    }
}

/// VE-F0007 域自检。
pub fn run_vea07_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vea07");

    // ---- 判据：标准位图 ----

    // 位序冻结：键→位号映射稳定（契约面）
    {
        let idx_ok = bit_of("raster2d") == Some(0)
            && bit_of("tensor") == Some(11)
            && bit_of("不存在的键") == None;
        let mut b = StandardBitmap::default();
        let set_ok = b.set("hdr", true) && b.get("hdr") == Some(true) && b.0 == 1 << 3;
        // 未知键/越域键拒绝（不猜）
        let reject_ok = !b.set("厂商私有", true) && b.get("厂商私有") == None;
        set.add("A07-位图-位序冻结表", idx_ok && set_ok && reject_ok, "");
    }

    // ---- 判据：供应商扩展位隔离 ----

    {
        let r = query(&mut CapCache::new(), &input());
        // 隔离声明机器面：标准位图高 16 位恒零 + 厂商位在独立结构
        let isolated = r.bitmap.vendor_free()
            && r.vendor.vid == 0x10DE
            && r.vendor.bits == 0b101
            && (r.bitmap.0 >> VENDOR_BITS_BASE) == 0;
        set.add("A07-隔离-厂商位不混入标准位", isolated, "");
    }

    // ---- 判据：实测校准 ----

    {
        let cal = calibrate(&input());
        // 虚报位（hdr/ray_trace 自报有实测无）被清除；漏报位（tensor）按实测保留
        let hdr_cleared = cal.corrected.get("hdr") == Some(false);
        let rt_cleared = cal.corrected.get("ray_trace") == Some(false);
        let tensor_kept = cal.corrected.get("tensor") == Some(true);
        let inflated_named = cal.inflated == vec!["hdr", "ray_trace"];
        let under_named = cal.underreported == vec!["tensor"];
        let audited = cal.audit.len() == 3
            && cal.audit.iter().all(|a| a.contains("下一步"));
        set.add(
            "A07-校准-虚报清除漏报保留",
            hdr_cleared && rt_cleared && tensor_kept && inflated_named && under_named && audited,
            "",
        );
    }
    // 全诚实输入零修正零审计
    {
        let mut claimed = StandardBitmap::default();
        let mut measured = StandardBitmap::default();
        for k in ["raster2d", "compute"] {
            claimed.set(k, true);
            measured.set(k, true);
        }
        let cal = calibrate(&CapQueryInput {
            slot: "s".to_string(),
            driver_version: "1".to_string(),
            claimed,
            measured,
            vendor: VendorExtensions::default(),
        });
        set.add(
            "A07-校准-全诚实零修正",
            cal.inflated.is_empty() && cal.underreported.is_empty() && cal.audit.is_empty(),
            "",
        );
    }

    // ---- 判据：分级降级 + 效果损失量化 ----

    {
        let cal = calibrate(&input());
        let grading = grade(&cal.corrected);
        // 每个标准特性都有条目（缺一即构建期缺陷）
        let complete = grading.len() == FEATURE_KEYS.len();
        // 支持项 Full 且零损失；不支持项必有降级路径 + 量化损失
        let coherent = grading.iter().all(|(_, g)| g.entry_valid());
        // 量化抽查：ray_trace 不支持 → 降级路径 + 0.7 损失
        let rt = grading.iter().find(|(k, _)| *k == "ray_trace").map(|(_, g)| g);
        let rt_ok = rt.map(|g| {
            g.level == Support::None
                && g.degrade_path == Some("屏幕空间反射/探针近似（光线追踪效果转栅格化）")
                && g.effect_loss == 0.7
        });
        // hdr 不支持 → SDR 映射 + 0.6
        let hdr = grading.iter().find(|(k, _)| *k == "hdr").map(|(_, g)| g);
        let hdr_ok = hdr.map(|g| g.level == Support::None && g.effect_loss == 0.6).unwrap_or(false);
        set.add(
            "A07-分级-全特性预登记量化",
            complete && coherent && rt_ok == Some(true) && hdr_ok,
            "",
        );
    }

    // ---- 判据：缓存失效 + 驱动更新联动 ----

    {
        let mut cache = CapCache::new();
        let r1 = query(&mut cache, &input());
        let r2 = query(&mut cache, &input());
        // 同 slot 同驱动 → 第二次命中
        let hit_ok = !r1.cache_hit && r2.cache_hit && cache.hits_total == 1;
        // 驱动一换 → miss（位图重查）
        let mut other = input();
        other.driver_version = "552.10".to_string();
        let r3 = query(&mut cache, &other);
        let requery = !r3.cache_hit && cache.misses == 2 && cache.len() == 2;
        // 语义机检：旧版本仍可查（别的版本在用不删），从未查过的新版本必 miss
        // （注意不能用 552.10 当"新版本"——r3 已把它写入缓存，语义探针要用全新版本）
        let semantic = cache.driver_update_requery_semantics(&input().slot, "551.23", "880.00");
        set.add(
            "A07-缓存-驱动更新联动失效",
            hit_ok && requery && semantic,
            "",
        );
    }
    // 显式失效口
    {
        let mut cache = CapCache::new();
        let _ = query(&mut cache, &input());
        cache.invalidate_all();
        let gone = cache.is_empty() && cache.invalidations == 1;
        let remiss = cache.lookup("0000:01:00.0", "551.23").is_none();
        set.add("A07-缓存-显式失效口", gone && remiss, "");
    }
    // 缓存位图就是校准后的（虚报不进缓存）
    {
        let mut cache = CapCache::new();
        let r = query(&mut cache, &input());
        let cached = cache.lookup(&input().slot, "551.23").unwrap();
        set.add(
            "A07-缓存-存的是校准后位图",
            cached == r.bitmap && cached.get("hdr") == Some(false) && cached.get("tensor") == Some(true),
            "",
        );
    }

    // ---- 判据：V01 能力协商复用 ----

    {
        let cal = calibrate(&input());
        let caps = to_v01_caps(&cal.corrected);
        let all12 = caps.len() == FEATURE_KEYS.len();
        let discrete = caps.iter().all(|(_, v)| *v == 0.0 || *v == 1.0);
        let hdr_zero = caps.iter().find(|(k, _)| *k == "hdr").map(|(_, v)| *v) == Some(0.0);
        set.add("A07-对接-V01能力表", all12 && discrete && hdr_zero, "");
    }

    // ---- 读屏可达 ----

    {
        let r = query(&mut CapCache::new(), &input());
        let ok = r.a11y.contains("GPU 能力查询")
            && r.a11y.contains("551.23")
            && r.a11y.contains("隔离")
            && r.bitmap.describe().contains("标准特性");
        set.add("A07-读屏-位图可播", ok, "");
    }

    // ---- 确定性 ----

    {
        let a = query(&mut CapCache::new(), &input());
        let b = query(&mut CapCache::new(), &input());
        set.add(
            "A07-确定-同输入同报告",
            a.bitmap == b.bitmap && a.grading == b.grading && a.a11y == b.a11y,
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn vea07_checks_all_green() {
        let set = run_vea07_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-A07 域自检红项：{}/{} 绿", passed, passed + failed);
            for it in items.iter().take(n) {
                if let Some(c) = it {
                    if !c.passed {
                        msg.push_str(&format!("\n  [红] {} — {}", c.name, c.detail));
                    }
                }
            }
            panic!("{}", msg);
        }
    }

    /// 校准纪律：虚报位永远进不了缓存与分级。
    #[test]
    fn inflated_never_reaches_grading() {
        let mut cache = CapCache::new();
        let r = query(&mut cache, &input());
        // 输入自报 ray_trace，实测无——报告里必须是 None 档带降级路径
        let rt = r.grading.iter().find(|(k, _)| *k == "ray_trace").unwrap();
        assert_eq!(rt.1.level, Support::None);
        assert!(rt.1.degrade_path.is_some());
        assert!(r.bitmap.get("ray_trace") == Some(false));
    }

    /// 驱动更新联动：换驱动必重查，旧版本条目互不干扰。
    #[test]
    fn driver_bump_forces_requery() {
        let mut cache = CapCache::new();
        let _ = query(&mut cache, &input());
        let mut bumped = input();
        bumped.driver_version = "999.99".to_string();
        bumped.measured = StandardBitmap::default();
        bumped.measured.set("raster2d", true);
        let r2 = query(&mut cache, &bumped);
        assert!(!r2.cache_hit, "换驱动必须重查");
        assert_eq!(cache.len(), 2, "两个版本条目并存");
        assert!(cache.lookup(&input().slot, "551.23").unwrap().get("tensor") == Some(true));
    }

    /// 全诚实校准不产生审计噪音。
    #[test]
    fn honest_input_zero_noise() {
        let mut c = StandardBitmap::default();
        let mut m = StandardBitmap::default();
        for k in FEATURE_KEYS.iter() {
            c.set(k, true);
            m.set(k, true);
        }
        let cal = calibrate(&CapQueryInput {
            slot: "s".to_string(),
            driver_version: "1".to_string(),
            claimed: c,
            measured: m,
            vendor: VendorExtensions::default(),
        });
        assert!(cal.audit.is_empty());
        assert_eq!(cal.corrected, m);
    }
}
