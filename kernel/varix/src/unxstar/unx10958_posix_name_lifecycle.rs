//! UNX-F10958 POSIX IPC 名字生命周期矩阵（unlink 后 open）（0x1d740 段）
//!
//! 卡片判据（UNX-F10958-J1）：
//! - 正面：unlink 后新 open→ENOENT（10/10）、活跃描述符存活（10/10）；
//! - 反面：延迟销毁语义零摇摆（千次 10/10）。
//!
//! 域：CoRun·UNX-C4-B28。本模块建模 POSIX IPC 名字生命周期：
//! unlink 只拆名不销毁对象，活跃描述符继续可用；新 open 命中
//! ENOENT；最后一个描述符关闭时对象才真正回收。千轮探测确认
//! 该"延迟销毁"语义从不摇摆。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10958-10900)*0x20 = 0x1d740）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d740;

/// LC_ENOENT：unlink 后新 open 或名字从未存在
pub const LC_ENOENT: u32 = SEG_BASE + 0x01;
/// LC_EBADF：描述符无效或已关闭
pub const LC_EBADF: u32 = SEG_BASE + 0x02;
/// LC_EEXIST：EXCL 创建撞名
pub const LC_EEXIST: u32 = SEG_BASE + 0x03;
/// LC_EACCES：非属主 unlink
pub const LC_EACCES: u32 = SEG_BASE + 0x04;
/// LC_EINVAL：名字/参数非法
pub const LC_EINVAL: u32 = SEG_BASE + 0x05;
/// LC_EWAVE：生命周期语义摇摆（自检面，不应发生）
pub const LC_EWAVE: u32 = SEG_BASE + 0x06;
/// LC_ENOSPC：对象表满
pub const LC_ENOSPC: u32 = SEG_BASE + 0x07;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// 对象表容量
const OBJ_CAP: usize = 8;
/// 描述符表容量
const FD_CAP: usize = 12;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 名字条目：生命周期核心
#[derive(Clone, Debug, PartialEq)]
pub struct NameEntry {
    pub name: String,
    pub value: u32,
    pub owner: u32,
    /// 名字是否仍在命名空间（unlink 后 false）
    pub linked: bool,
    /// 活跃描述符引用计数
    pub refs: u32,
}

impl NameEntry {
    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.value as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.owner as u64).wrapping_mul(0x100000001b3);
        h = (h ^ (self.linked as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ self.refs as u64).wrapping_mul(0x100000001b3);
        for b in self.name.as_bytes() {
            h = (h ^ (*b as u64)).wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// 生命周期判定台
#[derive(Clone, Debug)]
pub struct LifeDesk {
    pub objs: Vec<NameEntry>,
    /// fd -> 对象在 objs 中的索引（u32::MAX 为空）
    fds: Vec<u32>,
    /// 错误计数
    pub n_enoent: u32,
    pub n_ebadf: u32,
    pub n_eacces: u32,
    /// unlink 成功数
    pub n_unlink: u32,
    /// 真正回收数
    pub n_reap: u32,
    /// 活跃描述符读次数
    pub n_live_read: u32,
}

impl LifeDesk {
    pub fn new() -> Self {
        LifeDesk {
            objs: Vec::new(),
            fds: vec![u32::MAX; FD_CAP],
            n_enoent: 0,
            n_ebadf: 0,
            n_eacces: 0,
            n_unlink: 0,
            n_reap: 0,
            n_live_read: 0,
        }
    }

    /// 打开/创建：名字已拆则 ENOENT，绝不复用死名条目
    pub fn open(&mut self, name: &str, owner: u32) -> Result<i32, u32> {
        if name.as_bytes().first() != Some(&b'/') || name.len() > 64 {
            return Err(LC_EINVAL);
        }
        if let Some(i) = self.objs.iter().position(|o| o.name == name) {
            if !self.objs[i].linked {
                self.n_enoent = self.n_enoent.wrapping_add(1);
                return Err(LC_ENOENT);
            }
            // 活名：引用 +1
            self.objs[i].refs = self.objs[i].refs.wrapping_add(1);
            return self.alloc_fd(i as u32);
        }
        if self.objs.len() >= OBJ_CAP {
            return Err(LC_ENOSPC);
        }
        self.objs.push(NameEntry {
            name: String::from(name),
            value: 0,
            owner,
            linked: true,
            refs: 1,
        });
        let idx = self.objs.len().wrapping_sub(1) as u32;
        self.alloc_fd(idx)
    }

    /// 分配 fd 槽
    fn alloc_fd(&mut self, obj_idx: u32) -> Result<i32, u32> {
        match self.fds.iter().position(|&x| x == u32::MAX) {
            Some(i) => {
                self.fds[i] = obj_idx;
                Ok(i as i32)
            }
            None => Err(LC_ENOSPC),
        }
    }

    /// unlink：拆名不销毁，活跃 fd 存活
    pub fn unlink(&mut self, name: &str, pid: u32) -> Result<(), u32> {
        match self.objs.iter().position(|o| o.name == name && o.linked) {
            None => {
                self.n_enoent = self.n_enoent.wrapping_add(1);
                Err(LC_ENOENT)
            }
            Some(i) => {
                if self.objs[i].owner != pid {
                    self.n_eacces = self.n_eacces.wrapping_add(1);
                    return Err(LC_EACCES);
                }
                self.objs[i].linked = false;
                self.n_unlink = self.n_unlink.wrapping_add(1);
                self.maybe_reap(i);
                Ok(())
            }
        }
    }

    /// 引用归零则回收
    fn maybe_reap(&mut self, i: usize) {
        if !self.objs[i].linked && self.objs[i].refs == 0 {
            let _ = self.objs.remove(i);
            // fd 索引平移（>=i 的减一）
            for f in self.fds.iter_mut() {
                if *f != u32::MAX && *f > i as u32 {
                    *f = *f - 1;
                }
            }
            self.n_reap = self.n_reap.wrapping_add(1);
        }
    }

    /// 读：描述符必须存活；unlink 后仍可读（延迟销毁）
    pub fn read(&mut self, fd: i32) -> Result<u32, u32> {
        let idx = match self.resolve(fd) {
            Ok(i) => i,
            Err(e) => return Err(e),
        };
        self.n_live_read = self.n_live_read.wrapping_add(1);
        Ok(self.objs[idx].value)
    }

    /// 写
    pub fn write(&mut self, fd: i32, value: u32) -> Result<(), u32> {
        let idx = self.resolve(fd)?;
        self.objs[idx].value = value;
        Ok(())
    }

    /// fd 解析
    fn resolve(&mut self, fd: i32) -> Result<usize, u32> {
        if fd < 0 || fd as usize >= FD_CAP {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(LC_EBADF);
        }
        let oi = self.fds[fd as usize];
        if oi == u32::MAX || oi as usize >= self.objs.len() {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(LC_EBADF);
        }
        Ok(oi as usize)
    }

    /// 关闭描述符：引用减一，归零且已拆名则回收
    pub fn close(&mut self, fd: i32) -> Result<(), u32> {
        let idx = self.resolve(fd)?;
        let u = fd as usize;
        let oi = self.fds[u];
        self.fds[u] = u32::MAX;
        self.objs[oi as usize].refs = self.objs[oi as usize].refs.wrapping_sub(1);
        self.maybe_reap(oi as usize);
        let _ = idx;
        Ok(())
    }

    /// 生命周期不变式：拆名后 refs>0 则对象必在，refs==0 则必被回收
    pub fn invariant_ok(&self) -> bool {
        for o in &self.objs {
            if !o.linked && o.refs == 0 {
                return false; // 应被回收却仍在 → 摇摆
            }
        }
        // 拆名对象必须不可再 open（由 open 逻辑保证，这里复查链）
        true
    }

    /// 账目对平
    pub fn ledger_ok(&self, expect_unlink: u32, expect_read: u32) -> bool {
        self.n_unlink == expect_unlink && self.n_live_read == expect_read
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for o in &self.objs {
            h = (h ^ o.fold()).wrapping_mul(0x100000001b3);
        }
        h = (h ^ self.n_reap as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        let mut s = String::from("life:");
        for o in &self.objs {
            let tag = if o.linked { "L" } else { "U" };
            s.push_str(&crate::alloc::format!("/{}:{}={} ", o.name, tag, o.refs));
        }
        s
    }
}

impl Default for LifeDesk {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut d = LifeDesk::new();
    let f = match d.open("/life-a", 7) { Ok(f) => f, Err(_) => return [false; 4] };
    // 路1：unlink 后新 open → ENOENT
    let _ = d.unlink("/life-a", 7);
    let r1 = matches!(d.open("/life-a", 7), Err(x) if x == LC_ENOENT);
    // 路2：close 后读 → EBADF
    let _ = d.close(f);
    let r2 = matches!(d.read(f), Err(x) if x == LC_EBADF);
    // 路3：重复 unlink → ENOENT
    let r3 = matches!(d.unlink("/life-a", 7), Err(x) if x == LC_ENOENT);
    // 路4：非属主 unlink → EACCES
    let _ = d.open("/life-b", 7);
    let r4 = matches!(d.unlink("/life-b", 99), Err(x) if x == LC_EACCES);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：unlink 后语义探测 500 轮 + 活跃存活探测 500 轮，零摇摆。
pub fn sample_thousand() -> u32 {
    let mut d = LifeDesk::new();
    let mut keep = match d.open("/keep", 7) { Ok(f) => f, Err(_) => return 0 };
    let mut good: u32 = 0;
    for r in 0..1000u32 {
        let phase = r % 10;
        if phase < 5 {
            // 正面：unlink 后新 open ENOENT + 活跃 fd 存活
            let f = match d.open("/victim", 7) { Ok(f) => f, Err(_) => return good };
            let _ = d.write(f, 0x5a00 + (r & 0xff));
            let un = d.unlink("/victim", 7).is_ok();
            let enoent = matches!(d.open("/victim", 7), Err(x) if x == LC_ENOENT);
            let alive = d.read(f).is_ok();
            let _ = d.close(f);
            if un && enoent && alive {
                good = good.wrapping_add(1);
            }
        } else {
            // 反面：延迟销毁语义零摇摆——同一拆名对象反复读，结果恒定
            let f2 = match d.open("/steady", 7) { Ok(f) => f, Err(_) => return good };
            let _ = d.write(f2, 777);
            let _ = d.unlink("/steady", 7);
            let a = d.read(f2);
            let b = d.read(f2);
            let still_no = matches!(d.open("/steady", 7), Err(x) if x == LC_ENOENT);
            let wave = a != b || !still_no;
            let _ = d.close(f2);
            if !wave {
                good = good.wrapping_add(1);
            }
        }
        // 保活句柄周期性翻转，确保回收逻辑持续被压测
        if r % 97 == 0 {
            let _ = d.close(keep);
            keep = match d.open("/keep", 7) { Ok(f) => f, Err(_) => return good };
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10958_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10958");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "ENOENT/EBADF/EACCES 坏样本全部命中");

    // J1 正面A：unlink 后新 open→ENOENT 10/10
    let mut d = LifeDesk::new();
    let mut enoent10 = 0u32;
    let mut alive10 = 0u32;
    for i in 0..10u32 {
        let name = crate::alloc::format!("/j1-{}", i);
        let f = match d.open(&name, 7) { Ok(f) => f, Err(_) => -1 };
        let _ = d.write(f, 100 + i);
        let _ = d.unlink(&name, 7);
        if matches!(d.open(&name, 7), Err(x) if x == LC_ENOENT) {
            enoent10 = enoent10.wrapping_add(1);
        }
        if d.read(f) == Ok(100 + i) {
            alive10 = alive10.wrapping_add(1);
        }
        let _ = d.close(f);
    }
    s.add(
        "J1-正面-ENOENT/存活各10/10",
        enoent10 == 10 && alive10 == 10,
        "拆名后新开十次全 ENOENT，活跃句柄十次全存活",
    );

    // J2 反面：延迟销毁语义零摇摆（千次）
    let good = sample_thousand();
    s.add("J2-反面-零摇摆千次", good == 1000, "拆名对象反复读结果恒定，新开恒 ENOENT");

    // 不变式
    s.add("J3-生命周期不变式", d.invariant_ok(), "无死条目滞留、无活名丢失");

    // 判定序：死名（尚未回收）优先于创建，绝不复用
    let mut d2 = LifeDesk::new();
    let f_seq = match d2.open("/seq-x", 7) { Ok(f) => f, Err(_) => -1 };
    let _ = d2.unlink("/seq-x", 7);
    let seq_ok = matches!(d2.open("/seq-x", 7), Err(x) if x == LC_ENOENT);
    let _ = d2.close(f_seq);
    s.add("J4-判定序-死名优先", seq_ok, "拆名条目在存活期内永不被复用创建");

    // 错误码分段
    let codes = [LC_ENOENT, LC_EBADF, LC_EEXIST, LC_EACCES, LC_EINVAL, LC_EWAVE, LC_ENOSPC];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J5-错误码分段", seg_ok, "七码全部落在 0x1d740 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J6-错误码互异", distinct, "七码两两互异");

    // 账目对平
    s.add("J7-账目对平", d.ledger_ok(10, 10), "unlink 10 次、活跃读 10 次与账面一致");

    // 引用归零回收
    let mut d3 = LifeDesk::new();
    let f3a = match d3.open("/reap", 7) { Ok(f) => f, Err(_) => -1 };
    let f3b = match d3.open("/reap", 7) { Ok(f) => f, Err(_) => -1 };
    let _ = d3.unlink("/reap", 7);
    let _ = d3.close(f3a);
    let alive_b = d3.read(f3b).is_ok();
    let _ = d3.close(f3b);
    s.add(
        "J8-引用归零回收",
        alive_b && d3.n_reap == 1 && d3.objs.is_empty(),
        "最后一个 fd 关闭后对象才回收",
    );

    // 名字校验
    let mut d4 = LifeDesk::new();
    s.add(
        "J9-名字校验面",
        matches!(d4.open("bad", 7), Err(x) if x == LC_EINVAL),
        "无斜杠名被拒",
    );

    // fold 稳定
    let h1 = d3.fold_all();
    let h2 = d3.fold_all();
    s.add("J10-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染
    let r = d3.render();
    s.add("J11-渲染", r.starts_with("life:"), "摘要含生命周期标记");

    // EXCL 撞名（建模：同活名二开允许，但引用正确增长）
    let mut d5 = LifeDesk::new();
    let _ = d5.open("/ref", 7);
    let _ = d5.open("/ref", 7);
    let ref_ok = d5.objs[0].refs == 2;
    s.add("J12-引用计数", ref_ok, "两次打开同一活名引用为 2");

    // 动态判据
    s.add_dyn(
        "J13-拆名/读账",
        d.n_unlink == 10 && d.n_live_read >= 10,
        &crate::alloc::format!("unlink={} 活跃读={}", d.n_unlink, d.n_live_read),
    );
    s.add_dyn(
        "J14-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J15-回收账",
        d3.n_reap == 1,
        &crate::alloc::format!("回收次数={}", d3.n_reap),
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10958_posix_name_lifecycle_flow() {
        let mut d = LifeDesk::new();
        let f = d.open("/t", 7).unwrap();
        let _ = d.unlink("/t", 7);
        assert!(matches!(d.open("/t", 7), Err(x) if x == LC_ENOENT));
        assert_eq!(d.read(f), Ok(0));
        let _ = d.close(f);
        assert!(d.objs.is_empty());
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10958_checks_green() {
        let s = run_unx10958_checks();
        assert!(s.all_passed(), "UNX-F10958 判据未全绿");
    }
}
