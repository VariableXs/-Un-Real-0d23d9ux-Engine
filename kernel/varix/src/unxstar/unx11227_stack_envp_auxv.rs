//! UNX-F11227 环境块与 argv/auxv 读取消费（0x1f827 段）
//!
//! 锚点：UNX-F11227-J1 栈布局（argc/argv/envp/auxv 四区）偏移语义判据绿，
//! glibc 遍历假设零违反。
//! 域：CoRun·UNX-C5-B02——四区自栈顶向下严格相邻：argc 一字、argv 含 NULL
//! 尾、envp 含 NULL 尾、auxv 以 AT_NULL(0) 双字收尾；越界或错位即违例。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f827;
pub const SE_ARGV: u32 = SEG_BASE + 0x01; // argv 指针越区
pub const SE_ENVP: u32 = SEG_BASE + 0x02; // envp 指针越区
pub const SE_AUX: u32 = SEG_BASE + 0x03; // auxv 项越区
pub const SE_ALIGN: u32 = SEG_BASE + 0x04; // 未八字对齐
pub const SE_NULL: u32 = SEG_BASE + 0x05; // 缺 NULL 尾
pub const SE_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const SE_OVER: u32 = SEG_BASE + 0x07; // 超容

/// 栈区四段与字宽。
pub const WORD: u64 = 8;
pub const AUX_PAIR: u64 = 16;
pub const MAX_PTRS: u32 = 32;
pub const SE_TAG: u64 = 0x1f827e7;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一段栈区的偏移账。
#[derive(Clone, Debug)]
pub struct Region {
    pub name: &'static str,
    pub off: u64,
    pub bytes: u64,
}

impl Region {
    pub fn aligned(&self) -> bool {
        self.off % WORD == 0 && self.bytes % WORD == 0
    }

    pub fn end(&self) -> u64 {
        self.off.saturating_add(self.bytes)
    }
}

/// 栈布局账。
#[derive(Clone, Debug)]
pub struct StackLayout {
    pub base: u64,
    pub argc: u32,
    pub envn: u32,
    pub auxn: u32,
    pub regions: Vec<Region>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl StackLayout {
    pub fn new(base: u64, argc: u32, envn: u32, auxn: u32) -> StackLayout {
        StackLayout { base, argc, envn, auxn, regions: Vec::new(), frozen: false, root: None }
    }

    /// 按 argc/argv/envp/auxv 顺序排四区。
    pub fn arrange(&mut self) -> Result<(), u32> {
        if self.frozen {
            return Err(SE_FROZEN);
        }
        if self.argc == 0 || self.argc > MAX_PTRS || self.envn > MAX_PTRS || self.auxn == 0 || self.auxn > MAX_PTRS {
            return Err(SE_OVER);
        }
        if self.base % WORD != 0 {
            return Err(SE_ALIGN);
        }
        self.regions.clear();
        let a_off = 0u64;
        let a_bytes = WORD;
        let v_off = a_off + a_bytes;
        let v_bytes = WORD * (self.argc as u64 + 1);
        let e_off = v_off + v_bytes;
        let e_bytes = WORD * (self.envn as u64 + 1);
        let x_off = e_off + e_bytes;
        let x_bytes = AUX_PAIR * (self.auxn as u64 + 1);
        self.regions.push(Region { name: "argc", off: a_off, bytes: a_bytes });
        self.regions.push(Region { name: "argv", off: v_off, bytes: v_bytes });
        self.regions.push(Region { name: "envp", off: e_off, bytes: e_bytes });
        self.regions.push(Region { name: "auxv", off: x_off, bytes: x_bytes });
        Ok(())
    }

    pub fn argv_off(&self, i: u32) -> Result<u64, u32> {
        if i > self.argc {
            return Err(SE_ARGV);
        }
        let r = self.regions.get(1).ok_or(SE_ARGV)?;
        Ok(r.off + WORD * i as u64)
    }

    pub fn envp_off(&self, i: u32) -> Result<u64, u32> {
        if i > self.envn {
            return Err(SE_ENVP);
        }
        let r = self.regions.get(2).ok_or(SE_ENVP)?;
        Ok(r.off + WORD * i as u64)
    }

    pub fn aux_off(&self, i: u32) -> Result<u64, u32> {
        if i > self.auxn {
            return Err(SE_AUX);
        }
        let r = self.regions.get(3).ok_or(SE_AUX)?;
        Ok(r.off + AUX_PAIR * i as u64)
    }

    /// 四区首尾相接、各区内对齐。
    pub fn contiguous(&self) -> bool {
        if self.regions.len() != 4 {
            return false;
        }
        let mut k = 0usize;
        let mut at = 0u64;
        while k < self.regions.len() {
            if self.regions[k].off != at || !self.regions[k].aligned() {
                return false;
            }
            at = self.regions[k].end();
            k += 1;
        }
        true
    }

    /// NULL 尾与 AT_NULL 收尾。
    pub fn terminated(&self) -> bool {
        let mut ok = true;
        let a = self.argv_off(self.argc);
        if a.is_err() {
            ok = false;
        }
        let e = self.envp_off(self.envn);
        if e.is_err() {
            ok = false;
        }
        if let (Ok(ao), Ok(eo), Ok(xo)) = (a, e, self.aux_off(self.auxn)) {
            let v = self.regions.get(1);
            let w = self.regions.get(2);
            let z = self.regions.get(3);
            if let (Some(v), Some(w), Some(z)) = (v, w, z) {
                if ao < v.off || ao + WORD > v.end() || eo < w.off || eo + WORD > w.end() || xo + AUX_PAIR > z.end() {
                    ok = false;
                }
            }
        }
        ok
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(SE_FROZEN);
        }
        if !self.contiguous() {
            return Err(SE_ALIGN);
        }
        if !self.terminated() {
            return Err(SE_NULL);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("stack 0x{:x} argc {} env {} aux {}", self.base, self.argc, self.envn, self.auxn)
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = SE_TAG;
        h = (h ^ self.base).wrapping_mul(FP_STEP);
        h = (h ^ self.argc as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.envn as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.auxn as u64).wrapping_mul(FP_STEP);
        let mut k = 0usize;
        while k < self.regions.len() {
            let b = self.regions[k].name.as_bytes();
            let mut i = 0usize;
            while i < b.len() {
                h = (h ^ b[i] as u64).wrapping_mul(FP_STEP);
                i += 1;
            }
            h = (h ^ self.regions[k].off).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }
}

/// 典型栈顶与参数规模。
pub const STACK_TOP: u64 = 0x7ffd_0000_0000;

/// 组一张 argc=3/env=2/aux=5 的布局账。
pub fn build() -> StackLayout {
    let mut l = StackLayout::new(STACK_TOP, 3, 2, 5);
    let _ = l.arrange();
    l
}

/// 四路对抗：零 argc、错位基址、argv 越界、未排布即冻存。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = StackLayout::new(STACK_TOP, 0, 1, 1);
    out[0] = a.arrange() == Err(SE_OVER);
    let mut b = StackLayout::new(STACK_TOP + 4, 1, 1, 1);
    out[1] = b.arrange() == Err(SE_ALIGN);
    let mut c = build();
    out[2] = c.argv_off(4) == Err(SE_ARGV);
    let mut d = StackLayout::new(STACK_TOP, 1, 1, 1);
    out[3] = d.freeze() == Err(SE_ALIGN);
    out
}

/// 判据入口。
pub fn run_unx11227_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11227");
    let mut l = build();
    let root = l.freeze().unwrap_or(0);

    s.add("J1-四区齐", l.regions.len() == 4, "argc/argv/envp/auxv 四区各一段");
    s.add("J2-区首尾相接", l.contiguous(), "四区偏移逐段相接且各区内对齐");
    s.add("J3-字对齐", l.regions.iter().all(|r| r.aligned()), "每区偏移与长度均为八字倍数");
    s.add("J4-argv 尾", l.argv_off(3).is_ok() && l.argv_off(4).is_err(), "argv 指针可取到 NULL 尾、越界即拒");
    s.add("J5-envp 尾", l.envp_off(2).is_ok() && l.envp_off(3).is_err(), "envp 指针可取到 NULL 尾、越界即拒");
    s.add("J6-auxv 收尾", l.aux_off(5).is_ok() && l.aux_off(6).is_err(), "auxv 可取到 AT_NULL 项、越界即拒");
    s.add("J7-终止自证", l.terminated(), "NULL 尾与 AT_NULL 项均落在各自区内");
    s.add("J8-偏移单调", {
        let mut k = 1usize;
        let mut ok = true;
        while k < l.regions.len() {
            if l.regions[k].off <= l.regions[k - 1].off {
                ok = false;
            }
            k += 1;
        }
        ok
    }, "四区偏移自栈顶向下严格递增");
    s.add("J9-冻存根", root != 0 && l.root == Some(root), "布局账冻存产出非零根");

    s.add("J10-冻后排拒", l.arrange() == Err(SE_FROZEN), "冻存后重排拒为 SE_FROZEN");
    s.add("J11-零参拒", {
        let mut x = StackLayout::new(STACK_TOP, 0, 1, 1);
        x.arrange() == Err(SE_OVER)
    }, "argc 为零拒为 SE_OVER");
    s.add("J12-错位拒", {
        let mut x = StackLayout::new(STACK_TOP + 2, 1, 1, 1);
        x.arrange() == Err(SE_ALIGN)
    }, "基址非八字对齐拒为 SE_ALIGN");

    let four = inject_four();
    s.add("J13-四路注入", four.iter().all(|x| *x), "零参/错位/argv 越界/未排布四路全拒");
    s.add_dyn(
        "J14-区明细",
        l.regions.len() == 4 && l.regions[3].end() == 8 + 32 + 24 + 96,
        &format!("argc@{} argv@{} envp@{} auxv@{}，总 {} 字节", l.regions[0].off, l.regions[1].off, l.regions[2].off, l.regions[3].off, l.regions[3].end()),
    );
    s.add_dyn(
        "J15-账根",
        root == l.fold(),
        &format!("根 0x{:x}，aux 尾偏移 {}", root, l.aux_off(5).unwrap_or(0)),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11227_stack_envp_flow() {
        let mut l = build();
        assert!(l.contiguous());
        assert!(l.terminated());
        let r = l.freeze().unwrap_or(0);
        assert!(r != 0);
        assert_eq!(l.argv_off(4), Err(SE_ARGV));
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx11227_checks_green() {
        let s = run_unx11227_checks();
        assert!(s.all_passed(), "UNX-F11227 判据未全绿");
    }
}
