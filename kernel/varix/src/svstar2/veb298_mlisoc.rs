//! veb298_mlisoc —— VE-F0298 Mali SoC 集成变体处理。
//! 锚点（CoRun Unxreal 主册）：VE-F0298 · Mali SoC 集成变体处理 · 目标 340 行
//! 判据原文：VE-F0298-J1 SoC 变体账：变体登记（SoC 厂 + 变
//!           体名 + 三参数：核数 / 频上限 / L2 容 KB）；参数
//!           界核（核 1~32、频 100~2000、L2 0~8192）；变体
//!           覆盖（同 SoC 厂同变体名重登 → 覆盖旧参数）；厂
//!           内枚举（列同厂全部变体名）；能力推断（核数 ×
//!           频 → 吞吐指数，L2 ≥ 512 给 10% 加成）；变体基
//!           准（吞吐指数分级：≥1000 高 / ≥400 中 / 其余
//!           低）；注销（变体可注销，注销后枚举不见）。
//! 组件：变体账 + 参数界 + 覆盖写 + 吞吐推断。
//! 职责定位：SoC 变体是「Mali 的百家衣」——同一颗 GPU 缝在
//!           不同 SoC 里就是不同的衣服：核数被砍、频率被限、
//!           L2 被裁；覆盖写是百家衣的补丁：同名牌子新补丁
//!           盖旧补丁；吞吐推断是量体裁衣：先量（参数）再
//!           裁（分级）。
//! 性能逐项分解：登记 O(变体数)；推断 O(1)。
//! 诊断码段：0x31380（0x31381~0x31389 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SC_VERSION: &str = "veb298_mlisoc-v1";

/// 诊断段基址（0x31380 段 · 本模块独占）。
pub const SC_SEG_BASE: u32 = 0x31380;
pub const XC_MAGIC: u32 = 0x31381; // 账魔数破
pub const XC_CORES: u32 = 0x31382; // 核数出界
pub const XC_FREQ: u32 = 0x31383; // 频出界
pub const XC_L2: u32 = 0x31384; // L2 出界
pub const XC_NOVAR: u32 = 0x31385; // 无此变体

/// 账魔数。
const MAGIC: u32 = 0x3138_1B5A;

/// 参数界。
pub const CORES_MIN: u32 = 1;
pub const CORES_MAX: u32 = 32;
pub const FREQ_MIN: u32 = 100;
pub const FREQ_MAX: u32 = 2000;
pub const L2_MAX: u32 = 8192;
/// L2 加成线。
pub const L2_BONUS_TH: u32 = 512;

/// 变体。
pub struct Variant {
    /// SoC 厂 ID（模型化整数）。
    pub vendor: u32,
    /// 变体名 ID。
    pub name: u32,
    pub cores: u32,
    pub freq_max: u32,
    pub l2_kb: u32,
}

/// 变体账。
pub struct SocBook {
    vars: Vec<Variant>,
    magic: u32,
}

/// 分级。
#[derive(Debug, PartialEq, Eq)]
pub enum Grade {
    High,
    Mid,
    Low,
}

impl SocBook {
    /// 建。
    pub fn new() -> SocBook {
        SocBook { vars: Vec::new(), magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    fn find(&self, vendor: u32, name: u32) -> Option<usize> {
        for i in 0..self.vars.len() {
            if self.vars[i].vendor == vendor && self.vars[i].name == name {
                return Some(i);
            }
        }
        None
    }

    /// 登记 / 覆盖。
    pub fn put(&mut self, vendor: u32, name: u32, cores: u32, freq: u32, l2: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if cores < CORES_MIN || cores > CORES_MAX {
            return Err(XC_CORES);
        }
        if freq < FREQ_MIN || freq > FREQ_MAX {
            return Err(XC_FREQ);
        }
        if l2 > L2_MAX {
            return Err(XC_L2);
        }
        match self.find(vendor, name) {
            Some(i) => {
                self.vars[i].cores = cores;
                self.vars[i].freq_max = freq;
                self.vars[i].l2_kb = l2;
                Ok(())
            }
            None => {
                self.vars.push(Variant { vendor, name, cores, freq_max: freq, l2_kb: l2 });
                Ok(())
            }
        }
    }

    /// 查参数。
    pub fn get(&self, vendor: u32, name: u32) -> Result<(u32, u32, u32), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        match self.find(vendor, name) {
            Some(i) => Ok((self.vars[i].cores, self.vars[i].freq_max, self.vars[i].l2_kb)),
            None => Err(XC_NOVAR),
        }
    }

    /// 厂内枚举（变体名序，登记序）。
    pub fn enum_vendor(&self, vendor: u32) -> Vec<u32> {
        let mut v = Vec::new();
        for r in self.vars.iter() {
            if r.vendor == vendor {
                v.push(r.name);
            }
        }
        v
    }

    /// 吞吐指数：核数 × 频 / 10；L2 ≥ 512 → +10%。
    pub fn throughput(vendor: u32, cores: u32, freq: u32, l2: u32) -> u32 {
        let _ = vendor;
        let base = cores * freq / 10;
        if l2 >= L2_BONUS_TH {
            base + base / 10
        } else {
            base
        }
    }

    /// 分级。
    pub fn grade(idx: u32) -> Grade {
        if idx >= 1000 {
            Grade::High
        } else if idx >= 400 {
            Grade::Mid
        } else {
            Grade::Low
        }
    }

    /// 注销。
    pub fn del(&mut self, vendor: u32, name: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        match self.find(vendor, name) {
            Some(i) => {
                self.vars.remove(i);
                Ok(())
            }
            None => Err(XC_NOVAR),
        }
    }

    /// 变体总数。
    pub fn count(&self) -> usize {
        self.vars.len()
    }
}

pub fn run_veb298_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0298");
    let mut b = SocBook::new();
    // 族零：参数界。
    s.add("核 0 拒", b.put(1, 1, 0, 600, 256) == Err(XC_CORES), "下界");
    s.add("核 33 拒", b.put(1, 1, 33, 600, 256) == Err(XC_CORES), "上界");
    s.add("频 50 拒", b.put(1, 1, 4, 50, 256) == Err(XC_FREQ), "下界");
    s.add("频 2500 拒", b.put(1, 1, 4, 2500, 256) == Err(XC_FREQ), "上界");
    s.add("L2 9 千拒", b.put(1, 1, 4, 600, 9000) == Err(XC_L2), "上界");
    // 族一：登记与查。
    s.add("登 v1a", b.put(1, 10, 4, 800, 256).is_ok(), "四核 800M");
    s.add("查 v1a", b.get(1, 10) == Ok((4, 800, 256)), "三参");
    s.add("查无变体", b.get(1, 99) == Err(XC_NOVAR), "无此名");
    // 族二：覆盖写。
    s.add("覆盖 v1a", b.put(1, 10, 2, 600, 128).is_ok(), "新补丁");
    s.add("盖后查", b.get(1, 10) == Ok((2, 600, 128)), "参数换");
    s.add("总数 1", b.count() == 1, "不增条");
    // 厂内枚举。
    let _ = b.put(1, 20, 8, 1000, 1024);
    let _ = b.put(2, 10, 16, 1400, 2048);
    let e1 = b.enum_vendor(1);
    s.add("厂 1 两体", e1.len() == 2 && e1[0] == 10 && e1[1] == 20, "登记序");
    let e2 = b.enum_vendor(2);
    s.add("厂 2 一体", e2.len() == 1 && e2[0] == 10, "同名单厂");
    // 族三：吞吐推断。
    // 4 核 800M L2 256：4×800/10 = 320 无加成。
    s.add("指数 320", SocBook::throughput(1, 4, 800, 256) == 320, "无加成");
    // 8 核 1000M L2 1024：800 + 80 = 880。
    s.add("指数 880", SocBook::throughput(1, 8, 1000, 1024) == 880, "L2 加成");
    // 16 核 1400 L2 2048：2240 + 224 = 2464。
    s.add("指数 2464", SocBook::throughput(2, 16, 1400, 2048) == 2464, "高配");
    // 分级。
    s.add("320 低", SocBook::grade(320) == Grade::Low, "<400");
    s.add("880 中", SocBook::grade(880) == Grade::Mid, "400~999");
    s.add("2464 高", SocBook::grade(2464) == Grade::High, "≥1000");
    s.add("400 恰中", SocBook::grade(400) == Grade::Mid, "含等");
    s.add("1000 恰高", SocBook::grade(1000) == Grade::High, "含等");
    // 族四：注销。
    s.add("注销 v1a", b.del(1, 10).is_ok(), "拔衣");
    s.add("注后查无", b.get(1, 10) == Err(XC_NOVAR), "枚举不见");
    s.add("注后再注", b.del(1, 10) == Err(XC_NOVAR), "无二注");
    s.add("余 2 体", b.count() == 2, "20 与厂 2");
    // 注销后枚举缩。
    let e1b = b.enum_vendor(1);
    s.add("厂 1 缩 1", e1b.len() == 1 && e1b[0] == 20, "只 20");
    s.add("魔数核", SC_SEG_BASE == 0x31380, "段基址");
    s
}
