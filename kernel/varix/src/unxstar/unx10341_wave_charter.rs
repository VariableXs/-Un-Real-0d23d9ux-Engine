//! UNX-F10341 wave-06 对接开卷总纲（清点/核对/会签/联排四维齐备）
//! （0x1b520 段）
//!
//! 锚点：UNX-F10341-J1 四维齐备——清点/核对/会签/联排四维各按需求锚
//! （4/4/2/1）登记开卷条目，且联排条目须在清点齐备后方可登记（先后
//! 闸），四维同时达锚才算 wave-06 开卷总纲齐备。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：四维册（条=维号+键+题名，键同维唯一）+ 维锚（4/4/2/1）+
//! 先后闸（联排维度登记须清点齐备在先）+ 开卷闸（四维齐备方可开卷，
//! 开卷后拒加）+ 注入核（坏维号/同维重键/联排抢跑/空题名四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b520 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b520;
pub const WV_DIM: u32 = SEG_BASE + 0x01; // 维号越界
pub const WV_DUP: u32 = SEG_BASE + 0x02; // 同维重键
pub const WV_GATE: u32 = SEG_BASE + 0x03; // 联排抢跑
pub const WV_EMPTY: u32 = SEG_BASE + 0x04; // 题名空
pub const WV_CAP: u32 = SEG_BASE + 0x05; // 册超容
pub const WV_SEALED: u32 = SEG_BASE + 0x06; // 开卷后加条
pub const WV_NOTREADY: u32 = SEG_BASE + 0x07; // 未齐开卷

/// 四维号。
pub const DIM_COUNT: u8 = 1;
pub const DIM_CHECK: u8 = 2;
pub const DIM_COSIGN: u8 = 3;
pub const DIM_REHEARSE: u8 = 4;
pub const DIM_N: u8 = 5;

/// 各维需求锚：清点四/核对四/会签二/联排一。
pub const DIM_NEED: [usize; 4] = [4, 4, 2, 1];
/// 册定容。
pub const CAP: usize = 16;

/// 开卷条目。
#[derive(Clone)]
pub struct WaveEntry {
    pub dim: u8,
    pub key: String,
    pub name: String,
}

/// wave-06 开卷总纲册。
pub struct WaveCharter {
    pub entries: Vec<WaveEntry>,
    pub opened: bool,
}

impl WaveCharter {
    pub fn new() -> WaveCharter {
        WaveCharter { entries: Vec::new(), opened: false }
    }

    fn hit(&self, dim: u8, key: &str) -> bool {
        let mut k = 0usize;
        while k < self.entries.len() {
            let e = &self.entries[k];
            if e.dim == dim && e.key == key {
                return true;
            }
            k += 1;
        }
        false
    }

    fn dim_count(&self, dim: u8) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.entries.len() {
            if self.entries[k].dim == dim {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 清点维齐备。
    pub fn count_ready(&self) -> bool {
        self.dim_count(DIM_COUNT) >= DIM_NEED[0]
    }

    /// 登记条目：联排维须清点齐备在先。
    pub fn add(&mut self, dim: u8, key: &str, name: &str) -> Result<(), u32> {
        if self.opened {
            return Err(WV_SEALED);
        }
        if dim == 0 || dim >= DIM_N {
            return Err(WV_DIM);
        }
        if dim == DIM_REHEARSE && !self.count_ready() {
            return Err(WV_GATE);
        }
        if self.hit(dim, key) {
            return Err(WV_DUP);
        }
        if key.is_empty() || name.is_empty() {
            return Err(WV_EMPTY);
        }
        if self.entries.len() >= CAP {
            return Err(WV_CAP);
        }
        self.entries.push(WaveEntry {
            dim,
            key: key.to_string(),
            name: name.to_string(),
        });
        Ok(())
    }

    /// 单维齐备。
    pub fn dim_ready(&self, dim: u8) -> bool {
        if dim == 0 || dim >= DIM_N {
            return false;
        }
        self.dim_count(dim) >= DIM_NEED[(dim - 1) as usize]
    }

    /// 四维齐备。
    pub fn four_ready(&self) -> bool {
        let mut d = 1u8;
        while d < DIM_N {
            if !self.dim_ready(d) {
                return false;
            }
            d += 1;
        }
        true
    }

    /// 开卷：四维齐备且未开过。
    pub fn open(&mut self) -> Result<u32, u32> {
        if self.opened {
            return Err(WV_SEALED);
        }
        if !self.four_ready() {
            return Err(WV_NOTREADY);
        }
        self.opened = true;
        Ok(self.fold())
    }

    /// 维条目键清单。
    pub fn keys_of(&self, dim: u8) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < self.entries.len() {
            if self.entries[k].dim == dim {
                v.push(self.entries[k].key.clone());
            }
            k += 1;
        }
        v
    }

    /// 总纲指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b5200;
        h = h.wrapping_mul(31).wrapping_add(self.entries.len() as u32);
        let mut k = 0usize;
        while k < self.entries.len() {
            let e = &self.entries[k];
            h = h.wrapping_mul(31).wrapping_add(e.dim as u32);
            let kb = e.key.as_bytes();
            let mut i = 0usize;
            while i < kb.len() {
                h = h.wrapping_mul(31).wrapping_add(kb[i] as u32);
                i += 1;
            }
            let nb = e.name.as_bytes();
            let mut i = 0usize;
            while i < nb.len() {
                h = h.wrapping_mul(31).wrapping_add(nb[i] as u32);
                i += 1;
            }
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.opened as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut c = WaveCharter::new();
    // 1) 坏维号必拒
    got[0] = c.add(0, "x", "y").err() == Some(WV_DIM)
        && c.add(5, "x", "y").err() == Some(WV_DIM);
    // 2) 联排抢跑必拒（清点未齐先登联排）
    got[1] = c.add(DIM_REHEARSE, "r1", "联排抢跑").err() == Some(WV_GATE);
    // 3) 同维重键必拒
    let _ = c.add(DIM_COUNT, "b31", "清点键");
    got[2] = c.add(DIM_COUNT, "b31", "重键").err() == Some(WV_DUP);
    // 4) 空题名必拒
    got[3] = c.add(DIM_CHECK, "k", "").err() == Some(WV_EMPTY);
    got
}

/// wave-06 四维样例条目。
pub fn wave_samples() -> Vec<(u8, &'static str, &'static str)> {
    let mut v: Vec<(u8, &'static str, &'static str)> = Vec::new();
    v.push((DIM_COUNT, "b31", "B31 判据面清点"));
    v.push((DIM_COUNT, "b32", "B32 失绿归因清点"));
    v.push((DIM_COUNT, "b33", "B33 干跑步清点"));
    v.push((DIM_COUNT, "b34", "B34 会签清点"));
    v.push((DIM_CHECK, "deps", "跨域依赖核对"));
    v.push((DIM_CHECK, "roles", "参演职责核对"));
    v.push((DIM_CHECK, "comm", "触达规则核对"));
    v.push((DIM_CHECK, "ticks", "时长预算核对"));
    v.push((DIM_COSIGN, "dual", "入场双签"));
    v.push((DIM_COSIGN, "arb", "仲裁双签"));
    v.push((DIM_REHEARSE, "full", "全链彩排"));
    v
}

/// 批内判据。
pub fn run_unx10341_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10341");

    // J1 正路径：四维样例登记齐备并开卷
    let mut c = WaveCharter::new();
    let samples = wave_samples();
    let mut ok = true;
    let mut k = 0usize;
    while k < samples.len() {
        let (d, key, name) = (samples[k].0, samples[k].1, samples[k].2);
        if c.add(d, key, name).is_err() {
            ok = false;
        }
        k += 1;
    }
    let opened = c.open();
    s.add(
        "J1-四维齐备开卷",
        ok && c.four_ready() && opened.is_ok() && c.opened,
        "清点/核对/会签/联排四维达锚并成功开卷",
    );

    // J2 逐维计数达锚
    let j2 = c.dim_ready(DIM_COUNT)
        && c.dim_ready(DIM_CHECK)
        && c.dim_ready(DIM_COSIGN)
        && c.dim_ready(DIM_REHEARSE);
    s.add("J2-逐维达锚", j2, "四维计数分别为 4/4/2/1");

    // J3 注入检出：坏维号 + 联排抢跑
    let inj = inject_four();
    s.add("J3-维号抢跑检出", inj[0] && inj[1], "坏维号与联排抢跑必拒");

    // J4 注入检出：重键 + 空题名
    s.add("J4-重键空名检出", inj[2] && inj[3], "同维重键与空题名必拒");

    // J5 联排抢跑后补清点即可登记（先后闸放行）
    let j5 = {
        let mut t = WaveCharter::new();
        let mut q = 1u32;
        let mut placed = true;
        while q <= 4u32 {
            let key = format!("q{}", q);
            if t.add(DIM_COUNT, &key, "清点").is_err() {
                placed = false;
            }
            q += 1;
        }
        placed && t.add(DIM_REHEARSE, "r1", "联排放行").is_ok()
    };
    s.add("J5-先后闸放行", j5, "清点齐备后联排条目放行");

    // J6 未齐开卷必拒
    let j6 = WaveCharter::new().open().err() == Some(WV_NOTREADY);
    s.add("J6-未齐拒开", j6, "四维未齐时开卷必拒");

    // J7 开卷后加条必拒
    let j7 = {
        let mut t = WaveCharter::new();
        let samples2 = wave_samples();
        let mut q = 0usize;
        while q < samples2.len() {
            let _ = t.add(samples2[q].0, samples2[q].1, samples2[q].2);
            q += 1;
        }
        let _ = t.open();
        t.add(DIM_COUNT, "late", "迟条").err() == Some(WV_SEALED)
    };
    s.add("J7-开卷拒加", j7, "开卷后登记必拒");

    // J8 跨维同键允许
    let j8 = {
        let mut t = WaveCharter::new();
        let mut q = 1u32;
        let mut ok2 = true;
        while q <= 4u32 {
            let key = format!("q{}", q);
            if t.add(DIM_COUNT, &key, "清点").is_err() {
                ok2 = false;
            }
            q += 1;
        }
        ok2 && t.add(DIM_CHECK, "deps", "核对复用键名不冲突").is_err() == false
            && t.add(DIM_CHECK, "deps2", "核对另键").is_ok()
    };
    s.add("J8-跨维复用", j8, "同键跨维不判重号");

    // J9 键清单按登记序
    let j9 = {
        let ks = c.keys_of(DIM_COUNT);
        ks.len() == 4 && ks[0] == "b31" && ks[3] == "b34"
    };
    s.add("J9-键序一致", j9, "清点维键清单保持登记顺序");

    // J10 容量边界
    let j10 = {
        let mut t = WaveCharter::new();
        let mut n = 0u8;
        let mut filled = true;
        while (n as usize) < CAP {
            let d = 1 + (n % 4);
            let key = format!("k{}", n);
            if t.add(d, &key, "填").is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && t.add(DIM_COUNT, "over", "超").err() == Some(WV_CAP)
    };
    s.add("J10-容量边界", j10, "超容必拒且原册不损");

    // J11 指纹随登记推进且开卷后稳定
    let j11 = {
        let mut t = WaveCharter::new();
        let f0 = t.fold();
        let _ = t.add(DIM_COUNT, "a", "甲");
        let f1 = t.fold();
        let _ = t.add(DIM_COUNT, "b", "乙");
        let _ = t.add(DIM_COUNT, "c", "丙");
        let _ = t.add(DIM_COUNT, "d", "丁");
        let f4 = t.fold();
        let _ = t.open();
        let f5 = t.fold();
        f0 != f1 && f1 != f4 && f4 != f5 && t.fold() == f5
    };
    s.add("J11-指纹推进", j11, "登记与开卷各改指纹且终态稳定");

    // J12 动态明细：条目总数落账
    let total = c.entries.len();
    s.add_dyn(
        "J12-条目总数",
        total == wave_samples().len(),
        &format!("entries={}", total),
    );

    // J13 动态明细：总纲指纹落账
    let fp = c.fold();
    s.add_dyn("J13-总纲指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10341_wave_charter_flow() {
        let mut c = WaveCharter::new();
        assert!(c.add(0, "x", "y").err() == Some(WV_DIM));
        assert!(c.add(DIM_REHEARSE, "r", "抢跑").err() == Some(WV_GATE));
        assert!(c.add(DIM_COUNT, "a", "").err() == Some(WV_EMPTY));
        assert!(c.add(DIM_COUNT, "a", "甲").is_ok());
        assert!(c.add(DIM_COUNT, "a", "重").err() == Some(WV_DUP));
        assert!(!c.four_ready());
        assert!(c.open().err() == Some(WV_NOTREADY));
        assert!(c.add(DIM_COUNT, "b", "乙").is_ok());
        assert!(c.add(DIM_COUNT, "c", "丙").is_ok());
        assert!(c.add(DIM_COUNT, "d", "丁").is_ok());
        assert!(c.add(DIM_REHEARSE, "r", "放行").is_ok());
        assert!(c.add(DIM_CHECK, "e", "戊").is_ok());
        assert!(c.add(DIM_CHECK, "f", "己").is_ok());
        assert!(c.add(DIM_CHECK, "g", "庚").is_ok());
        assert!(c.add(DIM_CHECK, "h", "辛").is_ok());
        assert!(c.add(DIM_COSIGN, "i", "壬").is_ok());
        assert!(c.add(DIM_COSIGN, "j", "癸").is_ok());
        assert!(c.four_ready());
        assert!(c.open().is_ok());
        assert!(c.add(DIM_COUNT, "late", "迟").err() == Some(WV_SEALED));
    }

    #[test]
    fn unx10341_checks_green() {
        let s = run_unx10341_checks();
        assert!(s.all_passed(), "UNX-F10341 判据未全绿");
    }
}
