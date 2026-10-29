//! UNX-F11098 演习封存与复跑指引（长稳面移交注账）（0x1ec40 段）
//!
//! 锚点：UNX-F11098-J1 演习面封存逐件核销（10/10），复跑指引可执行（10/10）。
//! 域：长稳面移交注账——演习（drill）跑完即封存，封存件逐件核销出移交册，
//! 每件封存件挂一条复跑指引（步骤序、步骤文、预期指纹），指引步骤齐且
//! 指纹可重算才算可执行。
//!
//! 结构：演习封存册（SealBook，演习登记→产物落袋→封存翻位→核销出册）+
//! 复跑指引（RerunGuide，步骤向量，逐步序号步文指纹齐备才算可执行）+
//! 注入核（重复封存/缺产物/核销越界/步骤缺指纹四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1ec40 独占 0x20）。
pub const SEG_BASE: u32 = 0x1ec40;
pub const CC_RESEAL: u32 = SEG_BASE + 0x01; // 已封存再封存
pub const CC_NOART: u32 = SEG_BASE + 0x02; // 产物未落袋
pub const CC_OVER: u32 = SEG_BASE + 0x03; // 核销越界
pub const CC_STEP: u32 = SEG_BASE + 0x04; // 步骤缺指纹
pub const CC_DUPID: u32 = SEG_BASE + 0x05; // 演习编号重号
pub const CC_UNSEALED: u32 = SEG_BASE + 0x06; // 未封存先核销
pub const CC_OPEN: u32 = SEG_BASE + 0x07; // 核销册未闭合

/// 演习面定容：封存件上限。
pub const SEAL_CAP: usize = 16;
/// 演习锚（fold 种子）。
pub const SEAL_TAG: u64 = 0x1ec40d2;
/// 移交验收件数（判据 10/10）。
pub const HANDOVER_N: usize = 10;
/// 复跑指引标准步数（缺步即不可核销）。
pub const GUIDE_STEPS: usize = 10;
/// 指纹步进常量。
pub const FP_STEP: u64 = 0x100000001b3;

/// 单件封存件：演习产物袋 + 封存位。
#[derive(Clone, Debug, PartialEq)]
pub struct SealItem {
    pub id: u32,
    pub drill: String,
    pub rounds: u32,
    pub artifacts: Vec<u64>,
    pub sealed: bool,
    pub written_off: bool,
    pub guide: Option<RerunGuide>,
}

impl SealItem {
    pub fn sealed_ok(&self) -> bool {
        self.sealed && !self.artifacts.is_empty()
    }

    /// 核销判：已封存 + 指引可执行且步数齐（GUIDE_STEPS 步）；
    /// 重复核销另由 write_off 的 RESEAL 位单独拒止。
    pub fn write_off_ready(&self) -> bool {
        self.sealed_ok()
            && self
                .guide
                .as_ref()
                .map(|g| g.executable() && g.steps.len() >= GUIDE_STEPS)
                .unwrap_or(false)
    }

    /// 封存指纹：产物袋链式 fold。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SEAL_TAG ^ (self.id as u64).wrapping_shl(8);
        h = h.wrapping_add(self.rounds as u64);
        let mut k = 0usize;
        while k < self.artifacts.len() {
            h = (h ^ self.artifacts[k]).wrapping_mul(FP_STEP);
            k += 1;
        }
        h = (h ^ self.sealed as u64).wrapping_mul(FP_STEP);
        h
    }

    pub fn render(&self) -> String {
        let state = if self.written_off {
            "W"
        } else if self.sealed {
            "S"
        } else {
            "O"
        };
        format!("Z{}|{}|r{}", self.id, state, self.rounds)
    }
}

/// 复跑指引：步骤向量（步号步文步指纹），齐备才算可执行。
#[derive(Clone, Debug, PartialEq)]
pub struct RerunGuide {
    pub target: u32,
    pub steps: Vec<(u32, String, u64)>,
}

impl RerunGuide {
    pub fn new(target: u32) -> RerunGuide {
        RerunGuide { target, steps: Vec::new() }
    }

    /// 步骤登记：步号连续可跳步文不可空。
    pub fn add_step(&mut self, no: u32, text: &str, fp: u64) -> Result<(), u32> {
        if text.is_empty() {
            return Err(CC_STEP);
        }
        if fp == 0 {
            return Err(CC_STEP);
        }
        if self.steps.iter().any(|s| s.0 == no) {
            return Err(CC_DUPID);
        }
        self.steps.push((no, text.to_string(), fp));
        self.steps.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(())
    }

    /// 可执行判：至少一步、步号从 1 连续递增、每步指纹非零且可复算。
    pub fn executable(&self) -> bool {
        if self.steps.is_empty() {
            return false;
        }
        let mut no = 1u32;
        let mut k = 0usize;
        while k < self.steps.len() {
            if self.steps[k].0 != no {
                return false;
            }
            let (t, fp) = (&self.steps[k].1, self.steps[k].2);
            if t.is_empty() || fp == 0 || step_fp(self.target, no, t) != fp {
                return false;
            }
            no += 1;
            k += 1;
        }
        true
    }

    /// 指引指纹：全步骤链式 fold。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SEAL_TAG ^ (self.target as u64).wrapping_shl(4);
        let mut k = 0usize;
        while k < self.steps.len() {
            h = (h ^ self.steps[k].0 as u64).wrapping_mul(FP_STEP);
            let bytes = self.steps[k].1.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            h = (h ^ self.steps[k].2).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }
}

/// 步骤指纹复算：目标、步号、步文定值。
pub fn step_fp(target: u32, no: u32, text: &str) -> u64 {
    let mut h: u64 = SEAL_TAG ^ (target as u64).wrapping_shl(16) ^ no as u64;
    let bytes = text.as_bytes();
    let mut b = 0usize;
    while b < bytes.len() {
        h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
        b += 1;
    }
    h
}

/// 演习封存册：登记→产物落袋→封存→核销出册。
#[derive(Debug)]
pub struct SealBook {
    pub items: Vec<SealItem>,
}

impl SealBook {
    pub fn new() -> SealBook {
        SealBook { items: Vec::new() }
    }

    /// 演习登记：重号、超容、空名拒。
    pub fn enroll(&mut self, id: u32, drill: &str, rounds: u32) -> Result<(), u32> {
        if drill.is_empty() {
            return Err(CC_STEP);
        }
        if self.items.iter().any(|i| i.id == id) {
            return Err(CC_DUPID);
        }
        if self.items.len() >= SEAL_CAP {
            return Err(CC_OVER);
        }
        self.items.push(SealItem {
            id,
            drill: drill.to_string(),
            rounds,
            artifacts: Vec::new(),
            sealed: false,
            written_off: false,
            guide: None,
        });
        Ok(())
    }

    /// 产物落袋：封存后拒，袋空拒。
    pub fn deposit(&mut self, id: u32, artifacts: Vec<u64>) -> Result<(), u32> {
        let it = self.find(id).ok_or(CC_OVER)?;
        if it.sealed {
            return Err(CC_RESEAL);
        }
        if artifacts.is_empty() {
            return Err(CC_NOART);
        }
        it.artifacts = artifacts;
        Ok(())
    }

    /// 封存：翻位落袋校验（产物未落袋拒）。
    pub fn seal(&mut self, id: u32) -> Result<u64, u32> {
        let it = self.find(id).ok_or(CC_OVER)?;
        if it.sealed {
            return Err(CC_RESEAL);
        }
        if it.artifacts.is_empty() {
            return Err(CC_NOART);
        }
        it.sealed = true;
        Ok(it.fold())
    }

    /// 挂指引：未封存拒，已核销拒。
    pub fn attach(&mut self, id: u32, guide: RerunGuide) -> Result<(), u32> {
        let it = self.find(id).ok_or(CC_OVER)?;
        if !it.sealed {
            return Err(CC_UNSEALED);
        }
        if it.written_off {
            return Err(CC_OVER);
        }
        it.guide = Some(guide);
        Ok(())
    }

    /// 核销：逐件出册（翻 written_off 位）。
    pub fn write_off(&mut self, id: u32) -> Result<(), u32> {
        let it = self.find(id).ok_or(CC_OVER)?;
        if !it.sealed {
            return Err(CC_UNSEALED);
        }
        if it.written_off {
            return Err(CC_RESEAL);
        }
        if !it.write_off_ready() {
            return Err(CC_OPEN);
        }
        it.written_off = true;
        Ok(())
    }

    /// 移交裁断：全部核销且指引全可执行。
    pub fn handed(&self) -> bool {
        !self.items.is_empty()
            && self.items.iter().all(|i| i.written_off && i.write_off_ready())
    }

    /// 待核销清单（按登记序）。
    pub fn pending(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.items[k].written_off {
                g.push(self.items[k].id);
            }
            k += 1;
        }
        g
    }

    fn find(&mut self, id: u32) -> Option<&mut SealItem> {
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].id == id {
                return Some(&mut self.items[k]);
            }
            k += 1;
        }
        None
    }

    /// 册级指纹：逐件封存指纹链式 fold。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SEAL_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            h = (h ^ self.items[k].fold()).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }
}

/// 标准步骤文（确定性样本，正反两路共用）。
pub fn step_text(no: u32) -> String {
    format!("复跑第 {} 步：重放对应演习轮次", no)
}

/// 标准产物袋：轮次定值样本。
pub fn sample_artifacts(rounds: u32, n: usize) -> Vec<u64> {
    let mut v: Vec<u64> = Vec::new();
    let mut k = 0usize;
    while k < n {
        v.push(rounds as u64 * FP_STEP ^ k as u64);
        k += 1;
    }
    v
}

/// 标准指引构造：HANDOVER_N 步全齐。
pub fn sample_guide(target: u32) -> RerunGuide {
    let mut g = RerunGuide::new(target);
    let mut no = 1u32;
    while no <= GUIDE_STEPS as u32 {
        let t = step_text(no);
        let fp = step_fp(target, no, &t);
        let _ = g.add_step(no, &t, fp);
        no += 1;
    }
    g
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 重复封存必拒
    let mut r = SealBook::new();
    let _ = r.enroll(1, "重封样本", 100);
    let _ = r.deposit(1, sample_artifacts(100, 3));
    let s1 = r.seal(1);
    got[0] = s1.is_ok() && r.seal(1).err() == Some(CC_RESEAL);
    // 2) 缺产物封存必拒
    let mut n = SealBook::new();
    let _ = n.enroll(2, "缺产物样本", 50);
    got[1] = n.seal(2).err() == Some(CC_NOART);
    // 3) 未挂指引核销必拒（核销越界/闭合失败）
    let mut w = SealBook::new();
    let _ = w.enroll(3, "无指引样本", 60);
    let _ = w.deposit(3, sample_artifacts(60, 2));
    let _ = w.seal(3);
    got[2] = w.write_off(3).err() == Some(CC_OPEN);
    // 4) 指引缺指纹必不可执行
    let mut g = sample_guide(4);
    g.steps[2].2 = 0;
    got[3] = !g.executable();
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11098_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11098");

    // J1 演习面封存逐件核销（10/10）+ 复跑指引可执行（10/10）
    let mut book = SealBook::new();
    let mut id = 1u32;
    let mut done = 0usize;
    while id <= HANDOVER_N as u32 {
        let rounds = id * 10;
        let ok_en = book.enroll(id, "长稳演习", rounds).is_ok();
        let ok_de = book.deposit(id, sample_artifacts(rounds, 3)).is_ok();
        let ok_se = book.seal(id).is_ok();
        let ok_at = book.attach(id, sample_guide(id)).is_ok();
        let ok_wo = book.write_off(id).is_ok();
        if !(ok_en && ok_de && ok_se && ok_at && ok_wo) {
            break;
        }
        done += 1;
        id += 1;
    }
    s.add(
        "J1-封存核销十件",
        done == HANDOVER_N && book.handed(),
        "10 件封存逐件挂指引并核销出册",
    );

    // J2 指引可执行：十件指引全 executable 且步号 1..10 连续
    let j2 = book.items.len() == HANDOVER_N
        && book
            .items
            .iter()
            .all(|i| i.guide.as_ref().map(|g| g.executable()).unwrap_or(false));
    s.add("J2-指引可执行", j2, "复跑指引步号连续且指纹可复算");

    // J3 注入检出：重封 + 缺产物
    let inj = inject_four();
    s.add("J3-重封缺产物检出", inj[0] && inj[1], "重复封存与缺产物两路必抓");

    // J4 注入检出：无指引核销 + 缺指纹
    s.add("J4-无指引缺指纹检出", inj[2] && inj[3], "无指引核销必拒且缺指纹必不可执行");

    // J5 未封存先核销必拒
    let j5 = {
        let mut c = SealBook::new();
        let _ = c.enroll(9, "未封样本", 5);
        let _ = c.deposit(9, sample_artifacts(5, 1));
        c.write_off(9).err() == Some(CC_UNSEALED)
    };
    s.add("J5-未封先销拒", j5, "未封存核销必拒");

    // J6 指引步号断裂必不可执行
    let j6 = {
        let mut g = sample_guide(6);
        g.steps[4].0 = 99;
        !g.executable()
    };
    s.add("J6-步号断裂拒", j6, "步号不连续必拒执行");

    // J7 重号登记拒
    let j7 = {
        let mut c = SealBook::new();
        let _ = c.enroll(1, "甲", 10);
        c.enroll(1, "乙", 20).err() == Some(CC_DUPID)
    };
    s.add("J7-重号拒收", j7, "演习编号重号必拒");

    // J8 封存后落袋必拒
    let j8 = {
        let mut c = SealBook::new();
        let _ = c.enroll(2, "封后落袋", 8);
        let _ = c.deposit(2, sample_artifacts(8, 2));
        let _ = c.seal(2);
        c.deposit(2, sample_artifacts(8, 1)).err() == Some(CC_RESEAL)
    };
    s.add("J8-封后落袋拒", j8, "封存后产物袋必锁");

    // J9 容量边界：第 SEAL_CAP+1 条登记被拒
    let j9 = {
        let mut c = SealBook::new();
        let mut n = 1u32;
        while n <= SEAL_CAP as u32 {
            let _ = c.enroll(n, "x", 1);
            n += 1;
        }
        c.enroll(999, "over", 1).err() == Some(CC_OVER)
    };
    s.add("J9-容量边界", j9, "超出定容必拒");

    // J10 渲染面：封存件状态旗可区分
    let j10 = {
        let open = SealItem {
            id: 1,
            drill: "渲染".to_string(),
            rounds: 5,
            artifacts: Vec::new(),
            sealed: false,
            written_off: false,
            guide: None,
        };
        let mut sealed = open.clone();
        sealed.sealed = true;
        let mut off = sealed.clone();
        off.written_off = true;
        open.render() != sealed.render() && sealed.render() != off.render()
    };
    s.add("J10-渲染分旗", j10, "开/封/销三态渲染可区分");

    // J11 件指纹稳定：同件两次 fold 一致且非零
    let j11 = {
        let mut c = SealBook::new();
        let _ = c.enroll(3, "指纹样本", 20);
        let _ = c.deposit(3, sample_artifacts(20, 4));
        let _ = c.seal(3);
        let f1 = c.items[0].fold();
        f1 != 0 && f1 == c.items[0].fold()
    };
    s.add("J11-件指纹稳定", j11, "封存指纹可重算零漂移");

    // J12 缺步指引挂载后核销必拒
    let j12 = {
        let mut c = SealBook::new();
        let _ = c.enroll(4, "缺步样本", 30);
        let _ = c.deposit(4, sample_artifacts(30, 2));
        let _ = c.seal(4);
        let mut g = sample_guide(4);
        g.steps.pop();
        let _ = c.attach(4, g);
        c.write_off(4).err() == Some(CC_OPEN)
    };
    s.add("J12-缺步核销拒", j12, "指引不全核销必拒");

    // J13 动态明细：移交册指纹与待核销数
    let fp = book.fold();
    let pend = book.pending().len();
    s.add_dyn("J13-移交册明细", pend == 0 && fp != 0, &format!("pend={} fp={:#x}", pend, fp));

    // J14 动态明细：首件指引步数
    let sn = book.items.first().map(|i| i.guide.as_ref().map(|g| g.steps.len()).unwrap_or(0)).unwrap_or(0);
    s.add_dyn("J14-指引步数", sn == HANDOVER_N, &format!("steps={}", sn));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11098_seal_flow() {
        let mut l = SealBook::new();
        assert!(l.enroll(1, "a", 10).is_ok());
        assert!(l.seal(1).err() == Some(CC_NOART));
        assert!(l.deposit(1, sample_artifacts(10, 2)).is_ok());
        assert!(l.seal(1).is_ok());
        assert!(l.write_off(1).err() == Some(CC_OPEN));
        assert!(l.attach(1, sample_guide(1)).is_ok());
        assert!(l.write_off(1).is_ok());
        assert!(l.handed());
        assert!(l.enroll(1, "dup", 1).err() == Some(CC_DUPID));
    }

    #[test]
    fn unx11098_checks_green() {
        let s = run_unx11098_checks();
        assert!(s.all_passed(), "UNX-F11098 判据未全绿");
    }
}
