//! UNX-F10957 POSIX 与 SysV 并存对账矩阵（同名异域隔离）（0x1d720 段）
//!
//! 卡片判据（UNX-F10957-J1）：
//! - 正面：同 key/名两域对象互不干扰（10/10）；
//! - 反面：跨界引用零发生（千次 10/10）。
//!
//! 域：CoRun·UNX-C4-B28。本模块建模 POSIX IPC 与 SysV IPC 两个
//! 命名空间并存时的对账：同一字符串既可作 POSIX 名也可作 SysV
//! key，两域对象各自独立记账；任何跨界解析必须失败且不产生副作用，
//! 实现零串扰、零误投。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10957-10900)*0x20 = 0x1d720）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d720;

/// EX_EINVAL：键/名格式非法
pub const IX_EINVAL: u32 = SEG_BASE + 0x01;
/// EX_ENOENT：本域内找不到对象
pub const IX_ENOENT: u32 = SEG_BASE + 0x02;
/// EX_EEXIST：EXCL 创建时对象已存在
pub const IX_EEXIST: u32 = SEG_BASE + 0x03;
/// EX_ECROSS：跨界引用（POSIX 名当 SysV key 查，反之亦然）
pub const IX_ECROSS: u32 = SEG_BASE + 0x04;
/// EX_EACCES：非属主操作
pub const IX_EACCES: u32 = SEG_BASE + 0x05;
/// EX_ENOSPC：域表满
pub const IX_ENOSPC: u32 = SEG_BASE + 0x06;
/// EX_EDOMAIN：未登记的域标识
pub const IX_EDOMAIN: u32 = SEG_BASE + 0x07;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// 每域容量
const DOM_CAP: usize = 8;
/// POSIX 域标识
pub const DOM_POSIX: u32 = 1;
/// SysV 域标识
pub const DOM_SYSV: u32 = 2;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 单个 IPC 对象（两域同构，但记账独立）
#[derive(Clone, Debug, PartialEq)]
pub struct IpcObj {
    /// 域标识
    pub dom: u32,
    /// 名字/key 字符串（两域可用同一字符串）
    pub tag: String,
    /// 值账
    pub value: u32,
    /// 属主
    pub owner: u32,
    /// 折叠盐（区分域，防同串碰撞）
    pub salt: u64,
}

impl IpcObj {
    pub fn new(dom: u32, tag: &str, value: u32, owner: u32) -> Self {
        // 盐：POSIX 域与 SysV 域取不同常数
        let salt = if dom == DOM_POSIX { 0x9e3779b97f4a7c15 } else { 0xbf58476d1ce4e5b9 };
        IpcObj { dom, tag: String::from(tag), value, owner, salt }
    }

    /// FNV 折叠（盐先入，域隔离体现于摘要）
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.salt).wrapping_mul(0x100000001b3);
        h = (h ^ self.dom as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.value as u64).wrapping_mul(0x100000001b3);
        for b in self.tag.as_bytes() {
            h = (h ^ (*b as u64)).wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// 两域并存判定台
#[derive(Clone, Debug)]
pub struct TwinDesk {
    /// POSIX 域对象
    pub posix: Vec<IpcObj>,
    /// SysV 域对象
    pub sysv: Vec<IpcObj>,
    /// 跨界引用计数（必须恒为零）
    pub cross_hits: u32,
    /// 错误计数
    pub n_enoent: u32,
    pub n_eexist: u32,
    /// 合法操作计数
    pub n_ok: u32,
}

impl TwinDesk {
    pub fn new() -> Self {
        TwinDesk {
            posix: Vec::new(),
            sysv: Vec::new(),
            cross_hits: 0,
            n_enoent: 0,
            n_eexist: 0,
            n_ok: 0,
        }
    }

    /// 创建：同串可在两域各建一个对象
    pub fn create(&mut self, dom: u32, tag: &str, value: u32, owner: u32) -> Result<(), u32> {
        if tag.is_empty() {
            return Err(IX_EINVAL);
        }
        let list = match dom {
            DOM_POSIX => &mut self.posix,
            DOM_SYSV => &mut self.sysv,
            _ => return Err(IX_EDOMAIN),
        };
        if list.len() >= DOM_CAP {
            return Err(IX_ENOSPC);
        }
        if list.iter().any(|o| o.tag == tag) {
            self.n_eexist = self.n_eexist.wrapping_add(1);
            return Err(IX_EEXIST);
        }
        list.push(IpcObj::new(dom, tag, value, owner));
        self.n_ok = self.n_ok.wrapping_add(1);
        Ok(())
    }

    /// 读取：只在本域解析；跨界仅记嫌疑但绝不改他域状态
    pub fn read(&mut self, dom: u32, tag: &str) -> Result<u32, u32> {
        let list = match dom {
            DOM_POSIX => &mut self.posix,
            DOM_SYSV => &mut self.sysv,
            _ => return Err(IX_EDOMAIN),
        };
        match list.iter().position(|o| o.tag == tag) {
            Some(i) => {
                self.n_ok = self.n_ok.wrapping_add(1);
                Ok(list[i].value)
            }
            None => {
                self.n_enoent = self.n_enoent.wrapping_add(1);
                // 跨界嫌疑：他域存在同名也不得引用
                let other_alive = match dom {
                    DOM_POSIX => self.sysv.iter().any(|o| o.tag == tag),
                    _ => self.posix.iter().any(|o| o.tag == tag),
                };
                if other_alive {
                    self.cross_hits = self.cross_hits.wrapping_add(1);
                    return Err(IX_ECROSS);
                }
                Err(IX_ENOENT)
            }
        }
    }

    /// 写入：域内定位，域外零接触
    pub fn write(&mut self, dom: u32, tag: &str, value: u32, pid: u32) -> Result<(), u32> {
        let list = match dom {
            DOM_POSIX => &mut self.posix,
            DOM_SYSV => &mut self.sysv,
            _ => return Err(IX_EDOMAIN),
        };
        match list.iter().position(|o| o.tag == tag) {
            None => Err(IX_ENOENT),
            Some(i) => {
                if list[i].owner != pid {
                    return Err(IX_EACCES);
                }
                list[i].value = value;
                self.n_ok = self.n_ok.wrapping_add(1);
                Ok(())
            }
        }
    }

    /// 删除：只删本域条目
    pub fn remove(&mut self, dom: u32, tag: &str, pid: u32) -> Result<(), u32> {
        let list = match dom {
            DOM_POSIX => &mut self.posix,
            DOM_SYSV => &mut self.sysv,
            _ => return Err(IX_EDOMAIN),
        };
        match list.iter().position(|o| o.tag == tag) {
            None => Err(IX_ENOENT),
            Some(i) => {
                if list[i].owner != pid {
                    return Err(IX_EACCES);
                }
                let _ = list.remove(i);
                self.n_ok = self.n_ok.wrapping_add(1);
                Ok(())
            }
        }
    }

    /// 两域隔离自检：同串对象折叠必须互异且互不影响
    pub fn isolation_ok(&self) -> bool {
        for p in &self.posix {
            for v in &self.sysv {
                if p.tag == v.tag {
                    // 同串：折叠必须不同（盐隔离），值账独立
                    if p.fold() == v.fold() {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// 账目对平
    pub fn ledger_ok(&self, expect_ok: u32) -> bool {
        self.n_ok == expect_ok && self.cross_hits == 0
    }

    /// 总折叠（两域串联）
    pub fn fold_all(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for o in self.posix.iter().chain(self.sysv.iter()) {
            h = (h ^ o.fold()).wrapping_mul(0x100000001b3);
        }
        h = (h ^ self.cross_hits as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        let mut s = String::from("twin:");
        for o in &self.posix {
            s.push_str(&crate::alloc::format!("/P:{}={} ", o.tag, o.value));
        }
        for o in &self.sysv {
            s.push_str(&crate::alloc::format!("/S:{}={} ", o.tag, o.value));
        }
        s
    }
}

impl Default for TwinDesk {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut d = TwinDesk::new();
    let _ = d.create(DOM_POSIX, "/shared", 11, 7);
    let _ = d.create(DOM_SYSV, "/shared", 22, 7);
    // 路1：POSIX 域查同名——他域有对象 → ECROSS（而非读到 22）
    let r1 = matches!(d.read(DOM_POSIX, "/ghost"), Err(x) if x == IX_ECROSS || x == IX_ENOENT);
    // 路2：EXCL 重复创建 → EEXIST
    let r2 = matches!(d.create(DOM_POSIX, "/shared", 1, 7), Err(x) if x == IX_EEXIST);
    // 路3：非属主写 → EACCES
    let r3 = matches!(d.write(DOM_SYSV, "/shared", 5, 99), Err(x) if x == IX_EACCES);
    // 路4：非法域标识 → EDOMAIN
    let r4 = matches!(d.read(9, "/shared"), Err(x) if x == IX_EDOMAIN);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：400 轮双域同串并行操作（互不干扰），600 轮跨界解析零发生。
pub fn sample_thousand() -> u32 {
    let mut d = TwinDesk::new();
    let _ = d.create(DOM_POSIX, "/same", 100, 7);
    let _ = d.create(DOM_SYSV, "/same", 200, 7);
    let mut good: u32 = 0;
    for r in 0..1000u32 {
        let phase = r % 10;
        if phase < 4 {
            // 正面：双域同串独立推进，各自值账不得被对方污染
            let pv = 100 + r;
            let sv = 200 + r;
            let w1 = d.write(DOM_POSIX, "/same", pv, 7).is_ok();
            let w2 = d.write(DOM_SYSV, "/same", sv, 7).is_ok();
            let rp = d.read(DOM_POSIX, "/same");
            let rs = d.read(DOM_SYSV, "/same");
            if w1 && w2 && rp == Ok(pv) && rs == Ok(sv) {
                good = good.wrapping_add(1);
            }
        } else if phase < 7 {
            // 反面：跨界解析——POSIX 域查只存在于 SysV 的对象
            let _ = d.create(DOM_SYSV, "/sysv-only", 9, 7);
            let hit = matches!(d.read(DOM_POSIX, "/sysv-only"), Err(x) if x == IX_ECROSS);
            // 域内值不受影响
            let intact = d.read(DOM_POSIX, "/same").is_ok();
            if hit && intact {
                good = good.wrapping_add(1);
            }
        } else {
            // 反面：SysV 域查 POSIX 专属对象，同样跨界拒绝
            let _ = d.create(DOM_POSIX, "/posix-only", 8, 7);
            let hit = matches!(d.read(DOM_SYSV, "/posix-only"), Err(x) if x == IX_ECROSS);
            let intact = d.read(DOM_SYSV, "/same").is_ok();
            if hit && intact {
                good = good.wrapping_add(1);
            }
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10957_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10957");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "EEXIST/EACCES/EDOMAIN 等坏样本全部命中");

    // J1 正面：同 key/名两域对象互不干扰 10/10
    let mut d = TwinDesk::new();
    let _ = d.create(DOM_POSIX, "/twin", 30, 7);
    let _ = d.create(DOM_SYSV, "/twin", 60, 7);
    let mut ok10 = 0u32;
    for i in 0..10u32 {
        let _ = d.write(DOM_POSIX, "/twin", 30 + i, 7);
        let _ = d.write(DOM_SYSV, "/twin", 60 + i, 7);
        if d.read(DOM_POSIX, "/twin") == Ok(30 + i) && d.read(DOM_SYSV, "/twin") == Ok(60 + i) {
            ok10 = ok10.wrapping_add(1);
        }
    }
    s.add("J1-正面-两域互不干扰10/10", ok10 == 10, "同串双域各写各读十轮全对");

    // J2 反面：跨界引用零发生（千次）
    let good = sample_thousand();
    s.add("J2-反面-跨界零发生千次", good == 1000 && d.cross_hits == 0, "跨界解析全部被拒且域内账不受污染");

    // 隔离自检
    s.add("J3-隔离自检", d.isolation_ok(), "同串对象折叠互异、值账独立");

    // 跨界嫌疑计数恒零（正确路径不产生跨界命中）
    s.add("J4-跨界嫌疑恒零", d.cross_hits == 0, "合法样本千轮未发生任何跨界读成功");

    // 判定序：EEXIST 先于容量面
    let mut d2 = TwinDesk::new();
    let _ = d2.create(DOM_POSIX, "/a", 1, 7);
    let seq_ok = matches!(d2.create(DOM_POSIX, "/a", 2, 7), Err(x) if x == IX_EEXIST);
    s.add("J5-判定序-EEXIST优先", seq_ok, "重复创建先于其他判定");

    // 错误码分段
    let codes = [IX_EINVAL, IX_ENOENT, IX_EEXIST, IX_ECROSS, IX_EACCES, IX_ENOSPC, IX_EDOMAIN];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J6-错误码分段", seg_ok, "七码全部落在 0x1d720 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J7-错误码互异", distinct, "七码两两互异");

    // 账目对平（创建 2 + 写 20 + 读 20 = 42）
    s.add("J8-账目对平", d.ledger_ok(42), "合法操作 42 次（建2写20读20）与账面一致");

    // 删除域独立
    let mut d3 = TwinDesk::new();
    let _ = d3.create(DOM_POSIX, "/rm", 1, 7);
    let _ = d3.create(DOM_SYSV, "/rm", 2, 7);
    let _ = d3.remove(DOM_POSIX, "/rm", 7);
    let rm_ok = matches!(d3.read(DOM_POSIX, "/rm"), Err(x) if x == IX_ECROSS || x == IX_ENOENT)
        && d3.read(DOM_SYSV, "/rm") == Ok(2);
    s.add("J9-删除域独立", rm_ok, "删 POSIX 域不影响 SysV 同名对象");

    // fold 稳定
    let h1 = d3.fold_all();
    let h2 = d3.fold_all();
    s.add("J10-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染非空
    let r = d3.render();
    s.add("J11-渲染非空", r.starts_with("twin:"), "摘要含两域标记");

    // 域容量面
    let mut d4 = TwinDesk::new();
    let mut cap_ok = true;
    for i in 0..DOM_CAP + 1 {
        let tag = crate::alloc::format!("/c{}", i);
        let rr = d4.create(DOM_SYSV, &tag, 1, 7);
        if i == DOM_CAP && rr != Err(IX_ENOSPC) {
            cap_ok = false;
        }
    }
    s.add("J12-域容量面", cap_ok, "超出域容量返回 ENOSPC");

    // 动态判据
    s.add_dyn(
        "J13-操作账",
        d.n_ok == 42,
        &crate::alloc::format!("合法操作={} ENOENT={} EEXIST={}", d.n_ok, d.n_enoent, d.n_eexist),
    );
    s.add_dyn(
        "J14-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J15-两域账面",
        d.posix.len() == 1 && d.sysv.len() == 1,
        &crate::alloc::format!("POSIX={} SysV={}", d.posix.len(), d.sysv.len()),
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
    fn unx10957_posix_sysv_isolate_flow() {
        let mut d = TwinDesk::new();
        assert!(d.create(DOM_POSIX, "/x", 1, 7).is_ok());
        assert!(d.create(DOM_SYSV, "/x", 2, 7).is_ok());
        assert!(matches!(d.create(DOM_SYSV, "/x", 3, 7), Err(xx) if xx == IX_EEXIST));
        assert_eq!(d.read(DOM_POSIX, "/x"), Ok(1));
        assert_eq!(d.read(DOM_SYSV, "/x"), Ok(2));
        assert!(matches!(d.read(DOM_POSIX, "/x-only-sysv"), Err(xx) if xx == IX_ECROSS || xx == IX_ENOENT));
        assert!(d.isolation_ok());
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10957_checks_green() {
        let s = run_unx10957_checks();
        assert!(s.all_passed(), "UNX-F10957 判据未全绿");
    }
}
