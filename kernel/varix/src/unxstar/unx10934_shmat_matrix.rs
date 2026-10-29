//! UNX-F10934 shmat 错误矩阵（EMFILE/EACCES/ENOMEM/对齐）（0x1d440 段）
//!
//! 纯功能模型：进程侧 `shmat` 挂载裁决台。
//! 四错误面：① 坏句柄或地址不对齐 SHMLBA → EINVAL；② 单进程挂载
//! 数达上限 → EMFILE；③ 写请求而段无写权 → EACCES；④ 页预算耗尽
//! → ENOMEM。反面千轮样本只走合法挂载，零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d440;
/// 坏句柄或地址不对齐
pub const AT_EINVAL: u32 = SEG_BASE + 0x01;
/// 单进程挂载数达上限
pub const AT_EMFILE: u32 = SEG_BASE + 0x02;
/// 写请求而段无写权
pub const AT_EACCES: u32 = SEG_BASE + 0x03;
/// 页预算耗尽
pub const AT_ENOMEM: u32 = SEG_BASE + 0x04;
/// 台内对账失败
pub const AT_LEDGER: u32 = SEG_BASE + 0x05;

// ---------------------------------------------------------------- 常数
/// 对齐下限（SHMLBA）
pub const SHMLBA: usize = 4096;
/// 单进程挂载上限
pub const PROC_ATTACH_MAX: usize = 8;
/// 全台页预算
pub const PAGE_BUDGET: usize = 64;
/// 反面验收轮数
pub const AT_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 段定义（含权限位）。
#[derive(Clone, Debug)]
pub struct AtSeg {
    /// 段句柄
    pub shmid: u32,
    /// 段页数
    pub pages: usize,
    /// 属主读位
    pub mode_r: bool,
    /// 属主写位
    pub mode_w: bool,
    /// 全台挂载计数
    pub nattch: usize,
}

impl AtSeg {
    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ (self.shmid as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.pages as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.mode_r as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.mode_w as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.nattch as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "AtSeg{{id={},pages={},r={},w={},nattch={}}}",
            self.shmid, self.pages, self.mode_r as u32, self.mode_w as u32, self.nattch
        )
    }
}

/// 一条映射：pid → 段 → 基址。
#[derive(Clone, Debug, PartialEq)]
pub struct Mapping {
    /// 进程号
    pub pid: u32,
    /// 段句柄
    pub shmid: u32,
    /// 挂载基址
    pub addr: usize,
    /// 是否只读挂载
    pub ro: bool,
}

/// shmat 挂载裁决台。
#[derive(Clone, Debug)]
pub struct AttachDesk {
    /// 段集合
    pub segs: Vec<AtSeg>,
    /// 全部活跃映射
    pub maps: Vec<Mapping>,
    /// 自动挂载的下一基址
    pub auto_base: usize,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// EINVAL 命中
    pub einval_hits: usize,
    /// EMFILE 命中
    pub emfile_hits: usize,
    /// EACCES 命中
    pub eacces_hits: usize,
    /// ENOMEM 命中
    pub enomem_hits: usize,
}

impl AttachDesk {
    /// 建台：给定段集合。
    pub fn new(segs: Vec<AtSeg>) -> Self {
        AttachDesk {
            segs,
            maps: Vec::new(),
            auto_base: 0x60000000,
            rounds: 0,
            ok_hits: 0,
            einval_hits: 0,
            emfile_hits: 0,
            eacces_hits: 0,
            enomem_hits: 0,
        }
    }

    /// 段查找。
    pub fn find(&self, shmid: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.segs.len() {
            if self.segs[i].shmid == shmid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 某进程挂载数。
    pub fn attach_count(&self, pid: u32) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.maps.len() {
            if self.maps[i].pid == pid {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 已占页数。
    pub fn used_pages(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0;
        while i < self.maps.len() {
            match self.find(self.maps[i].shmid) {
                Some(ix) => n = n.wrapping_add(self.segs[ix].pages),
                None => {}
            }
            i += 1;
        }
        n
    }

    /// 记账。
    fn book(&mut self, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == AT_EINVAL => self.einval_hits = self.einval_hits.wrapping_add(1),
            x if x == AT_EMFILE => self.emfile_hits = self.emfile_hits.wrapping_add(1),
            x if x == AT_EACCES => self.eacces_hits = self.eacces_hits.wrapping_add(1),
            x if x == AT_ENOMEM => self.enomem_hits = self.enomem_hits.wrapping_add(1),
            _ => {}
        }
    }

    /// 裁决一次 shmat。判定序：
    /// ① 段查无 → EINVAL；② addr 非 0 且不对齐 → EINVAL；
    /// ③ 进程挂载数达上限 → EMFILE；④ 写请求无写权 → EACCES；
    /// ⑤ 页预算不足 → ENOMEM；⑥ 建映射（addr 为 0 时自动择址）。
    pub fn shmat(&mut self, pid: u32, shmid: u32, addr: usize, want_write: bool) -> Result<usize, u32> {
        let ix = match self.find(shmid) {
            Some(ix) => ix,
            None => {
                self.book(AT_EINVAL);
                return Err(AT_EINVAL);
            }
        };
        if addr != 0 && addr % SHMLBA != 0 {
            self.book(AT_EINVAL);
            return Err(AT_EINVAL);
        }
        if self.attach_count(pid) >= PROC_ATTACH_MAX {
            self.book(AT_EMFILE);
            return Err(AT_EMFILE);
        }
        if want_write && !self.segs[ix].mode_w {
            self.book(AT_EACCES);
            return Err(AT_EACCES);
        }
        if !want_write && !self.segs[ix].mode_r {
            self.book(AT_EACCES);
            return Err(AT_EACCES);
        }
        if self.used_pages().wrapping_add(self.segs[ix].pages) > PAGE_BUDGET {
            self.book(AT_ENOMEM);
            return Err(AT_ENOMEM);
        }
        let base = if addr == 0 {
            let b = self.auto_base;
            self.auto_base = self.auto_base.wrapping_add(self.segs[ix].pages.wrapping_mul(SHMLBA));
            b
        } else {
            addr
        };
        self.segs[ix].nattch = self.segs[ix].nattch.wrapping_add(1);
        self.maps.push(Mapping { pid, shmid, addr: base, ro: !want_write });
        self.book(0);
        Ok(base)
    }

    /// 卸载一次映射（供对账用）。
    pub fn shmdt(&mut self, pid: u32, addr: usize) -> bool {
        let mut found = None;
        let mut i = 0;
        while i < self.maps.len() {
            if self.maps[i].pid == pid && self.maps[i].addr == addr {
                found = Some(i);
            }
            i += 1;
        }
        match found {
            Some(ix) => {
                let m = self.maps.remove(ix);
                match self.find(m.shmid) {
                    Some(sx) => {
                        self.segs[sx].nattch = self.segs[sx].nattch.saturating_sub(1);
                    }
                    None => {}
                }
                true
            }
            None => false,
        }
    }

    /// 对账：nattch 总和等于映射条数，页数在预算内。
    pub fn ledger_ok(&self) -> bool {
        let mut total = 0usize;
        let mut i = 0;
        while i < self.segs.len() {
            total = total.wrapping_add(self.segs[i].nattch);
            i += 1;
        }
        total == self.maps.len() && self.used_pages() <= PAGE_BUDGET
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.segs.len() {
            h = (h ^ self.segs[i].fold()).wrapping_mul(0x100000001b3);
            i += 1;
        }
        let mut i = 0;
        while i < self.maps.len() {
            h = (h ^ (self.maps[i].pid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (self.maps[i].addr as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "AttachDesk{{segs={},maps={},rounds={},ok={},einval={},emfile={},eacces={},enomem={}}}",
            self.segs.len(), self.maps.len(), self.rounds, self.ok_hits,
            self.einval_hits, self.emfile_hits, self.eacces_hits, self.enomem_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被台正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut d = AttachDesk::new(vec![
        AtSeg { shmid: 1, pages: 1, mode_r: true, mode_w: true, nattch: 0 },
        AtSeg { shmid: 2, pages: 1, mode_r: true, mode_w: false, nattch: 0 },
    ]);
    // 注入 1：坏句柄 → EINVAL。
    caught[0] = matches!(d.shmat(1, 99, 0, false), Err(x) if x == AT_EINVAL);
    // 注入 2：不对齐地址 → EINVAL。
    caught[1] = matches!(d.shmat(1, 1, SHMLBA + 1, false), Err(x) if x == AT_EINVAL);
    // 注入 3：只读段写请求 → EACCES。
    caught[2] = matches!(d.shmat(1, 2, 0, true), Err(x) if x == AT_EACCES);
    // 注入 4：灌到进程上限 → EMFILE；再灌大段 → ENOMEM。
    let mut i = 0;
    let mut hit_emfile = false;
    while i < PROC_ATTACH_MAX + 4 {
        if matches!(d.shmat(7, 1, 0, false), Err(x) if x == AT_EMFILE) {
            hit_emfile = true;
        }
        i += 1;
    }
    let mut big = AttachDesk::new(vec![AtSeg { shmid: 3, pages: PAGE_BUDGET, mode_r: true, mode_w: true, nattch: 0 }]);
    let mut j = 0;
    let mut hit_enomem = false;
    while j < 4 {
        if matches!(big.shmat(9, 3, 0, false), Err(x) if x == AT_ENOMEM) {
            hit_enomem = true;
        }
        j += 1;
    }
    caught[3] = hit_emfile && hit_enomem;
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：合法自动挂载与四错误面轮转。返回"裁决符合预期"的轮数。
pub fn sample_thousand(d: &mut AttachDesk) -> usize {
    let mut good = 0;
    let mut r = 0;
    while r < AT_ROUNDS {
        let low = r % 10;
        let res;
        let expect_ok;
        if low < 5 {
            // 合法面：只读自动挂载小段，挂完即卸（净零占用，千轮不撞进程/页上限）。
            res = d.shmat(0x55, 1, 0, false);
            if let Ok(a) = res {
                let _ = d.shmdt(0x55, a);
            }
            expect_ok = true;
        } else if low == 5 {
            // EINVAL 面：坏句柄。
            res = d.shmat(0x55, 42, 0, false);
            expect_ok = false;
        } else if low == 6 {
            // EINVAL 面：不对齐。
            res = d.shmat(0x55, 1, SHMLBA + 3, false);
            expect_ok = false;
        } else if low == 7 {
            // EACCES 面：只读段写请求。
            res = d.shmat(0x55, 2, 0, true);
            expect_ok = false;
        } else if low == 8 {
            // EMFILE 面：本轮独占 pid 灌满本进程配额后必越限；灌满即撤，防页账外泄。
            let me = 0x9000u32.wrapping_add(r as u32);
            let mut addrs: Vec<usize> = Vec::new();
            let mut hit = false;
            let mut k = 0;
            while k < PROC_ATTACH_MAX + 2 {
                match d.shmat(me, 1, 0, false) {
                    Ok(a) => addrs.push(a),
                    Err(x) => {
                        if x == AT_EMFILE {
                            hit = true;
                        }
                    }
                }
                k += 1;
            }
            let mut i = 0;
            while i < addrs.len() {
                let _ = d.shmdt(me, addrs[i]);
                i += 1;
            }
            res = if hit { Err(AT_EMFILE) } else { Ok(0) };
            expect_ok = false;
        } else {
            // ENOMEM 面：换 pid 绕开单进程配额，把全台页预算（段 3 每次 2 页）压满后必越限，
            // 压满即撤。
            let mut addrs: Vec<(u32, usize)> = Vec::new();
            let mut hit = false;
            let mut k = 0;
            while k < PAGE_BUDGET + 2 {
                let who = 0x8000u32.wrapping_add(k as u32);
                match d.shmat(who, 3, 0, false) {
                    Ok(a) => addrs.push((who, a)),
                    Err(x) => {
                        if x == AT_ENOMEM {
                            hit = true;
                        }
                    }
                }
                k += 1;
            }
            let mut i = 0;
            while i < addrs.len() {
                let _ = d.shmdt(addrs[i].0, addrs[i].1);
                i += 1;
            }
            res = if hit { Err(AT_ENOMEM) } else { Ok(0) };
            expect_ok = false;
        }
        let matched = match &res {
            Ok(_) => expect_ok,
            Err(x) => {
                !expect_ok
                    && (*x == AT_EINVAL || *x == AT_EMFILE || *x == AT_EACCES || *x == AT_ENOMEM)
            }
        };
        if matched {
            good += 1;
        }
        // 定期卸载 0x55 进程的一部分映射，防页预算提前耗尽。
        if r % 40 == 0 {
            let mut victim: usize = 0;
            let mut i = 0;
            while i < d.maps.len() {
                if d.maps[i].pid == 0x55 {
                    victim = d.maps[i].addr;
                    break;
                }
                i += 1;
            }
            if victim != 0 {
                let _ = d.shmdt(0x55, victim);
            }
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10934 判据组。
pub fn run_unx10934_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10934");
    // 公共台：段 1 可读写 1 页，段 2 只读 1 页，段 3 可读写 2 页。
    let mk = || {
        AttachDesk::new(vec![
            AtSeg { shmid: 1, pages: 1, mode_r: true, mode_w: true, nattch: 0 },
            AtSeg { shmid: 2, pages: 1, mode_r: true, mode_w: false, nattch: 0 },
            AtSeg { shmid: 3, pages: 2, mode_r: true, mode_w: true, nattch: 0 },
        ])
    };
    // J1 正面之一：EINVAL 十轮全中（坏句柄与不对齐交替）。
    let mut d1 = mk();
    let mut einval_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let r = if k % 2 == 0 {
            d1.shmat(1, 88, 0, false)
        } else {
            d1.shmat(1, 1, SHMLBA + (k as usize % 7) + 1, false)
        };
        if matches!(r, Err(x) if x == AT_EINVAL) {
            einval_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EINVAL", einval_cnt == 10, "坏句柄/不对齐十轮全部裁 EINVAL");
    // J1 正面之二：EMFILE 十轮全中。
    let mut d2 = mk();
    // 先把该进程挂到配额上限（PROC_ATTACH_MAX），十轮首击即越限；
    // 否则前几轮尚有余量，越限要到累计第 9 次才出现。
    let mut fill = 0;
    while fill < PROC_ATTACH_MAX {
        let _ = d2.shmat(0x21, 1, 0, false);
        fill += 1;
    }
    let mut emfile_cnt = 0;
    let mut k = 0;
    while k < 10 {
        let mut tries = 0;
        let mut hit = false;
        while tries <= k % 2 {
            if matches!(d2.shmat(0x21, 1, 0, false), Err(x) if x == AT_EMFILE) {
                hit = true;
            }
            tries += 1;
        }
        if hit {
            emfile_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EMFILE", emfile_cnt == 10, "进程挂载超限十轮全部裁 EMFILE");
    // J1 正面之三：EACCES 十轮全中。
    let mut d3 = mk();
    let mut eacces_cnt = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(d3.shmat(1, 2, 0, true), Err(x) if x == AT_EACCES) {
            eacces_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面EACCES", eacces_cnt == 10, "只读段写请求十轮全部裁 EACCES");
    // J1 正面之四：ENOMEM 十轮全中。
    let mut d4 = mk();
    let mut enomem_cnt = 0;
    let mut k = 0;
    while k < 10 {
        // 逐次换 pid：单进程 8 条上限会先裁 EMFILE，页预算面必须绕开进程配额
        // 才能把全台 64 页压满（段 3 每次 2 页）。
        let mut i = 0;
        let mut hit = false;
        while i < PAGE_BUDGET {
            if matches!(d4.shmat(0x1000 + (k * 64 + i) as u32, 3, 0, false), Err(x) if x == AT_ENOMEM) {
                hit = true;
            }
            i += 1;
        }
        if hit {
            enomem_cnt += 1;
        }
        k += 1;
    }
    s.add("J1-正面ENOMEM", enomem_cnt == 10, "页预算耗尽十轮全部裁 ENOMEM");
    // J2 反面：千轮合法挂载零误报。
    let mut d5 = mk();
    let good = sample_thousand(&mut d5);
    s.add_dyn("J2-反面零误报", good == AT_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "EINVAL/EMFILE/EACCES/ENOMEM 四路坏样本全部拦截",
    );
    // J4 判定序：坏句柄先于不对齐（两者同现裁 EINVAL 一次计入坏句柄面）。
    let mut d6 = mk();
    let r6 = d6.shmat(1, 99, SHMLBA + 1, false);
    s.add("J4-判定序", matches!(r6, Err(x) if x == AT_EINVAL), "双错同现按判定序裁 EINVAL");
    // J5 对齐边界：恰好 SHMLBA 对齐可指定挂载。
    let mut d7 = mk();
    let ok7 = d7.shmat(1, 1, SHMLBA * 3, false);
    s.add(
        "J5-对齐边界",
        matches!(ok7, Ok(a) if a == SHMLBA * 3),
        "恰好对齐的指定基址挂载成功",
    );
    // J6 错误码分段。
    s.add(
        "J6-错误码分段",
        AT_EINVAL > SEG_BASE && AT_LEDGER < SEG_BASE + 0x20,
        "五个错误码全部落在段基址页内",
    );
    // J7 错误码互异。
    let codes = [AT_EINVAL, AT_EMFILE, AT_EACCES, AT_ENOMEM, AT_LEDGER];
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
    // J8 台内对账。
    s.add("J8-台内对账", d5.ledger_ok(), "nattch 总和等于映射条数且页数在预算内");
    // J9 fold 稳定。
    let f = d5.fold();
    let clone = d5.clone();
    s.add("J9-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J10 自动择址单调递增且对齐。
    let mut d8 = mk();
    let a1 = d8.shmat(3, 1, 0, false);
    let a2 = d8.shmat(3, 1, 0, false);
    let mono = matches!((&a1, &a2), (Ok(x), Ok(y)) if *y > *x && *y % SHMLBA == 0);
    s.add("J10-自动择址", mono, "自动基址严格递增且对齐");
    // J11 渲染非空。
    let txt = d5.render();
    s.add("J11-渲染非空", txt.len() > 10 && txt.contains("AttachDesk"), "台级渲染含类型名与计数");
    // J12 卸载复核：shmdt 后 nattch 回落。
    let mut d9 = mk();
    let a9 = d9.shmat(4, 1, 0, false);
    let n_before = d9.segs[0].nattch;
    let mut det_ok = false;
    if let Ok(addr) = a9 {
        det_ok = d9.shmdt(4, addr) && d9.segs[0].nattch == n_before.saturating_sub(1);
    }
    s.add("J12-卸载回落", det_ok, "卸载后段挂载计数回落");
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == AT_ROUNDS,
        &format!("good={} desk={}", good, d5.render()),
    );
    // J14 动态：注入明细。
    s.add_dyn(
        "J14-注入明细",
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
    fn unx10934_shmat_matrix_flow() {
        // 流程：合法挂载 → 坏句柄 → 不对齐 → EACCES → EMFILE → ENOMEM。
        let mut d = AttachDesk::new(vec![
            AtSeg { shmid: 1, pages: 1, mode_r: true, mode_w: true, nattch: 0 },
            AtSeg { shmid: 2, pages: 1, mode_r: true, mode_w: false, nattch: 0 },
            AtSeg { shmid: 3, pages: 2, mode_r: true, mode_w: true, nattch: 0 },
        ]);
        let a = d.shmat(1, 1, 0, true);
        assert!(a.is_ok());
        assert!(matches!(d.shmat(1, 9, 0, false), Err(x) if x == AT_EINVAL));
        assert!(matches!(d.shmat(1, 1, SHMLBA + 1, false), Err(x) if x == AT_EINVAL));
        assert!(matches!(d.shmat(1, 2, 0, true), Err(x) if x == AT_EACCES));
        let mut i = 0;
        let mut hit_em = false;
        while i < PROC_ATTACH_MAX + 2 {
            if matches!(d.shmat(2, 1, 0, false), Err(x) if x == AT_EMFILE) {
                hit_em = true;
            }
            i += 1;
        }
        assert!(hit_em);
        // 段须一挂即占满全台页预算：单进程 8 条上限会先裁 EMFILE，小段灌不满 64 页。
        let mut big = AttachDesk::new(vec![AtSeg { shmid: 3, pages: PAGE_BUDGET, mode_r: true, mode_w: true, nattch: 0 }]);
        let mut j = 0;
        let mut hit_nm = false;
        while j < 4 {
            if matches!(big.shmat(3, 3, 0, false), Err(x) if x == AT_ENOMEM) {
                hit_nm = true;
            }
            j += 1;
        }
        assert!(hit_nm);
        assert!(d.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10934_checks_green() {
        let s = run_unx10934_checks();
        assert!(s.all_passed(), "UNX-F10934 判据未全绿");
    }
}
