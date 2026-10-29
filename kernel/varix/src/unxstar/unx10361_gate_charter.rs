//! UNX-F10361 硬闸正演开卷总纲（正演面/回归面/清账面三维齐备）（0x1b7a0 段）
//!
//! 锚点：UNX-F10361-J1 硬闸正演开卷总纲——正演面/回归面/清账面三维
//! 逐条落目（维+键+目名），三维各有下限（3/3/2），清账面必须待回归面
//! 落满方可开落（先回归后清账），三维全满方许开卷。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：总纲册（三维目行）+ 下限核（三维各达下限）+ 闸序核（清账面
//! 待回归面满）+ 开卷核（三维齐才开）+ 注入核（重目/坏维/空名/越闸
//! 四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b7a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b7a0;
pub const GC_DIM: u32 = SEG_BASE + 0x01; // 维号越界
pub const GC_ZERO: u32 = SEG_BASE + 0x02; // 目名空
pub const GC_KEY: u32 = SEG_BASE + 0x03; // 零键
pub const GC_CAP: u32 = SEG_BASE + 0x04; // 册超容
pub const GC_DUP: u32 = SEG_BASE + 0x05; // (维,键) 重目
pub const GC_GATE: u32 = SEG_BASE + 0x06; // 清账面先于回归面
pub const GC_NOTREADY: u32 = SEG_BASE + 0x07; // 三维不齐开卷
pub const GC_SEALED: u32 = SEG_BASE + 0x08; // 开卷后再落

/// 三维。
pub const DIM_FORWARD: u8 = 1;
pub const DIM_REGRESS: u8 = 2;
pub const DIM_CLEARING: u8 = 3;
pub const DIM_N: u8 = 4;

/// 三维下限。
pub const DIM_NEED: [usize; 3] = [3, 3, 2];
/// 册定容。
pub const CAP: usize = 12;

/// 目行：维 + 键 + 目名。
#[derive(Clone)]
pub struct CharterLine {
    pub dim: u8,
    pub key: u32,
    pub name: String,
}

/// 硬闸开卷总纲。
#[derive(Clone)]
pub struct GateCharter {
    pub lines: Vec<CharterLine>,
    pub opened: bool,
}

impl GateCharter {
    /// 空纲。
    pub fn new() -> Self {
        GateCharter {
            lines: Vec::new(),
            opened: false,
        }
    }

    /// 落目：闸序核在清账面（维 3）生效。
    pub fn hold(&mut self, dim: u8, key: u32, name: &str) -> Result<(), u32> {
        if self.opened {
            return Err(GC_SEALED);
        }
        if dim < DIM_FORWARD || dim >= DIM_N {
            return Err(GC_DIM);
        }
        if name.is_empty() {
            return Err(GC_ZERO);
        }
        if key == 0 {
            return Err(GC_KEY);
        }
        if self.lines.len() >= CAP {
            return Err(GC_CAP);
        }
        if dim == DIM_CLEARING && self.dim_count(DIM_REGRESS) < DIM_NEED[1] {
            return Err(GC_GATE);
        }
        let mut i = 0;
        while i < self.lines.len() {
            if self.lines[i].dim == dim && self.lines[i].key == key {
                return Err(GC_DUP);
            }
            i += 1;
        }
        self.lines.push(CharterLine {
            dim,
            key,
            name: name.to_string(),
        });
        Ok(())
    }

    /// 某维目数。
    pub fn dim_count(&self, dim: u8) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.lines.len() {
            if self.lines[i].dim == dim {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 某维达下限。
    pub fn dim_ready(&self, dim: u8) -> bool {
        if dim < DIM_FORWARD || dim >= DIM_N {
            return false;
        }
        self.dim_count(dim) >= DIM_NEED[(dim - 1) as usize]
    }

    /// 三维齐备。
    pub fn three_ready(&self) -> bool {
        let mut d = DIM_FORWARD;
        while d < DIM_N {
            if !self.dim_ready(d) {
                return false;
            }
            d += 1;
        }
        true
    }

    /// 开卷：三维齐才开。
    pub fn open(&mut self) -> Result<(), u32> {
        if self.opened {
            return Err(GC_SEALED);
        }
        if !self.three_ready() {
            return Err(GC_NOTREADY);
        }
        self.opened = true;
        Ok(())
    }

    /// 某维键集。
    pub fn keys_of(&self, dim: u8) -> Vec<u32> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.lines.len() {
            if self.lines[i].dim == dim {
                out.push(self.lines[i].key);
            }
            i += 1;
        }
        out
    }

    /// 某维折叠指纹。
    pub fn dim_fold(&self, dim: u8) -> u32 {
        let mut h: u32 = 0x1b7a01b7 + dim as u32;
        let mut i = 0;
        while i < self.lines.len() {
            if self.lines[i].dim == dim {
                let l = &self.lines[i];
                h = (h ^ l.key).wrapping_mul(0x01000193);
                let bytes = l.name.as_bytes();
                let mut j = 0;
                while j < bytes.len() {
                    h = (h ^ (bytes[j] as u32)).wrapping_mul(0x01000193);
                    j += 1;
                }
            }
            i += 1;
        }
        h
    }

    /// 总折叠指纹。
    pub fn fold(&self) -> u32 {
        self.dim_fold(DIM_FORWARD)
            ^ self
                .dim_fold(DIM_REGRESS)
                .rotate_left(7)
            ^ self
                .dim_fold(DIM_CLEARING)
                .rotate_left(14)
    }
}

/// 总纲落目样例：正演三目/回归三目/清账两目。
pub fn charter_samples() -> [(u8, u32, &'static str); 8] {
    [
        (DIM_FORWARD, 11, "正演面·脚本终版走位"),
        (DIM_FORWARD, 12, "正演面·触达回执闭环"),
        (DIM_FORWARD, 13, "正演面·时序表对表"),
        (DIM_REGRESS, 21, "回归面·释放项入集"),
        (DIM_REGRESS, 22, "回归面·全量跑批"),
        (DIM_REGRESS, 23, "回归面·四计数出数"),
        (DIM_CLEARING, 31, "清账面·失绿归因入账"),
        (DIM_CLEARING, 32, "清账面·发现项闭环"),
    ]
}

/// 注入核：四路坏目必抓。
/// 0: (维,键) 重目；1: 坏维号；2: 清账面越闸；3: 空目名。
pub fn inject_four() -> [bool; 4] {
    let mut c = GateCharter::new();
    let mut k = 0;
    while k < charter_samples().len() {
        let (d, key, name) = charter_samples()[k];
        let _ = c.hold(d, key, name);
        k += 1;
    }
    let d0 = c.hold(DIM_FORWARD, 11, "重目").err() == Some(GC_DUP);
    let d1 = c.hold(0, 41, "坏维").err() == Some(GC_DIM);
    let d2 = {
        let mut g = GateCharter::new();
        g.hold(DIM_CLEARING, 31, "越闸目").err() == Some(GC_GATE)
    };
    let d3 = c.hold(DIM_FORWARD, 14, "").err() == Some(GC_ZERO);
    [d0, d1, d2, d3]
}

/// 判据：run_unx10361_checks。
pub fn run_unx10361_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10361");
    // J1 正路径：八目按样例落纲（正演/回归先行，清账随后）。
    let mut c = GateCharter::new();
    let mut j1 = true;
    let mut k = 0;
    while k < charter_samples().len() {
        let (d, key, name) = charter_samples()[k];
        if c.hold(d, key, name).is_err() {
            j1 = false;
        }
        k += 1;
    }
    s.add("J1-八目落纲", j1 && c.lines.len() == 8, "三维八目全落");
    // J2 边界：坏维号拒。
    let mut c2 = GateCharter::new();
    let j2 = c2.hold(4, 41, "越维").err() == Some(GC_DIM);
    s.add("J2-越维拒", j2, "维号不在 1..=3 即 GC_DIM");
    // J3 边界：零键拒。
    let mut c3 = GateCharter::new();
    let j3 = c3.hold(DIM_FORWARD, 0, "零键").err() == Some(GC_KEY);
    s.add("J3-零键拒", j3, "零键即 GC_KEY");
    // J4 边界：空目名拒。
    let mut c4 = GateCharter::new();
    let j4 = c4.hold(DIM_REGRESS, 21, "").err() == Some(GC_ZERO);
    s.add("J4-空名拒", j4, "空目名即 GC_ZERO");
    // J5 注入：重目必抓。
    let mut c5 = GateCharter::new();
    let _ = c5.hold(DIM_FORWARD, 11, "原目");
    let j5 = c5.hold(DIM_FORWARD, 11, "重目").err() == Some(GC_DUP);
    s.add("J5-重目抓", j5, "同维同键即 GC_DUP");
    // J6 注入：清账面越闸必抓。
    let mut c6 = GateCharter::new();
    let _ = c6.hold(DIM_REGRESS, 21, "回归一");
    let j6 = c6.hold(DIM_CLEARING, 31, "越闸").err() == Some(GC_GATE);
    s.add("J6-越闸抓", j6, "回归面未满清账先落即 GC_GATE");
    // J7 边界：清账面在回归满后可落。
    let mut c7 = GateCharter::new();
    let _ = c7.hold(DIM_REGRESS, 21, "回归一");
    let _ = c7.hold(DIM_REGRESS, 22, "回归二");
    let _ = c7.hold(DIM_REGRESS, 23, "回归三");
    let j7 = c7.hold(DIM_CLEARING, 31, "清账一").is_ok();
    s.add("J7-闸通行", j7, "回归面满三清账放行");
    // J8 边界：三维不齐开卷拒。
    let mut c8 = GateCharter::new();
    let mut k8 = 0;
    while k8 < 6 {
        let (d, key, name) = charter_samples()[k8];
        let _ = c8.hold(d, key, name);
        k8 += 1;
    }
    let j8 = !c8.three_ready() && c8.open().err() == Some(GC_NOTREADY);
    s.add("J8-缺开拒", j8, "清账面缺位开卷即 GC_NOTREADY");
    // J9 正路径：三维齐开卷。
    let mut c9 = c.clone();
    let j9 = c9.three_ready() && c9.open().is_ok() && c9.opened;
    s.add("J9-开卷成", j9, "三维齐开卷通过");
    // J10 注入：开卷后再落拒。
    let mut c10 = c.clone();
    let _ = c10.open();
    let j10 = c10
        .hold(DIM_FORWARD, 14, "后补目")
        .err()
        == Some(GC_SEALED);
    s.add("J10-开后落拒", j10, "开卷后落目即 GC_SEALED");
    // J11 动态：三维目数与键集。
    let kf = c.keys_of(DIM_FORWARD);
    let j11 = kf == vec![11, 12, 13] && c.dim_count(DIM_CLEARING) == 2;
    s.add_dyn(
        "J11-维目出数",
        j11,
        &format!(
            "正演 {} 回归 {} 清账 {}，正演键 {:?}",
            c.dim_count(DIM_FORWARD),
            c.dim_count(DIM_REGRESS),
            c.dim_count(DIM_CLEARING),
            kf
        ),
    );
    // J12 动态：三维指纹互异。
    let f1 = c.dim_fold(DIM_FORWARD);
    let f2 = c.dim_fold(DIM_REGRESS);
    let f3 = c.dim_fold(DIM_CLEARING);
    let j12 = f1 != f2 && f2 != f3 && c.fold() != 0;
    s.add_dyn(
        "J12-指纹互异",
        j12,
        &format!(
            "三维指纹 {:08x}/{:08x}/{:08x}，总指纹 {:08x}",
            f1,
            f2,
            f3,
            c.fold()
        ),
    );
    // J13 边界：册超容拒。
    let mut c13 = GateCharter::new();
    let mut n13 = 0;
    let mut last = true;
    while n13 < CAP {
        let dim = if n13 % 2 == 0 { DIM_FORWARD } else { DIM_REGRESS };
        let key = (n13 + 1) as u32;
        let nm = format!("g{}", n13);
        if c13.hold(dim, key, &nm).is_err() {
            last = false;
        }
        n13 += 1;
    }
    let j13 = last && c13.hold(DIM_CLEARING, 99, "溢目").err() == Some(GC_CAP);
    s.add("J13-超容拒", j13, "第十三条落目即 GC_CAP");
    // J14 注入：inject_four 四路必抓。
    let inj = inject_four();
    let j14 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add("J14-注入抓", j14, "重目/坏维/越闸/空名四路全检出");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_and_open() {
        let mut c = GateCharter::new();
        let mut k = 0;
        while k < charter_samples().len() {
            let (d, key, name) = charter_samples()[k];
            assert!(c.hold(d, key, name).is_ok());
            k += 1;
        }
        assert!(c.three_ready());
        assert!(c.open().is_ok());
        assert!(c.hold(DIM_FORWARD, 14, "后补").err() == Some(GC_SEALED));
        assert!(c.fold() != 0);
        assert_eq!(c.keys_of(DIM_CLEARING), vec![31, 32]);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10361_checks();
        assert!(s.all_passed());
    }
}
