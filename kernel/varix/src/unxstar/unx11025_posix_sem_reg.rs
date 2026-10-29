//! UNX-F11025 POSIX sem 挂号联签（sem_wait/post 挂点账对账）（0x1e320 段）
//!
//! 锚点：UNX-F11025-J1 wait/post 挂点万次对平（10/10），计数账零偏离（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·POSIX sem）。
//!
//! 结构：具名信号量表（名 + 值 + wait/post 计数 + 拒止计数）+ 挂点器
//! （wait 值零即拒计入拒止账、post 超界拒）+ 万次对账器（确定性轮转模
//! 式）+ 零偏离断言（发 = 收 + 存量）+ fold 指纹与冻结 + 注入核（超界
//! post/无名量/重复名/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e320 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e320;
pub const PS_OVERFLOW: u32 = SEG_BASE + 0x01; // post 超界
pub const PS_NONAME: u32 = SEG_BASE + 0x02; // 无名量
pub const PS_DUPNAME: u32 = SEG_BASE + 0x03; // 重名注册
pub const PS_EMPTYNAME: u32 = SEG_BASE + 0x04; // 空名
pub const PS_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const PS_CAP: u32 = SEG_BASE + 0x06; // 量表演超容

/// 量表演定容。
pub const SEM_TBL_CAP: usize = 12;
/// 万次挂点轮数。
pub const PS_ROUNDS: u32 = 10_000;
/// POSIX sem 账域标（fold 种子）。
pub const PSEM_TAG: u64 = 0x1e32_25;
/// 单量值上限。
pub const PS_VAL_CAP: u32 = 6;
/// 量表演规模（10/10）。
pub const SEMS_NEEDED: usize = 10;

/// 单个具名信号量：名 + 初值 + 值 + wait/post/拒止计数。
#[derive(Clone, Debug)]
pub struct NamedSem {
    pub name: String,
    pub init: u32,
    pub val: u32,
    pub waits: u32,
    pub posts: u32,
    pub denied: u32,
}

impl NamedSem {
    /// 零偏离：发 + 初值 = 收 + 存量。
    pub fn zero_drift(&self) -> bool {
        self.posts.wrapping_add(self.init) == self.waits.wrapping_add(self.val)
    }

    pub fn render(&self) -> String {
        format!("psem|{}|v{}|w{}|p{}", self.name, self.val, self.waits, self.posts)
    }
}

/// POSIX sem 挂号账：注册具名量 → wait/post 挂点 → 零偏离封账。
pub struct PsBook {
    pub sems: Vec<NamedSem>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl PsBook {
    pub fn new() -> PsBook {
        PsBook { sems: Vec::new(), frozen: false, root: None }
    }

    /// 注册具名量：冻结、空名、重名、超容必拒。
    pub fn enroll(&mut self, name: &str, init: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(PS_FROZEN);
        }
        if name.is_empty() {
            return Err(PS_EMPTYNAME);
        }
        if init > PS_VAL_CAP {
            return Err(PS_OVERFLOW);
        }
        if self.sems.iter().any(|x| x.name == name) {
            return Err(PS_DUPNAME);
        }
        if self.sems.len() >= SEM_TBL_CAP {
            return Err(PS_CAP);
        }
        self.sems.push(NamedSem {
            name: name.to_string(),
            init,
            val: init,
            waits: 0,
            posts: 0,
            denied: 0,
        });
        Ok(())
    }

    /// wait 挂点：值零即拒（计入拒止账），否则取量。
    pub fn wait(&mut self, name: &str) -> Result<bool, u32> {
        if self.frozen {
            return Err(PS_FROZEN);
        }
        let mut k = 0usize;
        while k < self.sems.len() {
            if self.sems[k].name == name {
                if self.sems[k].val == 0 {
                    self.sems[k].denied = self.sems[k].denied.wrapping_add(1);
                    return Ok(false);
                }
                self.sems[k].val -= 1;
                self.sems[k].waits = self.sems[k].waits.wrapping_add(1);
                return Ok(true);
            }
            k += 1;
        }
        Err(PS_NONAME)
    }

    /// post 挂点：超界必拒，否则还量。
    pub fn post(&mut self, name: &str) -> Result<u32, u32> {
        if self.frozen {
            return Err(PS_FROZEN);
        }
        let mut k = 0usize;
        while k < self.sems.len() {
            if self.sems[k].name == name {
                if self.sems[k].val >= PS_VAL_CAP {
                    return Err(PS_OVERFLOW);
                }
                self.sems[k].val += 1;
                self.sems[k].posts = self.sems[k].posts.wrapping_add(1);
                return Ok(self.sems[k].val);
            }
            k += 1;
        }
        Err(PS_NONAME)
    }

    /// 全表零偏离。
    pub fn zero_drift_all(&self) -> bool {
        let mut ok = true;
        let mut k = 0usize;
        while k < self.sems.len() {
            if !self.sems[k].zero_drift() {
                ok = false;
            }
            k += 1;
        }
        ok
    }

    /// wait 挂点总量（10/10 对平面）。
    pub fn waits_total(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.sems.len() {
            n = n.wrapping_add(self.sems[k].waits);
            k += 1;
        }
        n
    }

    /// post 挂点总量。
    pub fn posts_total(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.sems.len() {
            n = n.wrapping_add(self.sems[k].posts);
            k += 1;
        }
        n
    }

    /// 按名查位次。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut k = 0usize;
        while k < self.sems.len() {
            if self.sems[k].name == name {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = PSEM_TAG;
        let mut k = 0usize;
        while k < self.sems.len() {
            let x = &self.sems[k];
            h = (h ^ x.val as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.waits as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.posts as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.denied as u64).wrapping_mul(0x100000001b3);
            let bytes = x.name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(0x100000001b3);
                b += 1;
            }
            k += 1;
        }
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 名确定性样本。
pub fn ps_name(no: usize) -> String {
    format!("/psem{}", no)
}

/// 初值确定性样本。
pub fn ps_init(no: usize) -> u32 {
    (no % 3 + 1) as u32
}

/// 万次对账全链：十量注册，万轮在路 i%10 上 post+wait 成对收发。
pub fn build_psbook() -> (PsBook, bool) {
    let mut b = PsBook::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < SEMS_NEEDED {
        if b.enroll(&ps_name(i), ps_init(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut r = 0u32;
    while r < PS_ROUNDS {
        let k = (r % SEMS_NEEDED as u32) as usize;
        if b.post(&ps_name(k)).is_err() {
            ok = false;
        }
        if b.wait(&ps_name(k)).is_err() {
            ok = false;
        }
        r += 1;
    }
    let b_ok = ok && b.zero_drift_all();
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 满量 post 超界必拒
    let mut a = PsBook::new();
    let _ = a.enroll("/a", 0);
    let mut n = 0u32;
    let mut filled = true;
    while n < PS_VAL_CAP {
        if a.post("/a").is_err() {
            filled = false;
        }
        n += 1;
    }
    got[0] = filled && a.post("/a").err() == Some(PS_OVERFLOW);
    // 2) 无名量挂点必拒
    let mut b = PsBook::new();
    got[1] = b.wait("/none").err() == Some(PS_NONAME)
        && b.post("/none").err() == Some(PS_NONAME);
    // 3) 空名与重名必拒
    let mut c = PsBook::new();
    let z = c.enroll("", 1).err() == Some(PS_EMPTYNAME);
    let _ = c.enroll("/d", 1);
    got[2] = z && c.enroll("/d", 1).err() == Some(PS_DUPNAME);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_psbook();
    d.freeze();
    got[3] = d.post(&ps_name(0)).err() == Some(PS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11025_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11025");

    // J1 wait/post 挂点万次对平：万轮成对收发
    let (mut book, green) = build_psbook();
    s.add("J1-万次对平", green && book.waits_total() == PS_ROUNDS && book.posts_total() == PS_ROUNDS, "十量万轮成对收发对平");

    // J2 计数账零偏离：逐量发 = 收 + 存量
    s.add("J2-计数零偏离", book.zero_drift_all(), "逐量零偏离断言成立");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.post(&ps_name(0)).err() == Some(PS_FROZEN)
        && book.wait(&ps_name(0)).err() == Some(PS_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后 post 与 wait 均必拒");

    // J5 注入检出：超界 post + 无名量
    let inj = inject_four();
    s.add("J5-超界无名抓", inj[0] && inj[1], "满量 post 与无名量两路必抓");

    // J6 注入检出：空名重名 + 冻结后写
    s.add("J6-空名冻结检出", inj[2] && inj[3], "空名重名与冻结后写必抓");

    // J7 拒止账：值零 wait 计拒止不取量
    let j7 = {
        let mut c = PsBook::new();
        let _ = c.enroll("/z", 0);
        let g1 = c.wait("/z");
        let g2 = c.wait("/z");
        g1 == Ok(false) && g2 == Ok(false) && c.find("/z").map(|k| c.sems[k].denied) == Some(2)
    };
    s.add("J7-拒止记账", j7, "值零 wait 计拒止账不取量");

    // J8 初值语义：初值即可取量且取后归零
    let j8 = {
        let mut c = PsBook::new();
        let _ = c.enroll("/i", 3);
        let mut got_ok = true;
        let mut n = 0u32;
        while n < 3 {
            if c.wait("/i") != Ok(true) {
                got_ok = false;
            }
            n += 1;
        }
        got_ok && c.find("/i").map(|k| c.sems[k].val) == Some(0) && c.sems[0].zero_drift()
    };
    s.add("J8-初值语义", j8, "初值即可取量且取后零偏离");

    // J9 轮转均匀：万轮后各量 wait 计数均衡
    let j9 = {
        let per = PS_ROUNDS / SEMS_NEEDED as u32;
        let mut even = true;
        let mut k = 0usize;
        while k < book.sems.len() {
            if book.sems[k].waits != per {
                even = false;
            }
            k += 1;
        }
        even && per == 1000
    };
    s.add("J9-轮转均匀", j9, "轮转模式各量计数均衡千次");

    // J10 存量上界：全程值不超上限
    let j10 = {
        let mut ok = true;
        let mut k = 0usize;
        while k < book.sems.len() {
            if book.sems[k].val > PS_VAL_CAP {
                ok = false;
            }
            k += 1;
        }
        ok
    };
    s.add("J10-存量上界", j10, "万轮后各量存量均在界内");

    // J11 超容拒：第 SEM_TBL_CAP+1 量必拒
    let j11 = {
        let mut c = PsBook::new();
        let mut filled = true;
        let mut n = 0usize;
        while n < SEM_TBL_CAP {
            let nm = format!("/cap{}", n);
            if c.enroll(&nm, 1).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll("/capx", 1).err() == Some(PS_CAP)
    };
    s.add("J11-超容边界", j11, "超出定容必拒且原账不损");

    // J12 渲染面：量渲染含名与计数
    let j12 = {
        let x = NamedSem {
            name: "/r".to_string(),
            init: 0,
            val: 2,
            waits: 5,
            posts: 7,
            denied: 0,
        };
        let r = x.render();
        r.contains("/r") && r.contains("v2") && r.contains("w5")
    };
    s.add("J12-渲染明细", j12, "量渲染含名值计数");

    // J13 样本确定性：名与初值同入同出
    let n2a = ps_name(2);
    let n2b = ps_name(2);
    let i4a = ps_init(4);
    let i4b = ps_init(4);
    s.add("J13-样本确定性", n2a == n2b && i4a == i4b, "名与初值样本同入同出");

    // J14 偏离注入：构造发收不符样本偏离断言为假
    let j14 = {
        let x = NamedSem {
            name: "/d1".to_string(),
            init: 0,
            val: 1,
            waits: 1,
            posts: 1,
            denied: 0,
        };
        !x.zero_drift() && book.zero_drift_all()
    };
    s.add("J14-偏离注入", j14, "漏收一轮偏离断言为假且存量精确");

    // J15 动态明细：万次对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-万次明细",
        fp == root && book.waits_total() == PS_ROUNDS,
        &format!("root={:#x} waits={}", fp, book.waits_total()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11025_ps_sem_flow() {
        let (mut b, built) = build_psbook();
        assert!(built);
        assert!(b.wait("/none").err() == Some(PS_NONAME));
        b.freeze();
        assert!(b.post(&ps_name(1)).err() == Some(PS_FROZEN));
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11025_checks_green() {
        let s = run_unx11025_checks();
        assert!(s.all_passed(), "UNX-F11025 判据未全绿");
    }
}
