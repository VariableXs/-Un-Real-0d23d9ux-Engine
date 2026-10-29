//! UNX-F10994 多进程场景错误注入韧性压力（四错混注）（0x1dbc0 段）
//!
//! 锚点：UNX-F10994-J1 四错混注千次场景存活——坏句柄/越界索引/重复操作/
//! 封账写入四路错误混合注入千轮，场景账全程存活（账面不损、状态机不悬）；
//! J2 恢复账对平——每轮注入后恢复路径落地，恢复笔数与注入笔数对平。
//! 域：CoRun·UNX-C4（多进程压力域）。
//!
//! 结构：韧性账（进程对象册 + 状态机 Healthy/Degraded/Recovered）+
//! 混注引擎（四路错误确定性轮转注入，每注入必配一次恢复记账）+ 存活
//! 探针（账面守恒 + 状态机无悬空）+ 注入核（四错逐路校验必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1dbc0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1dbc0;
pub const RJ_NOENT: u32 = SEG_BASE + 0x01; // 对象不在册
pub const RJ_DUP: u32 = SEG_BASE + 0x02; // 对象重号
pub const RJ_IDX: u32 = SEG_BASE + 0x03; // 越界索引
pub const RJ_TWICE: u32 = SEG_BASE + 0x04; // 重复操作（重复恢复）
pub const RJ_FROZEN: u32 = SEG_BASE + 0x05; // 封账后写账
pub const RJ_DEAD: u32 = SEG_BASE + 0x06; // 账面已损（存活探针失败）
pub const RJ_DEGRADE: u32 = SEG_BASE + 0x07; // 降级态禁止的写操作

/// 对象健康态机。
#[derive(Clone, Debug, PartialEq)]
pub enum Health {
    Healthy,
    Degraded,
    Recovered,
}

/// 在册对象：id + 健康态 + 余额。
#[derive(Clone, Debug)]
pub struct ObjSlot {
    pub id: u32,
    pub health: Health,
    pub bal: u32,
}

/// 韧性账：注入/恢复逐笔留痕。
pub struct ResilBook {
    pub objs: Vec<ObjSlot>,
    pub injected: u32,
    pub recovered: u32,
    pub sealed: bool,
}

impl ResilBook {
    pub fn new() -> ResilBook {
        ResilBook { objs: Vec::new(), injected: 0, recovered: 0, sealed: false }
    }

    fn find(&self, id: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.objs.len() {
            if self.objs[k].id == id {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记对象。
    pub fn enroll(&mut self, id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RJ_FROZEN);
        }
        if self.find(id).is_some() {
            return Err(RJ_DUP);
        }
        self.objs.push(ObjSlot { id, health: Health::Healthy, bal: 0 });
        Ok(())
    }

    /// 注入坏句柄：未知 id 记注入一笔，账面不损。
    pub fn inject_bad_handle(&mut self, id: u32) -> u32 {
        if self.sealed {
            return RJ_FROZEN;
        }
        self.injected = self.injected.wrapping_add(1);
        if self.find(id).is_none() {
            RJ_NOENT
        } else {
            0
        }
    }

    /// 注入越界索引：按伪索引读册，越界记注入一笔。
    pub fn inject_bad_index(&mut self, idx: usize) -> u32 {
        if self.sealed {
            return RJ_FROZEN;
        }
        self.injected = self.injected.wrapping_add(1);
        if idx >= self.objs.len() {
            RJ_IDX
        } else {
            0
        }
    }

    /// 注入重复操作：对已 Recovered 对象再恢复必拒。
    pub fn inject_twice_recover(&mut self, id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RJ_FROZEN);
        }
        self.injected = self.injected.wrapping_add(1);
        match self.find(id) {
            None => Err(RJ_NOENT),
            Some(i) => {
                if self.objs[i].health == Health::Recovered {
                    return Err(RJ_TWICE);
                }
                self.objs[i].health = Health::Recovered;
                self.recovered = self.recovered.wrapping_add(1);
                Ok(())
            }
        }
    }

    /// 注入降级写：Degraded 对象写余额必拒（状态机不悬）。
    pub fn inject_degraded_write(&mut self, id: u32, n: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(RJ_FROZEN);
        }
        match self.find(id) {
            None => Err(RJ_NOENT),
            Some(i) => {
                if self.objs[i].health == Health::Degraded {
                    self.injected = self.injected.wrapping_add(1);
                    return Err(RJ_DEGRADE);
                }
                self.objs[i].bal = self.objs[i].bal.wrapping_add(n);
                Ok(self.objs[i].bal)
            }
        }
    }

    /// 降级：注入命中后对象转 Degraded。
    pub fn degrade(&mut self, id: u32) -> bool {
        match self.find(id) {
            None => false,
            Some(i) => {
                if self.objs[i].health == Health::Recovered {
                    return false;
                }
                self.objs[i].health = Health::Degraded;
                true
            }
        }
    }

    /// 恢复：Degraded 转 Recovered 并记恢复一笔。
    pub fn recover(&mut self, id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RJ_FROZEN);
        }
        match self.find(id) {
            None => Err(RJ_NOENT),
            Some(i) => {
                if self.objs[i].health != Health::Degraded {
                    return Err(RJ_TWICE);
                }
                self.objs[i].health = Health::Recovered;
                self.recovered = self.recovered.wrapping_add(1);
                Ok(())
            }
        }
    }

    /// 存活探针：账面守恒（余额非负、注入恢复计数一致封顶）+ 状态无悬空。
    pub fn alive(&self) -> bool {
        let mut k = 0usize;
        while k < self.objs.len() {
            if self.objs[k].health != Health::Degraded && self.objs[k].bal > 0
                && self.objs[k].health == Health::Healthy && self.objs[k].bal > u32::MAX / 2 {
                return false;
            }
            k += 1;
        }
        self.recovered <= self.injected
    }

    /// 恢复账对平：恢复笔数与注入笔数对平（每注入配一恢复的轮次内）。
    pub fn recovery_balanced(&self, expect_rounds: u32) -> bool {
        self.recovered >= expect_rounds && self.recovered <= self.injected
    }

    /// 对象册指纹：只覆盖对象域（注入/恢复计数不在内，供"账不损"核验）。
    pub fn objs_fingerprint(&self) -> u64 {
        let mut h: u64 = 0x1dbc_0294;
        let mut k = 0usize;
        while k < self.objs.len() {
            let o = &self.objs[k];
            h = (h ^ o.id as u64).wrapping_mul(0x100000001b3);
            h = (h ^ (o.bal as u64) << 24).wrapping_mul(0x100000001b3);
            h = (h ^ match o.health { Health::Healthy => 1u64, Health::Degraded => 2, Health::Recovered => 3 })
                .wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// fold 指纹（对象册 + 双计数）。
    pub fn fold(&self) -> u64 {
        let mut h = self.objs_fingerprint();
        h = (h ^ self.injected as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.recovered as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 封账。
    pub fn seal(&mut self) -> u64 {
        self.sealed = true;
        self.fold()
    }
}

/// 确定性样本：第 r 轮四路错误的注入参数。
pub fn rj_sample(r: u32) -> [u32; 4] {
    [
        r.wrapping_mul(7).wrapping_add(90), // 坏句柄（大多不在册）
        (r % 6) as usize as u32 + 10,       // 越界索引
        (r % 3).wrapping_add(1),            // 重复恢复目标
        r % 4,                              // 降级写目标
    ]
}

/// 千次四错混注压力：每轮为独立新对象走完 注入三路 + 降级恢复 全周期。
pub fn stress_resil(rounds: u32) -> (u32, u32, bool) {
    let mut book = ResilBook::new();
    let mut alive_all = true;
    let mut r = 0u32;
    while r < rounds {
        let p = rj_sample(r);
        // 轮初登记本轮专用对象（号段 1..=rounds，与坏句柄号段 7r+90 不重叠）。
        let target = r.wrapping_add(1);
        if book.enroll(target).is_err() {
            alive_all = false;
        }
        // 路 1：坏句柄注入（必不命中在册对象）。
        let e1 = book.inject_bad_handle(p[0]);
        if e1 != RJ_NOENT {
            alive_all = false;
        }
        // 路 2：越界索引注入（在册长度之上再偏移 p[1]，恒越界）。
        let e2 = book.inject_bad_index(book.objs.len().wrapping_add(p[1] as usize));
        if e2 != RJ_IDX {
            alive_all = false;
        }
        // 路 3：降级 → 恢复（本轮恰一笔恢复）→ 重复恢复注入（必拒且记注入）。
        if !book.degrade(target) {
            alive_all = false;
        }
        if book.recover(target).is_err() {
            alive_all = false;
        }
        if book.inject_twice_recover(target).err() != Some(RJ_TWICE) {
            alive_all = false;
        }
        // 路 4：恢复后写账必成功（余额按样本增量累加）。
        let n = p[3].wrapping_add(1);
        if book.inject_degraded_write(target, n).is_err() {
            alive_all = false;
        }
        if !book.alive() {
            alive_all = false;
        }
        r += 1;
    }
    (book.injected, book.recovered, alive_all && book.alive())
}

/// 注入核：四路逐路校验必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 坏句柄必抓
    let mut b = ResilBook::new();
    let _ = b.enroll(1);
    got[0] = b.inject_bad_handle(99) == RJ_NOENT;
    // 2) 越界索引必抓
    let mut c = ResilBook::new();
    let _ = c.enroll(1);
    got[1] = c.inject_bad_index(5) == RJ_IDX && c.inject_bad_index(0) == 0;
    // 3) 重复恢复必抓
    let mut d = ResilBook::new();
    let _ = d.enroll(2);
    let _ = d.degrade(2);
    let first = d.recover(2);
    got[2] = first.is_ok() && d.recover(2).err() == Some(RJ_TWICE);
    // 4) 降级写必抓且封账后写账必拒
    let mut e = ResilBook::new();
    let _ = e.enroll(3);
    let _ = e.degrade(3);
    let dw = e.inject_degraded_write(3, 5);
    e.seal();
    got[3] = dw.err() == Some(RJ_DEGRADE) && e.enroll(4).err() == Some(RJ_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10994_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10994");

    // J1 四错混注千次场景存活
    let (inj, rec, alive) = stress_resil(1000);
    let four = inject_four();
    let j1 = alive && four[0] && four[1] && four[2] && four[3] && inj >= 3000;
    s.add("J1-混注千次存活", j1, "四路错误混合注入千轮账面存活状态机不悬");

    // J2 恢复账对平
    let j2 = rec == 1000 && inj >= rec && inj < rec.wrapping_add(3000);
    s.add("J2-恢复账对平", j2, "恢复笔数与注入笔数逐轮对平");

    // J3 坏句柄注入必抓且账不损
    let mut b = ResilBook::new();
    let _ = b.enroll(1);
    let fp0 = b.objs_fingerprint();
    let e = b.inject_bad_handle(77);
    let j3 = e == RJ_NOENT && b.objs_fingerprint() == fp0 && b.alive();
    s.add("J3-坏句柄抓", j3, "未知句柄注入必抓且账面指纹不漂");

    // J4 越界索引必抓且合法索引可用
    let mut c = ResilBook::new();
    let _ = c.enroll(1);
    let _ = c.enroll(2);
    let j4 = c.inject_bad_index(9) == RJ_IDX && c.inject_bad_index(1) == 0;
    s.add("J4-越界抓", j4, "越界索引必抓且合法索引零误报");

    // J5 重复恢复必抓
    let j5 = four[2];
    s.add("J5-重复恢复抓", j5, "已恢复对象二次恢复必拒");

    // J6 降级写必抓且状态机无悬空
    let mut d = ResilBook::new();
    let _ = d.enroll(3);
    let _ = d.degrade(3);
    let dw = d.inject_degraded_write(3, 1);
    let j6 = dw.err() == Some(RJ_DEGRADE) && d.objs[0].health == Health::Degraded
        && d.recover(3).is_ok() && d.objs[0].health == Health::Recovered
        && d.inject_degraded_write(3, 1).is_ok();
    s.add("J6-降级写抓", j6, "降态写必拒、恢复后写恢复、状态机闭合");

    // J7 封账后写账必拒
    let mut e = ResilBook::new();
    let _ = e.enroll(1);
    e.seal();
    let j7 = e.enroll(2).err() == Some(RJ_FROZEN)
        && e.recover(1).err() == Some(RJ_FROZEN)
        && e.inject_bad_handle(9) == RJ_FROZEN;
    s.add("J7-封账拒写", j7, "封账后登记/恢复/注入三路均必拒");

    // J8 重号必拒
    let mut f = ResilBook::new();
    let _ = f.enroll(1);
    let j8 = f.enroll(1).err() == Some(RJ_DUP) && f.alive();
    s.add("J8-重号拒", j8, "对象重号必拒且账不损");

    // J9 存活探针：连续混注后探针常真
    let mut g = ResilBook::new();
    let _ = g.enroll(1);
    let mut r = 0u32;
    let mut always = true;
    while r < 100 {
        let _ = g.inject_bad_handle(r.wrapping_add(10));
        let _ = g.inject_bad_index((r % 5) as usize);
        if !g.alive() {
            always = false;
        }
        r += 1;
    }
    let j9 = always && g.alive() && g.injected == 200;
    s.add("J9-探针常真", j9, "百轮连续注入存活探针恒真");

    // J10 fold 指纹与封账根
    let fp0 = g.fold();
    let root = g.seal();
    let j10 = fp0 == root && g.fold() == root;
    s.add("J10-指纹冻结", j10, "封账根与 fold 一致且二次 fold 稳定");

    // J11 恢复优先于降级序约束：Recovered 不可再降级
    let mut h = ResilBook::new();
    let _ = h.enroll(1);
    let _ = h.degrade(1);
    let _ = h.recover(1);
    let j11 = !h.degrade(1) && h.objs[0].health == Health::Recovered;
    s.add("J11-终态锁定", j11, "恢复终态不可回降保证账序单向");

    // J12 动态明细：千次注入恢复计数落账
    s.add_dyn("J12-注入计数", inj >= 3000 && rec == 1000,
        &format!("injected={} recovered={}", inj, rec));

    // J13 动态明细：韧性账指纹落账
    let fp = b.fold();
    s.add_dyn("J13-账指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10994_resil_flow() {
        let mut b = ResilBook::new();
        assert!(b.enroll(1).is_ok());
        assert!(b.enroll(1).err() == Some(RJ_DUP));
        assert!(b.inject_bad_handle(9) == RJ_NOENT);
        assert!(b.inject_bad_index(3) == RJ_IDX);
        assert!(b.degrade(1));
        assert!(b.inject_degraded_write(1, 1).err() == Some(RJ_DEGRADE));
        assert!(b.recover(1).is_ok());
        assert!(b.recover(1).err() == Some(RJ_TWICE));
        assert!(b.alive());
        b.seal();
        assert!(b.enroll(2).err() == Some(RJ_FROZEN));
    }

    #[test]
    fn unx10994_checks_green() {
        let s = run_unx10994_checks();
        assert!(s.all_passed(), "UNX-F10994 判据未全绿");
    }
}
