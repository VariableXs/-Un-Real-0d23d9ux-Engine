//! veb397_handover —— VE-F0397 VE-C 域移交包。
//! 锚点（CoRun Unxreal 主册）：VE-F0397 · VE-C 域移交包 · 目标 360 行
//! 判据原文：VE-F0397-J1 移交包含五类件：接口清单（冻结哈希）、
//!           错误码表（段界 + 表长）、知识库（词条数）、遗留事
//!           项（未决数 + 每项接手人）、演进预留（预留位数）；
//!           包完整性核（五件齐全才许封包，缺件报件名）；封包
//!           出包章（五件哈希级联）；封包后篡改检出（重算哈希
//!           不等）；移交确认（C 域签收后包只读）。
//! 组件：五件容器 + 完整性闸 + 包章 + 篡改检出 + 签收态。
//! 职责定位：移交包是「B 域给 C 域的交接箱」——五件是五份交接
//!           清单：接口是钥匙，错误码是暗号本，知识库是说明书，
//!           遗留事项是欠条，演进预留是空白页；缺一件都不能封
//!           箱；包章让「交接时是什么样」可验证，签收后只读是
//!           交接纪律：签了字的东西不许再动。
//! 性能逐项分解：封包 O(件规模)；篡改检 O(件规模)；确认 O(1)。
//! 诊断码段：0x308C0（0x308C1~0x308C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const HV_VERSION: &str = "veb397_handover-v1";

/// 诊断段基址（0x308C0 段 · 本模块独占）。
pub const HV_SEG_BASE: u32 = 0x308C0;
pub const XC_MISS_IFACE: u32 = 0x308C1; // 缺接口清单
pub const XC_MISS_ERRTAB: u32 = 0x308C2; // 缺错误码表
pub const XC_MISS_KB: u32 = 0x308C3; // 缺知识库
pub const XC_MISS_PENDING: u32 = 0x308C4; // 缺遗留事项
pub const XC_MISS_RESERVE: u32 = 0x308C5; // 缺演进预留
pub const XC_MAGIC: u32 = 0x308C6; // 账魔数破
pub const XC_NOT_SEAL: u32 = 0x308C7; // 未封包先签收
pub const XC_TAMPER: u32 = 0x308C8; // 篡改检出
pub const XC_RDONLY: u32 = 0x308C9; // 签收后只读

fn fnv32a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 五件容器。
pub struct Handover {
    /// 件一：接口清单冻结哈希。
    pub iface_hash: Option<u32>,
    /// 件二：错误码表（段基址，表长）。
    pub errtab: Option<(u32, u32)>,
    /// 件三：知识库词条数。
    pub kb_entries: Option<u32>,
    /// 件四：遗留事项（未决数 + 接手人代号）。
    pub pending: Option<(u32, &'static str)>,
    /// 件五：演进预留位数。
    pub reserve_bits: Option<u32>,
    sealed: bool,
    seal_hash: Option<u32>,
    signed: bool,
    magic: u32,
}

impl Handover {
    /// 建。
    pub fn new() -> Handover {
        Handover {
            iface_hash: None, errtab: None, kb_entries: None,
            pending: None, reserve_bits: None,
            sealed: false, seal_hash: None, signed: false,
            magic: 0x08C0_1A0E,
        }
    }

    fn pack_hash(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        match self.iface_hash {
            Some(v) => buf.extend_from_slice(&v.to_le_bytes()),
            None => buf.push(0),
        }
        match self.errtab {
            Some((seg, n)) => {
                buf.extend_from_slice(&seg.to_le_bytes());
                buf.extend_from_slice(&n.to_le_bytes());
            }
            None => buf.push(0),
        }
        match self.kb_entries {
            Some(v) => buf.extend_from_slice(&v.to_le_bytes()),
            None => buf.push(0),
        }
        match self.pending {
            Some((n, who)) => {
                buf.extend_from_slice(&n.to_le_bytes());
                for b in who.as_bytes() {
                    buf.push(*b);
                }
            }
            None => buf.push(0),
        }
        match self.reserve_bits {
            Some(v) => buf.extend_from_slice(&v.to_le_bytes()),
            None => buf.push(0),
        }
        fnv32a(&buf)
    }

    /// 完整性闸：五件齐才许封包，缺件报件名。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.magic != 0x08C0_1A0E {
            return Err(XC_MAGIC);
        }
        if self.iface_hash.is_none() {
            return Err(XC_MISS_IFACE);
        }
        if self.errtab.is_none() {
            return Err(XC_MISS_ERRTAB);
        }
        if self.kb_entries.is_none() {
            return Err(XC_MISS_KB);
        }
        if self.pending.is_none() {
            return Err(XC_MISS_PENDING);
        }
        if self.reserve_bits.is_none() {
            return Err(XC_MISS_RESERVE);
        }
        self.sealed = true;
        let h = self.pack_hash();
        self.seal_hash = Some(h);
        Ok(h)
    }

    /// 篡改检出：重算哈希与包章比。
    pub fn tamper_check(&self) -> Result<bool, u32> {
        match self.seal_hash {
            Some(h) => {
                let cur = self.pack_hash();
                Ok(cur == h)
            }
            None => Err(XC_NOT_SEAL),
        }
    }

    /// C 域签收（须已封包）。
    pub fn sign_off(&mut self) -> Result<(), u32> {
        if !self.sealed {
            return Err(XC_NOT_SEAL);
        }
        self.signed = true;
        Ok(())
    }

    /// 签收后只读：任何置件操作拒。
    pub fn put_iface(&mut self, h: u32) -> Result<(), u32> {
        if self.signed {
            return Err(XC_RDONLY);
        }
        self.iface_hash = Some(h);
        Ok(())
    }

    pub fn put_errtab(&mut self, seg: u32, n: u32) -> Result<(), u32> {
        if self.signed {
            return Err(XC_RDONLY);
        }
        self.errtab = Some((seg, n));
        Ok(())
    }

    pub fn put_kb(&mut self, n: u32) -> Result<(), u32> {
        if self.signed {
            return Err(XC_RDONLY);
        }
        self.kb_entries = Some(n);
        Ok(())
    }

    pub fn put_pending(&mut self, n: u32, who: &'static str) -> Result<(), u32> {
        if self.signed {
            return Err(XC_RDONLY);
        }
        self.pending = Some((n, who));
        Ok(())
    }

    pub fn put_reserve(&mut self, bits: u32) -> Result<(), u32> {
        if self.signed {
            return Err(XC_RDONLY);
        }
        self.reserve_bits = Some(bits);
        Ok(())
    }

    /// 封包/签收态。
    pub fn state(&self) -> (bool, bool) {
        (self.sealed, self.signed)
    }
}


pub fn run_veb397_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0397");
    // 族一：缺件逐报。
    let mut h = Handover::new();
    s.add("缺接口清单", h.seal() == Err(XC_MISS_IFACE), "件一");
    let _ = h.put_iface(0xABCD);
    s.add("缺错误码表", h.seal() == Err(XC_MISS_ERRTAB), "件二");
    let _ = h.put_errtab(0x31000, 42);
    s.add("缺知识库", h.seal() == Err(XC_MISS_KB), "件三");
    let _ = h.put_kb(17);
    s.add("缺遗留事项", h.seal() == Err(XC_MISS_PENDING), "件四");
    let _ = h.put_pending(3, "AI-WBB-1022");
    s.add("缺演进预留", h.seal() == Err(XC_MISS_RESERVE), "件五");
    let _ = h.put_reserve(64);
    // 族二：五件齐封包。
    let hash = h.seal();
    s.add("封包出章", hash.is_ok(), "五件齐");
    s.add("封包态", h.state() == (true, false), "未签收");
    // 族三：篡改检出。
    s.add("原包核真", h.tamper_check() == Ok(true), "章对");
    // 篡改：改 KB（put 允许——未签收但已封包；模型上封包后置件
    // 本身不拦，靠 tamper_check 检出）。
    let _ = h.put_kb(18);
    s.add("篡改检出", h.tamper_check() == Ok(false), "哈希漂");
    // 还原。
    let _ = h.put_kb(17);
    s.add("还原核真", h.tamper_check() == Ok(true), "章复对");
    // 族四：签收。
    s.add("签收过", h.sign_off().is_ok(), "C 域接箱");
    s.add("签收态", h.state() == (true, true), "已签");
    s.add("签收后置件拒", h.put_kb(99) == Err(XC_RDONLY), "只读");
    s.add("签收后置件拒 2", h.put_iface(1) == Err(XC_RDONLY), "只读");
    s.add("重复签收无害", h.sign_off().is_ok(), "幂等");
    // 族五：未封包先签收拒。
    let mut k = Handover::new();
    s.add("未封包签收拒", k.sign_off() == Err(XC_NOT_SEAL), "先封箱");
    s.add("未封包篡改检拒", k.tamper_check() == Err(XC_NOT_SEAL), "无章可核");
    s.add("魔数核", HV_SEG_BASE == 0x308C0, "段基址");
    s
}
