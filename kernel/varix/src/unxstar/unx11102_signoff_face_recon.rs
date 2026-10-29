//! UNX-F11102 联签面总对账（C2 协议全挂点核销）（0x1f582 段）
//!
//! 锚点：UNX-F11102-J1 挂点全账终核对平（10/10），核销零残留断言（10/10）。
//! 域：CoRun·UNX-C4-B36（联签面）。
//!
//! 结构：挂点册（SignoffFace：协议挂点登记 → 挂接 → 核销 → 残留断言）+
//! 单挂点（HangPoint：挂接位 + 核销位）+ 冻结根 + 注入核（未挂先核销/
//! 重复核销/重号登记/残留未清四路必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1f582 独占 0x20）。
pub const SEG_BASE: u32 = 0x1f582;
pub const SF_BADID: u32 = SEG_BASE + 0x01; // 挂点号为 0
pub const SF_DUP: u32 = SEG_BASE + 0x02; // 挂点重号
pub const SF_EMPTY: u32 = SEG_BASE + 0x03; // 协议名空
pub const SF_UNHOOKED: u32 = SEG_BASE + 0x04; // 未挂接先核销
pub const SF_WRITTEN: u32 = SEG_BASE + 0x05; // 重复核销
pub const SF_RESIDUAL: u32 = SEG_BASE + 0x06; // 核销有残留
pub const SF_FROZEN: u32 = SEG_BASE + 0x07; // 冻结后写账
pub const SF_OVER: u32 = SEG_BASE + 0x08; // 超定容

/// 联签面挂点条数（12 个 C2 协议挂点）。
pub const NEEDED: usize = 12;
/// 挂点册定容。
pub const CAP: usize = 16;
/// 联签面标（fold 种子）。
pub const FACE_TAG: u64 = 0x1f5_8282;
/// 指纹步进。
pub const FP_STEP: u64 = 0x100000001b3;

/// 单挂点：协议名 + 挂接位 + 核销位。
#[derive(Clone, Debug, PartialEq)]
pub struct HangPoint {
    pub id: u32,
    pub proto: String,
    pub hooked: bool,
    pub written: bool,
}

impl HangPoint {
    /// 挂点闭合：已挂接且已核销。
    pub fn closed(&self) -> bool {
        self.hooked && self.written
    }

    /// 渲染：挂点号 | 挂接旗 | 核销旗。
    pub fn render(&self) -> String {
        let h = if self.hooked { "H" } else { "-" };
        let w = if self.written { "W" } else { "-" };
        format!("P{}|{}|{}|{}", self.id, h, w, self.proto)
    }
}

/// 联签面挂点册。
#[derive(Clone, Debug)]
pub struct SignoffFace {
    pub points: Vec<HangPoint>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl SignoffFace {
    pub fn new() -> SignoffFace {
        SignoffFace { points: Vec::new(), frozen: false, root: None }
    }

    /// 挂点登记：零号/重号/空名/超容皆拒。
    pub fn register(&mut self, id: u32, proto: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(SF_FROZEN);
        }
        if id == 0 {
            return Err(SF_BADID);
        }
        if proto.is_empty() {
            return Err(SF_EMPTY);
        }
        if self.points.iter().any(|p| p.id == id) {
            return Err(SF_DUP);
        }
        if self.points.len() >= CAP {
            return Err(SF_OVER);
        }
        self.points.push(HangPoint {
            id,
            proto: proto.to_string(),
            hooked: false,
            written: false,
        });
        Ok(())
    }

    fn find(&mut self, id: u32) -> Option<&mut HangPoint> {
        let mut k = 0usize;
        while k < self.points.len() {
            if self.points[k].id == id {
                return Some(&mut self.points[k]);
            }
            k += 1;
        }
        None
    }

    /// 挂接：未登记拒；重复挂接拒。
    pub fn hook(&mut self, id: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SF_FROZEN);
        }
        let p = self.find(id).ok_or(SF_UNHOOKED)?;
        if p.hooked {
            return Err(SF_DUP);
        }
        p.hooked = true;
        Ok(())
    }

    /// 核销：未挂接拒；重复核销拒。
    pub fn write_off(&mut self, id: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SF_FROZEN);
        }
        let p = self.find(id).ok_or(SF_UNHOOKED)?;
        if !p.hooked {
            return Err(SF_UNHOOKED);
        }
        if p.written {
            return Err(SF_WRITTEN);
        }
        p.written = true;
        Ok(())
    }

    /// 残留条数（未核销）。
    pub fn residual(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.points.len() {
            if !self.points[k].written {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 残留点名（按登记序）。
    pub fn residual_ids(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.points.len() {
            if !self.points[k].written {
                v.push(self.points[k].id);
            }
            k += 1;
        }
        v
    }

    /// 未挂接点名。
    pub fn unhooked(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.points.len() {
            if !self.points[k].hooked {
                v.push(self.points[k].id);
            }
            k += 1;
        }
        v
    }

    /// 挂点全账终核：12 点齐、全挂接、零残留。
    pub fn reconciled(&self) -> bool {
        self.points.len() == NEEDED
            && self.unhooked().is_empty()
            && self.residual() == 0
            && self.closed_count() == NEEDED
    }

    pub fn closed_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.points.len() {
            if self.points[k].closed() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 冻结并落根。
    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 面级指纹：挂点号、协议名、两位旗标链式入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = FACE_TAG;
        let mut k = 0usize;
        while k < self.points.len() {
            let p = &self.points[k];
            h = (h ^ p.id as u64).wrapping_mul(FP_STEP);
            h = (h ^ p.hooked as u64).wrapping_mul(FP_STEP);
            h = (h ^ p.written as u64).wrapping_mul(FP_STEP);
            let bytes = p.proto.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            k += 1;
        }
        h
    }
}

/// 协议名样本（确定性）。
pub fn proto_name(id: u32) -> String {
    format!("C2 挂点协议 {}", id)
}

/// 标准面：12 个挂点登记 → 挂接 → 核销。
pub fn build_face() -> (SignoffFace, bool) {
    let mut f = SignoffFace::new();
    let mut ok = true;
    let mut id = 1u32;
    while id <= NEEDED as u32 {
        if f.register(id, &proto_name(id)).is_err() {
            ok = false;
        }
        if f.hook(id).is_err() {
            ok = false;
        }
        if f.write_off(id).is_err() {
            ok = false;
        }
        id += 1;
    }
    (f, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 未挂先核销必拒
    let mut a = SignoffFace::new();
    let _ = a.register(1, "未挂");
    got[0] = a.write_off(1).err() == Some(SF_UNHOOKED) && a.residual() == 1;
    // 2) 重复核销必拒
    let mut b = SignoffFace::new();
    let _ = b.register(2, "重销");
    let _ = b.hook(2);
    let first = b.write_off(2).is_ok();
    got[1] = first && b.write_off(2).err() == Some(SF_WRITTEN);
    // 3) 重号登记必拒
    let mut c = SignoffFace::new();
    let _ = c.register(3, "甲");
    got[2] = c.register(3, "乙").err() == Some(SF_DUP);
    // 4) 残留未清则终核为假
    let mut d = SignoffFace::new();
    let mut id = 1u32;
    while id <= NEEDED as u32 {
        let _ = d.register(id, &proto_name(id));
        let _ = d.hook(id);
        if id != 6 {
            let _ = d.write_off(id);
        }
        id += 1;
    }
    got[3] = !d.reconciled() && d.residual() == 1 && d.residual_ids().contains(&6);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11102_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11102");

    let (mut face, built) = build_face();
    // J1 挂点全账终核对平
    s.add(
        "J1-挂点全账对平",
        built && face.points.len() == NEEDED && face.reconciled(),
        "12 个 C2 协议挂点登记、挂接、核销三面对平",
    );

    // J2 核销零残留
    s.add(
        "J2-核销零残留",
        face.residual() == 0 && face.residual_ids().is_empty() && face.closed_count() == NEEDED,
        "残留条数为零且闭合计数足额",
    );

    // J3 未挂先核销拒
    let j3 = {
        let mut f = SignoffFace::new();
        let _ = f.register(7, "未挂样本");
        f.write_off(7).err() == Some(SF_UNHOOKED) && f.residual() == 1
    };
    s.add("J3-未挂拒核销", j3, "未挂接即核销必拒且残留不消");

    // J4 重复核销拒
    let j4 = {
        let mut f = SignoffFace::new();
        let _ = f.register(8, "重销样本");
        let _ = f.hook(8);
        let _ = f.write_off(8);
        f.write_off(8).err() == Some(SF_WRITTEN)
    };
    s.add("J4-重复核销拒", j4, "同一挂点二次核销必拒");

    // J5 重复挂接拒
    let j5 = {
        let mut f = SignoffFace::new();
        let _ = f.register(9, "重挂样本");
        let _ = f.hook(9);
        f.hook(9).err() == Some(SF_DUP)
    };
    s.add("J5-重复挂接拒", j5, "同一挂点二次挂接必拒");

    // J6 零号与重号登记拒
    let j6 = {
        let mut f = SignoffFace::new();
        let bad = f.register(0, "零号").err() == Some(SF_BADID);
        let _ = f.register(1, "甲");
        bad && f.register(1, "乙").err() == Some(SF_DUP)
    };
    s.add("J6-零号重号拒", j6, "零号挂点与重号登记两路必拒");

    // J7 空协议名拒
    let j7 = {
        let mut f = SignoffFace::new();
        f.register(2, "").err() == Some(SF_EMPTY) && f.points.is_empty()
    };
    s.add("J7-空名拒收", j7, "空协议名登记必拒且不入册");

    // J8 未登记核销拒
    let j8 = {
        let mut f = SignoffFace::new();
        f.write_off(99).err() == Some(SF_UNHOOKED) && f.hook(99).err() == Some(SF_UNHOOKED)
    };
    s.add("J8-未登记拒", j8, "未登记挂点核销与挂接两路必拒");

    // J9 注入检出：未挂核销 + 重复核销
    let inj = inject_four();
    s.add("J9-未挂重销检出", inj[0] && inj[1], "未挂先核销与重复核销两路必抓");

    // J10 注入检出：重号 + 残留
    s.add("J10-重号残留检出", inj[2] && inj[3], "重号登记与残留未清两路必抓");

    // J11 残留点名可枚举
    let j11 = {
        let (f, _) = build_face();
        f.residual_ids().len() == 0 && f.unhooked().is_empty() && f.closed_count() == NEEDED
    };
    s.add("J11-残留点名", j11, "全核销后残留与未挂接点名为空");

    // J12 冻结拒写
    let root = face.seal();
    let j12 = face.register(20, "追挂").err() == Some(SF_FROZEN)
        && face.hook(1).err() == Some(SF_FROZEN)
        && face.write_off(1).err() == Some(SF_FROZEN);
    s.add("J12-冻结拒写", j12, "封存后登记/挂接/核销三路必拒");

    // J13 冻结根稳定
    s.add(
        "J13-冻结根稳定",
        face.root == Some(root) && root == face.fold() && root != 0,
        "联签面冻结根可重算零漂移",
    );

    // J14 渲染面
    let j14 = {
        let (f, _) = build_face();
        f.points[0].render().contains("H")
            && f.points[0].render().contains("W")
            && f.points[0].render() != f.points[1].render()
    };
    s.add("J14-挂点渲染", j14, "已挂已销挂点渲染带 HW 双旗");

    // J15 动态明细
    s.add_dyn(
        "J15-联签明细",
        face.residual() == 0 && face.fold() == root,
        &format!("closed={} residual={} root={:#x}", face.closed_count(), face.residual(), root),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11102_signoff_flow() {
        let (mut f, built) = build_face();
        assert!(built);
        assert!(f.reconciled());
        assert!(f.residual() == 0);
        assert!(f.seal() != 0);
        assert!(f.write_off(1).err() == Some(SF_FROZEN));
        assert!(f.register(30, "x").err() == Some(SF_FROZEN));
    }

    #[test]
    fn unx11102_checks_green() {
        let s = run_unx11102_checks();
        assert!(s.all_passed(), "UNX-F11102 判据未全绿");
    }
}
