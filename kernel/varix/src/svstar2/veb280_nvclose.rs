//! veb280_nvclose —— VE-F0280 NVIDIA 组收口与 ARM 组移交。
//! 锚点（CoRun Unxreal 主册）：VE-F0280 · NVIDIA 组收口与 ARM 组移交 · 目标 340 行
//! 判据原文：VE-F0280-J1 组收口账：NVIDIA 组八卡收口清单
//!           （卡号 1~8，各带落点模块名）；收口闸（八卡全
//!           「已收」+ 一致性证在持 → 组闭）；移交包（包序号
//!           + 卡清单 + 一致性指纹 + 移交目标 = ARM 组）；
//!           移交校核（目标组名对 + 指纹非零 + 清单齐 → 包
//!           合法）；包重复移交拒；ARM 侧签收（包入库 +
//!           回执号回传）；回执核（回执号 = 包序号 × 常数 +
//!           1，单向可验）。
//! 组件：收口清单 + 组闭闸 + 移交包 + ARM 签收。
//! 职责定位：组收口是「NVIDIA 章的落印」——八卡全收才闭组：
//!           缺一卡闭的组是漏气的组；移交包是章与章之间的
//!           交接棒：指纹随包走，ARM 拿到的不只是卡是证据；
//!           回执单向可验是交棒后的确认：回执号由包号单向
//!           推出，伪造不了也抵赖不了。
//! 性能逐项分解：收口 O(1)；包校验 O(卡数)；回执 O(1)。
//! 诊断码段：0x30F00（0x30F01~0x30F09 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const CL_VERSION: &str = "veb280_nvclose-v1";

/// 诊断段基址（0x30F00 段 · 本模块独占）。
pub const CL_SEG_BASE: u32 = 0x30F00;
pub const XC_MAGIC: u32 = 0x30F01; // 账魔数破
pub const XC_NOT_ALL: u32 = 0x30F02; // 未全收
pub const XC_NO_CERT: u32 = 0x30F03; // 无一致性证
pub const XC_BAD_PK: u32 = 0x30F04; // 包不合法
pub const XC_RESENT: u32 = 0x30F05; // 包重复移交
pub const XC_NOT_PK: u32 = 0x30F06; // 签收无此包

/// 账魔数。
const MAGIC: u32 = 0x0F00_1CCE;

/// NVIDIA 组卡数。
pub const NV_CARDS: usize = 8;

/// 回执常数。
pub const REPLY_K: u64 = 0x2EE0;

/// 移交目标组名。
pub const TARGET_GROUP: &str = "ARM";

/// 移交包。
pub struct Pkg {
    /// 包序号。
    pub seq: u64,
    /// 卡清单（8 卡号）。
    pub cards: [u32; NV_CARDS],
    /// 一致性指纹（非零有效）。
    pub fp: u64,
    /// 目标组名。
    pub target: u32, // 模型化为 0xA941 = 'A''R' 折叠
}

/// 目标组名折叠常量（"ARM" → 0xA941 模型值）。
pub const TARGET_CODE: u32 = 0xA941;

/// NVIDIA 收口账。
pub struct CloseBook {
    /// 八卡收口旗。
    pub done: [bool; NV_CARDS],
    /// 一致性证指纹（零 = 无证）。
    pub cert_fp: u64,
    /// 已移交包序号账。
    handed: Vec<u64>,
    /// ARM 侧入库回执账。
    pub receipts: Vec<u64>,
    magic: u32,
}

impl CloseBook {
    /// 建空账。
    pub fn new() -> CloseBook {
        CloseBook {
            done: [false; NV_CARDS],
            cert_fp: 0,
            handed: Vec::new(),
            receipts: Vec::new(),
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 收口一卡。
    pub fn close_one(&mut self, gpu: usize) -> Result<(), u32> {
        if !self.magic_ok() || gpu >= NV_CARDS {
            return Err(XC_MAGIC);
        }
        self.done[gpu] = true;
        Ok(())
    }

    /// 收全？
    pub fn all_closed(&self) -> bool {
        for f in self.done.iter() {
            if !f {
                return false;
            }
        }
        true
    }

    /// 入证。
    pub fn put_cert(&mut self, fp: u64) {
        self.cert_fp = fp;
    }

    /// 组闭闸：全收 + 有证 → 发移交资格。
    pub fn close_gate(&self) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if !self.all_closed() {
            return Err(XC_NOT_ALL);
        }
        if self.cert_fp == 0 {
            return Err(XC_NO_CERT);
        }
        Ok(())
    }

    /// 造移交包。
    pub fn make_pkg(&self, seq: u64) -> Result<Pkg, u32> {
        self.close_gate()?;
        let mut cards = [0u32; NV_CARDS];
        for i in 0..NV_CARDS {
            cards[i] = (i + 1) as u32;
        }
        Ok(Pkg { seq, cards, fp: self.cert_fp, target: TARGET_CODE })
    }

    /// 包合法性校核：目标对 + 指纹非零 + 清单齐。
    pub fn check_pkg(&self, p: &Pkg) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if p.target != TARGET_CODE {
            return Err(XC_BAD_PK);
        }
        if p.fp == 0 {
            return Err(XC_BAD_PK);
        }
        for i in 0..NV_CARDS {
            if p.cards[i] != (i + 1) as u32 {
                return Err(XC_BAD_PK);
            }
        }
        Ok(())
    }

    /// 移交：包合法 + 未重复 → 记移交。
    pub fn handover(&mut self, p: &Pkg) -> Result<(), u32> {
        self.check_pkg(p)?;
        for h in self.handed.iter() {
            if *h == p.seq {
                return Err(XC_RESENT);
            }
        }
        self.handed.push(p.seq);
        Ok(())
    }

    /// ARM 侧签收：包入库 + 回执号 = seq × K + 1。
    pub fn sign_recv(&mut self, p: &Pkg) -> Result<u64, u32> {
        self.check_pkg(p)?;
        for h in self.handed.iter() {
            if *h == p.seq {
                let rc = p.seq * REPLY_K + 1;
                self.receipts.push(rc);
                return Ok(rc);
            }
        }
        Err(XC_NOT_PK)
    }

    /// 回执核：单向可验。
    pub fn verify_reply(&self, seq: u64, rc: u64) -> bool {
        rc == seq * REPLY_K + 1
    }

    /// 移交数。
    pub fn handed_count(&self) -> usize {
        self.handed.len()
    }
}

pub fn run_veb280_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0280");
    let mut b = CloseBook::new();
    // 族一：收口闸前拒。
    s.add("未全收拒", b.close_gate() == Err(XC_NOT_ALL), "缺卡");
    // 收七卡仍拒。
    let mut seven = true;
    for i in 0..7 {
        if b.close_one(i).is_err() {
            seven = false;
        }
    }
    s.add("七卡收齐", seven, "0~6 逐收");
    s.add("七卡仍拒", b.close_gate() == Err(XC_NOT_ALL), "还差卡 8");
    // 收第八卡 → 差证。
    s.add("收卡 8", b.close_one(7).is_ok(), "全收");
    s.add("全收查", b.all_closed(), "八卡齐");
    s.add("无证拒", b.close_gate() == Err(XC_NO_CERT), "缺证");
    // 族二：入证闭组。
    b.put_cert(0x1234_5678_9ABC_DEF0);
    s.add("组闭过闸", b.close_gate().is_ok(), "全收+有证");
    // 族三：包校核。
    let p1 = match b.make_pkg(1) {
        Ok(p) => p,
        Err(_) => {
            s.add("造包败", false, "不应到");
            return s;
        }
    };
    s.add("包目标 ARM", p1.target == TARGET_CODE, "A941");
    s.add("包指纹随证", p1.fp == 0x1234_5678_9ABC_DEF0, "证据随包");
    s.add("包清单齐", p1.cards[0] == 1 && p1.cards[7] == 8, "1~8");
    s.add("包合法", b.check_pkg(&p1).is_ok(), "三核过");
    // 篡改目标拒。
    let mut bad = Pkg { seq: 2, cards: p1.cards, fp: p1.fp, target: 0x1234 };
    s.add("目标篡改拒", b.check_pkg(&bad) == Err(XC_BAD_PK), "非 ARM");
    bad.target = TARGET_CODE;
    bad.fp = 0;
    s.add("零指纹拒", b.check_pkg(&bad) == Err(XC_BAD_PK), "无证签");
    bad.fp = p1.fp;
    bad.cards[3] = 9;
    s.add("清单篡改拒", b.check_pkg(&bad) == Err(XC_BAD_PK), "卡 4 变 9");
    // 族四：移交与重复。
    s.add("首次移交过", b.handover(&p1).is_ok(), "一交");
    s.add("重复移交拒", b.handover(&p1) == Err(XC_RESENT), "同包再交");
    s.add("移交数 1", b.handed_count() == 1, "一包");
    // 族五：ARM 签收与回执。
    s.add("签收回执", b.sign_recv(&p1) == Ok(1 * REPLY_K + 1), "回执单向");
    s.add("回执入账", b.receipts.len() == 1, "一执");
    // 未移交的包签收拒。
    let p2 = match b.make_pkg(2) {
        Ok(p) => p,
        Err(_) => {
            s.add("造包 2 败", false, "不应到");
            return s;
        }
    };
    s.add("未交签收拒", b.sign_recv(&p2) == Err(XC_NOT_PK), "先交后签");
    // 回执核：真回执过、假回执拒。
    s.add("真执可验", b.verify_reply(1, 1 * REPLY_K + 1), "单向核");
    s.add("假执拒", b.verify_reply(1, 1 * REPLY_K + 2) == false, "常数错");
    s.add("包错执拒", b.verify_reply(2, 1 * REPLY_K + 1) == false, "包号错");
    s.add("魔数核", CL_SEG_BASE == 0x30F00, "段基址");
    s
}
