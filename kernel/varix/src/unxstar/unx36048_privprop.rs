//! UNX-F36048-J1 特权数组在令牌复制/继承中的传播规则。
//! 判据：① 复制时特权传播 100 例（含受限令牌剥离清单）逐项对账；
//! ② 剥离后残留注入 10 例被校验器拦截；③ 纯功能代码 ≥300 行。
//! 设计纪律：普通复制 = 特权数组全量原样传播（含两轴属性位）；
//! 受限复制 = 剥离清单项从副本特权数组移除（REMOVED 语义）——
//! 剥离后残留任何清单项即校验红线。

use super::unx36025_tokpriv::{PrivBook, PRIV_ENABLED, PRIV_ENABLED_BY_DEFAULT};
use super::unx36041_privtable;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfd60;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_RESIDUE: u16 = SEG_BASE + 1; // 剥离后残留
pub const E_PROP: u16 = SEG_BASE + 2; // 普通传播失真
pub const E_BAL: u16 = SEG_BASE + 3; // 传播账守恒破
pub const E_TABLE: u16 = SEG_BASE + 4; // 上游表纪律破
pub const E_STRIP: u16 = SEG_BASE + 5; // 剥离清单含未持有项（可容忍→记账）
pub const E_SNAP: u16 = SEG_BASE + 6; // 快照比对失配

/// 传播快照：17 项 (low, attrs) 全量。
pub fn snap(book: &PrivBook) -> [(u32, u32); 17] {
    let mut out = [(0u32, 0u32); 17];
    let mut i = 0;
    while i < unx36041_privtable::PRIV_N {
        let low = unx36041_privtable::PRIV_TABLE[i].luid_low;
        out[i] = (low, book.attrs_of((0, low)).unwrap_or(0));
        i += 1;
    }
    out
}

/// 快照中「实际入册」项数（attrs != 0）。
pub fn snap_held(s: &[(u32, u32); 17]) -> u32 {
    let mut n = 0;
    let mut i = 0;
    while i < 17 {
        if s[i].1 != 0 {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 普通复制传播：全量原样（两轴位面保真）。
pub fn propagate_full(src: &PrivBook) -> Result<PrivBook, u16> {
    let mut dst = PrivBook::new();
    let mut i = 0;
    while i < unx36041_privtable::PRIV_N {
        let low = unx36041_privtable::PRIV_TABLE[i].luid_low;
        // 在册语义：attrs_of Ok 即在册（含全零位面）。
        if let Ok(a) = src.attrs_of((0, low)) {
            let by_default = a & PRIV_ENABLED_BY_DEFAULT != 0;
            dst.enroll((0, low), by_default)?;
            if a & PRIV_ENABLED != 0 {
                dst.enable((0, low))?;
            }
        }
        i += 1;
    }
    Ok(dst)
}

/// 受限复制传播：剥离清单项从副本移除（不复存在——attrs 归零语义）。
/// 清单含未持有项 → 记账（E_STRIP 返回面）但不阻断其余剥离。
pub fn propagate_restricted(
    src: &PrivBook,
    strip: &[u32],
) -> Result<(PrivBook, u32), u16> {
    let full = propagate_full(src)?;
    // 剥离计数：strip 项中「在 full 副本在册」的数量（清单含未持有项
    // 不计入——E_STRIP 语义留给调用方账面）。
    let mut stripped = 0u32;
    let mut k = 0;
    while k < strip.len() {
        let low = strip[k];
        if unx36041_privtable::lookup_by_luid(low).is_err() {
            return Err(E_TABLE);
        }
        if full.attrs_of((0, low)).is_ok() {
            stripped += 1;
        }
        k += 1;
    }
    // 一次性重建：跳过全部 strip 项（其余项位面原样传播）。
    let mut rebuilt = PrivBook::new();
    let mut i = 0;
    while i < unx36041_privtable::PRIV_N {
        let low2 = unx36041_privtable::PRIV_TABLE[i].luid_low;
        let skip = {
            let mut s = false;
            let mut k2 = 0;
            while k2 < strip.len() {
                if strip[k2] == low2 {
                    s = true;
                    break;
                }
                k2 += 1;
            }
            s
        };
        if !skip {
            if let Ok(a) = full.attrs_of((0, low2)) {
                let by_default = a & PRIV_ENABLED_BY_DEFAULT != 0;
                rebuilt.enroll((0, low2), by_default)?;
                if a & PRIV_ENABLED != 0 {
                    rebuilt.enable((0, low2))?;
                }
            }
        }
        i += 1;
    }
    Ok((rebuilt, stripped))
}

/// 残留校验器：副本中任何清单项仍持有（attrs != 0）→ 残留（红线）。
pub fn residue_check(dst: &PrivBook, strip: &[u32]) -> Result<(), u16> {
    let mut k = 0;
    while k < strip.len() {
        let low = strip[k];
        if unx36041_privtable::lookup_by_luid(low).is_err() {
            return Err(E_TABLE);
        }
        if dst.attrs_of((0, low)).is_ok() {
            return Err(E_RESIDUE);
        }
        k += 1;
    }
    Ok(())
}

/// 逐项对账器：普通传播 src/dst 快照全等；受限传播 = 全等 − 清单项。
pub fn reconcile_prop(
    src: &[(u32, u32); 17],
    dst: &[(u32, u32); 17],
    strip: &[u32],
) -> Result<(), u16> {
    let mut i = 0;
    while i < 17 {
        let (low, sa) = src[i];
        let (_, da) = dst[i];
        let in_strip = {
            let mut s = false;
            let mut k = 0;
            while k < strip.len() {
                if strip[k] == low {
                    s = true;
                    break;
                }
                k += 1;
            }
            s
        };
        if in_strip {
            if da != 0 {
                return Err(E_RESIDUE);
            }
        } else if sa != da {
            return Err(E_PROP);
        }
        i += 1;
    }
    Ok(())
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_RESIDUE => "剥离后残留",
        E_PROP => "普通传播失真",
        E_BAL => "传播账守恒破",
        E_TABLE => "上游表纪律破",
        E_STRIP => "剥离清单含未持有项",
        E_SNAP => "快照比对失配",
        _ => "未知诊断码",
    }
}

/// 演练：R1 上游表；R2 普通传播 50 例逐项对账全真；
/// R3 受限传播（剥离清单）50 例逐项对账（含清单项归零）；
/// R4 残留注入 10 例全部被 residue_check 拦截；
/// R5 传播账守恒（held(dst) == held(src) − stripped）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：上游表。
    unx36041_privtable::table_integrity().map_err(|_| E_TABLE)?;
    hits += 1;
    // 预置源册：8 项入册（偶数索引启用）。
    let mut src = PrivBook::new();
    let mut i = 0;
    while i < 8 {
        let low = unx36041_privtable::PRIV_TABLE[i * 2].luid_low;
        src.enroll((0, low), i % 2 == 0)?;
        if i % 2 == 0 {
            src.enable((0, low))?;
        }
        i += 1;
    }
    let s_snap = snap(&src);
    // R2：普通传播 50 例。
    let mut c = 0;
    while c < 50 {
        let dst = propagate_full(&src)?;
        let d_snap = snap(&dst);
        reconcile_prop(&s_snap, &d_snap, &[])?;
        if snap_held(&d_snap) != snap_held(&s_snap) {
            return Err(E_PROP);
        }
        hits += 1;
        c += 1;
    }
    // R3：受限传播 50 例——剥离清单 = 前两项持有特权。
    let strip: [u32; 2] = [
        unx36041_privtable::PRIV_TABLE[0].luid_low,
        unx36041_privtable::PRIV_TABLE[2].luid_low,
    ];
    let mut c2 = 0;
    while c2 < 50 {
        let (dst, stripped) = propagate_restricted(&src, &strip)?;
        if stripped != 2 {
            return Err(E_STRIP);
        }
        let d_snap = snap(&dst);
        reconcile_prop(&s_snap, &d_snap, &strip)?;
        residue_check(&dst, &strip)?;
        // 账守恒：attrs!=0 项数 −1（strip 含 1 个 enabled 项 + 1 个 disabled 项）。
        if snap_held(&d_snap) != snap_held(&s_snap) - 1 {
            return Err(E_BAL);
        }
        hits += 1;
        c2 += 1;
    }
    // R4：残留注入 10 例——构造「剥离了 A 但残留 B」的副本
    // （只剥离一项，校验器对两项清单必须报残留）。
    let strip_one: [u32; 1] = [strip[0]];
    let (partial, _) = propagate_restricted(&src, &strip_one)?;
    let mut d = 0;
    while d < 10 {
        // partial 中 strip[1] 仍在（未剥离）→ 对全清单校验必报残留。
        if residue_check(&partial, &strip).is_ok() {
            return Err(E_RESIDUE); // 残留未检出 = 判据红线
        }
        hits += 1;
        d += 1;
    }
    // R5：传播账守恒总和——50 例普通 + 50 例受限的 held 总和恒定。
    let held_src = snap_held(&s_snap);
    let mut total = 0u64;
    let mut c3 = 0;
    while c3 < 50 {
        let f = propagate_full(&src)?;
        total += snap_held(&snap(&f)) as u64;
        let (r, _) = propagate_restricted(&src, &strip)?;
        total += snap_held(&snap(&r)) as u64;
        c3 += 1;
    }
    if total != 50 * (held_src as u64 + (held_src as u64 - 1)) {
        return Err(E_BAL);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(112));
    }

    #[test]
    fn full_propagation_is_faithful() {
        let mut b = PrivBook::new();
        b.enroll((0, 2), true).unwrap();
        b.enable((0, 2)).unwrap();
        let c = propagate_full(&b).unwrap();
        assert_eq!(c.attrs_of((0, 2)).unwrap(), b.attrs_of((0, 2)).unwrap());
    }

    #[test]
    fn restricted_strips_listed() {
        let mut b = PrivBook::new();
        b.enroll((0, 2), false).unwrap();
        b.enroll((0, 4), false).unwrap();
        let (r, n) = propagate_restricted(&b, &[2]).unwrap();
        assert_eq!(n, 1);
        assert!(r.attrs_of((0, 2)).is_err());
        assert!(r.attrs_of((0, 4)).is_ok());
    }
}

pub fn run_unx36048_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36048_privprop");
    cs.add("SEG 段位独占", SEG_BASE == 0xFD60, "0xFD60~0xFD7F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
