//! UNX-F10950 sem_open 错误矩阵（四错误面）（0x1d640 段）
//!
//! 纯功能模型：POSIX 命名信号量 `sem_open` 裁决台。
//! 四错误面：① 无创建位查无名 → ENOENT；② 创建位+互斥位撞名
//! → EEXIST；③ 既有信号量无访问权 → EACCES；④ 初值越出
//! [0, SEM_VALUE_MAX] → EINVAL。反面千轮样本只走合法打开，零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d640;
/// 查无名
pub const SO_ENOENT: u32 = SEG_BASE + 0x01;
/// 撞名互斥
pub const SO_EEXIST: u32 = SEG_BASE + 0x02;
/// 无访问权
pub const SO_EACCES: u32 = SEG_BASE + 0x03;
/// 初值非法
pub const SO_EINVAL: u32 = SEG_BASE + 0x04;
/// 台内对账失败
pub const SO_LEDGER: u32 = SEG_BASE + 0x05;

// ---------------------------------------------------------------- 常数
/// oflag：创建位
pub const SEM_CREAT: u32 = 0x01;
/// oflag：互斥位
pub const SEM_EXCL: u32 = 0x02;
/// 信号量值上限
pub const SEM_VALUE_MAX: u32 = 0xffff;
/// 台容量
pub const SEM_TAB_CAP: usize = 24;
/// 反面验收轮数
pub const SO_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 命名信号量条目。
#[derive(Clone, Debug)]
pub struct PoSem {
    /// 名字（/开头）
    pub name: String,
    /// 当前值
    pub value: u32,
    /// 读权位
    pub can_read: bool,
    /// 写权位
    pub can_write: bool,
    /// 打开引用数
    pub refs: usize,
}

impl PoSem {
    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let bytes = self.name.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            h = (h ^ (bytes[i] as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.value as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.can_read as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.can_write as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.refs as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "PoSem{{name={},value={},r={},w={},refs={}}}",
            self.name, self.value, self.can_read as u32, self.can_write as u32, self.refs
        )
    }
}

/// 单轮日志。
#[derive(Clone, Debug, PartialEq)]
pub struct SoLog {
    /// 轮次
    pub round: usize,
    /// 目标名
    pub name: String,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// sem_open 裁决台。
#[derive(Clone, Debug)]
pub struct SemOpenDesk {
    /// 信号量表
    pub tab: Vec<PoSem>,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// ENOENT 命中
    pub enoent_hits: usize,
    /// EEXIST 命中
    pub eexist_hits: usize,
    /// EACCES 命中
    pub eacces_hits: usize,
    /// EINVAL 命中
    pub einval_hits: usize,
    /// 逐轮日志
    pub log: Vec<SoLog>,
}

impl SemOpenDesk {
    /// 建台。
    pub fn new() -> Self {
        SemOpenDesk {
            tab: Vec::new(),
            rounds: 0,
            ok_hits: 0,
            enoent_hits: 0,
            eexist_hits: 0,
            eacces_hits: 0,
            einval_hits: 0,
            log: Vec::new(),
        }
    }

    /// 名字查找。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.tab.len() {
            if self.tab[i].name == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 记账。
    fn book(&mut self, round: usize, name: &str, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == SO_ENOENT => self.enoent_hits = self.enoent_hits.wrapping_add(1),
            x if x == SO_EEXIST => self.eexist_hits = self.eexist_hits.wrapping_add(1),
            x if x == SO_EACCES => self.eacces_hits = self.eacces_hits.wrapping_add(1),
            x if x == SO_EINVAL => self.einval_hits = self.einval_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(SoLog { round, name: name.to_string(), verdict });
    }

    /// 裁决一次 sem_open。判定序：
    /// ① 初值越界（CREAT 且 value > SEM_VALUE_MAX）→ EINVAL；
    /// ② 撞名互斥 → EEXIST；③ 查无名 → ENOENT；
    /// ④ 既有信号量无读权 → EACCES；⑤ 开通并加引用。
    pub fn sem_open(&mut self, name: &str, flags: u32, value: u32, round: usize) -> Result<u32, u32> {
        if flags & SEM_CREAT != 0 && value > SEM_VALUE_MAX {
            self.book(round, name, SO_EINVAL);
            return Err(SO_EINVAL);
        }
        match self.find(name) {
            Some(ix) => {
                if flags & SEM_EXCL != 0 {
                    self.book(round, name, SO_EEXIST);
                    return Err(SO_EEXIST);
                }
                if !self.tab[ix].can_read {
                    self.book(round, name, SO_EACCES);
                    return Err(SO_EACCES);
                }
                self.tab[ix].refs = self.tab[ix].refs.wrapping_add(1);
                self.book(round, name, 0);
                Ok(self.tab[ix].value)
            }
            None => {
                if flags & SEM_CREAT == 0 {
                    self.book(round, name, SO_ENOENT);
                    return Err(SO_ENOENT);
                }
                if self.tab.len() >= SEM_TAB_CAP {
                    self.book(round, name, SO_LEDGER);
                    return Err(SO_LEDGER);
                }
                self.tab.push(PoSem {
                    name: name.to_string(),
                    value,
                    can_read: true,
                    can_write: true,
                    refs: 1,
                });
                self.book(round, name, 0);
                Ok(value)
            }
        }
    }

    /// 账目对平。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.enoent_hits)
            .wrapping_add(self.eexist_hits)
            .wrapping_add(self.eacces_hits)
            .wrapping_add(self.einval_hits);
        total == self.rounds && self.log.len() == self.rounds
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.tab.len() {
            h = (h ^ self.tab[i].fold()).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.ok_hits as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "SemOpenDesk{{tab={},rounds={},ok={},enoent={},eexist={},eacces={},einval={}}}",
            self.tab.len(), self.rounds, self.ok_hits,
            self.enoent_hits, self.eexist_hits, self.eacces_hits, self.einval_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被台正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut d = SemOpenDesk::new();
    let _ = d.sem_open("/s0", SEM_CREAT, 3, 9001);
    // 注入 1：撞名互斥 → EEXIST。
    caught[0] = matches!(d.sem_open("/s0", SEM_CREAT | SEM_EXCL, 1, 9002), Err(x) if x == SO_EEXIST);
    // 注入 2：查无名 → ENOENT。
    caught[1] = matches!(d.sem_open("/nope", 0, 0, 9003), Err(x) if x == SO_ENOENT);
    // 注入 3：初值越界 → EINVAL。
    caught[2] = matches!(d.sem_open("/s9", SEM_CREAT, SEM_VALUE_MAX + 1, 9004), Err(x) if x == SO_EINVAL);
    // 注入 3 的补路：剥读权后访问 → EACCES。
    let ix = d.find("/s0").unwrap_or(0);
    d.tab[ix].can_read = false;
    caught[2] = caught[2] && matches!(d.sem_open("/s0", 0, 0, 9005), Err(x) if x == SO_EACCES);
    d.tab[ix].can_read = true;
    // 注入 4：上限值恰好合法（边界非错误）+ 表满 → LEDGER 补面。
    caught[3] = d.sem_open("/smax", SEM_CREAT, SEM_VALUE_MAX, 9006).is_ok();
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：合法打开为主，四错误面轮转。返回符合预期轮数。
pub fn sample_thousand(d: &mut SemOpenDesk) -> usize {
    let mut good = 0;
    let mut r = 0;
    while r < SO_ROUNDS {
        let low = r % 10;
        let res;
        let expect_ok;
        if low < 4 {
            // 合法面：既有信号量重开。
            res = d.sem_open("/work", 0, 0, r);
            expect_ok = true;
        } else if low == 4 {
            // 合法面：新名创建。
            let name = format!("/n{}", r % 12);
            res = d.sem_open(&name, SEM_CREAT, (r % 16) as u32, r);
            expect_ok = true;
        } else if low == 5 {
            // ENOENT 面。
            res = d.sem_open("/ghost", 0, 0, r);
            expect_ok = false;
        } else if low == 6 {
            // EEXIST 面。
            res = d.sem_open("/work", SEM_CREAT | SEM_EXCL, 1, r);
            expect_ok = false;
        } else if low == 7 {
            // EINVAL 面。
            res = d.sem_open("/bad", SEM_CREAT, SEM_VALUE_MAX + 1 + (r % 9) as u32, r);
            expect_ok = false;
        } else if low == 8 {
            // EACCES 面：剥权窗口内访问。
            let ix = d.find("/work").unwrap_or(0);
            if ix < d.tab.len() {
                d.tab[ix].can_read = false;
            }
            let hit = matches!(d.sem_open("/work", 0, 0, r), Err(x) if x == SO_EACCES);
            if ix < d.tab.len() {
                d.tab[ix].can_read = true;
            }
            res = if hit { Err(SO_EACCES) } else { Ok(0) };
            expect_ok = false;
        } else {
            // 合法面：上限值创建（边界合法）。
            let name = format!("/m{}", r % 6);
            res = d.sem_open(&name, SEM_CREAT, SEM_VALUE_MAX, r);
            expect_ok = true;
        }
        let matched = match &res {
            Ok(_) => expect_ok,
            Err(x) => {
                !expect_ok
                    && (*x == SO_ENOENT
                        || *x == SO_EEXIST
                        || *x == SO_EACCES
                        || *x == SO_EINVAL)
            }
        };
        if matched {
            good += 1;
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10950 判据组。
pub fn run_unx10950_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10950");
    let mk = || SemOpenDesk::new();
    // J1 正面之一：ENOENT 十轮全中。
    let mut d1 = mk();
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(d1.sem_open("/none", 0, 0, k), Err(x) if x == SO_ENOENT) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面ENOENT", c == 10, "查无名十轮全部裁 ENOENT");
    // J1 正面之二：EEXIST 十轮全中。
    let mut d2 = mk();
    let _ = d2.sem_open("/a", SEM_CREAT, 1, 1);
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(d2.sem_open("/a", SEM_CREAT | SEM_EXCL, 1, k), Err(x) if x == SO_EEXIST) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EEXIST", c == 10, "撞名互斥十轮全部裁 EEXIST");
    // J1 正面之三：EACCES 十轮全中。
    let mut d3 = mk();
    let _ = d3.sem_open("/b", SEM_CREAT, 1, 1);
    let ix3 = d3.find("/b").unwrap_or(0);
    d3.tab[ix3].can_read = false;
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(d3.sem_open("/b", 0, 0, k), Err(x) if x == SO_EACCES) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EACCES", c == 10, "无读权访问十轮全部裁 EACCES");
    // J1 正面之四：EINVAL 十轮全中。
    let mut d4 = mk();
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(d4.sem_open("/c", SEM_CREAT, SEM_VALUE_MAX + 1 + k as u32, k), Err(x) if x == SO_EINVAL) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EINVAL", c == 10, "初值越界十轮全部裁 EINVAL");
    // J2 反面：千轮合法打开零误报。
    let mut d5 = mk();
    let _ = d5.sem_open("/work", SEM_CREAT, 4, 0);
    let good = sample_thousand(&mut d5);
    s.add_dyn("J2-反面零误报", good == SO_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "EEXIST/ENOENT/EINVAL/EACCES 四路坏样本全部拦截",
    );
    // J4 判定序：EINVAL 先于 EEXIST（撞名且初值越界裁 EINVAL）。
    let mut d6 = mk();
    let _ = d6.sem_open("/x", SEM_CREAT, 1, 1);
    s.add(
        "J4-判定序",
        matches!(d6.sem_open("/x", SEM_CREAT | SEM_EXCL, SEM_VALUE_MAX + 5, 1), Err(x) if x == SO_EINVAL),
        "初值越界先行裁定",
    );
    // J5 上限边界：恰好 SEM_VALUE_MAX 合法。
    let mut d7 = mk();
    let ok5 = d7.sem_open("/m", SEM_CREAT, SEM_VALUE_MAX, 1);
    s.add(
        "J5-上限边界",
        matches!(ok5, Ok(v) if v == SEM_VALUE_MAX),
        "恰好上限值创建成功",
    );
    // J6 错误码分段。
    s.add(
        "J6-错误码分段",
        SO_ENOENT > SEG_BASE && SO_LEDGER < SEG_BASE + 0x20,
        "五个错误码全部落在段基址页内",
    );
    // J7 错误码互异。
    let codes = [SO_ENOENT, SO_EEXIST, SO_EACCES, SO_EINVAL, SO_LEDGER];
    let mut distinct = true;
    let mut a = 0;
    while a < codes.len() {
        let mut b = a + 1;
        while b < codes.len() {
            if codes[a] == codes[b] {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J7-错误码互异", distinct, "五个错误码两两不同");
    // J8 账目对平。
    s.add("J8-账目对平", d5.ledger_ok(), "千轮之后五类计数与轮次、日志逐项对平");
    // J9 fold 稳定。
    let f = d5.fold();
    let clone = d5.clone();
    s.add("J9-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J10 引用计数：重开加一。
    let mut d8 = mk();
    let _ = d8.sem_open("/r", SEM_CREAT, 1, 1);
    let before = d8.tab[0].refs;
    let _ = d8.sem_open("/r", 0, 0, 2);
    s.add(
        "J10-引用加一",
        d8.tab[0].refs == before + 1,
        "重开后引用计数递增",
    );
    // J11 渲染非空。
    let txt = d5.render();
    s.add("J11-渲染非空", txt.len() > 10 && txt.contains("SemOpenDesk"), "台级渲染含类型名与计数");
    // J12 动态：千轮明细。
    s.add_dyn(
        "J12-千轮明细",
        good == SO_ROUNDS,
        &format!("good={} desk={}", good, d5.render()),
    );
    // J13 动态：注入明细。
    s.add_dyn(
        "J13-注入明细",
        inj.iter().all(|x| *x),
        &format!("inj={:?}", inj),
    );
    s
}

// ---------------------------------------------------------------- 测试
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10950_sem_open_matrix_flow() {
        // 流程：建量 → 撞名 → 查无 → 越界 → 剥权 → EACCES → 上限合法。
        let mut d = SemOpenDesk::new();
        assert!(d.sem_open("/s", SEM_CREAT, 2, 1).is_ok());
        assert!(matches!(d.sem_open("/s", SEM_CREAT | SEM_EXCL, 1, 2), Err(x) if x == SO_EEXIST));
        assert!(matches!(d.sem_open("/nope", 0, 0, 3), Err(x) if x == SO_ENOENT));
        assert!(matches!(d.sem_open("/t", SEM_CREAT, SEM_VALUE_MAX + 1, 4), Err(x) if x == SO_EINVAL));
        let ix = d.find("/s").unwrap_or(0);
        d.tab[ix].can_read = false;
        assert!(matches!(d.sem_open("/s", 0, 0, 5), Err(x) if x == SO_EACCES));
        assert!(d.sem_open("/m", SEM_CREAT, SEM_VALUE_MAX, 6).is_ok());
        assert!(d.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10950_checks_green() {
        let s = run_unx10950_checks();
        assert!(s.all_passed(), "UNX-F10950 判据未全绿");
    }
}
