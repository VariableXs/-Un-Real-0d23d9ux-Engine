//! unx0449_dsdtloc —— UNX-F0449 DSDT 指针账（只读定位）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0449 · DSDT 指针账 · 目标 240 行
//! 判据原文：UNX-F0449-J1 DSDT（差分系统描述表，签名 "DSDT"）
//!           只读定位账：账一 FADT 内指针（FACP 偏移 140 的
//!           u32 = DSDT 物理地址，1.0 系；偏移 140 在 64 位
//!           系仍为主位面——X_DSDT @140+4 仅当 FADT 长度 ≥
//!           148 且非零时优先）；账二 代际优先核：X_DSDT 非
//!           零且表长 ≥ 148 → 用 64 位面；否则退 32 位面；
//!           面选择必须与根表代际一致（RSDT 代忽略 X_DSDT）；
//!           账三 DSDT 表头自证：定位到的表签名必须 "DSDT"，
//!           否则定位失败立案；账四 长度自证：declared ≥ 36
//!           且 ≤ 实际缓冲；账五 只读纪律：定位全程不写任
//!           何 FADT/DSDT 字节（软模型记账面证明零写）。
//! 组件：双代指针选择器 + 面优先核 + 表头自证闸 + 长度自
//!       证 + 只读记账面 + 失配立案器。
//! 职责定位：DSDT 是 ACPI 的「主板说明书」（ASL 字节码全在
//!   这里），定位错代是 64 位机器上最常见的启动悬案——32
//!   位指针在 >4G 内存位面直接截断；只读纪律是定位器的本
//!   分：看可以，动一根指头都算违例。
//! 性能逐项分解：面选择 O(1)；自证 O(4)；记账 O(1)。
//! 诊断码段：0x34180（0x34101~0x34189 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DL_VERSION: &str = "unx0449_dsdtloc-v1";

/// 诊断段基址（0x34180 段 · 本模块独占）。
pub const DL_SEG_BASE: u32 = 0x34180;
pub const XC_SIG: u32 = 0x34181; // 定位表签名非 DSDT
pub const XC_LEN: u32 = 0x34182; // 长度自证破
pub const XC_NO_PTR: u32 = 0x34183; // 双面指针全零
pub const XC_WRITE: u32 = 0x34184; // 只读违例
pub const XC_GEN: u32 = 0x34185; // 代际面失配
pub const XC_HDR_SHORT: u32 = 0x34186; // FADT 不足 144
pub const XC_MAGIC: u32 = 0x34189; // 账魔数破

/// FACP 内偏移。
pub const OFF_DSDT32: usize = 140; // u32（1.0 系）
pub const OFF_XDSDT: usize = 144; // u64（FADT ≥ 148）
pub const FACP_LEN64: usize = 148; // 64 位面生效的最小表长
/// 代际。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gen {
    R = 1, // RSDT 代（32 位面）
    X = 2, // XSDT 代（64 位面）
}

/// FADT 供面者：供出双面指针（软模型直存，免字节组装）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FacdSupply {
    pub declared_len: usize,
    pub dsdt32: u32,
    pub x_dsdt: u64,
    pub gen: Gen,
}

/// 面选择核：X_DSDT 非零且表长 ≥148 且代 X → 64 位面；否
/// 则退 32 位面；双面全零立案；RSDT 代无视 X_DSDT。
pub fn select_ptr(s: &FacdSupply) -> Result<u64, u32> {
    let x_eligible = s.declared_len >= FACP_LEN64 && s.x_dsdt != 0;
    match s.gen {
        Gen::X => {
            if x_eligible {
                Ok(s.x_dsdt)
            } else if s.dsdt32 != 0 {
                Ok(s.dsdt32 as u64)
            } else {
                Err(XC_NO_PTR)
            }
        }
        Gen::R => {
            if s.dsdt32 != 0 {
                Ok(s.dsdt32 as u64) // R 代无视 64 位面
            } else {
                Err(XC_NO_PTR)
            }
        }
    }
}

/// 代际面失配核：R 代却要求用 X 面 → 立案。
pub fn gen_face_ok(s: &FacdSupply, want_x: bool) -> Result<(), u32> {
    let is_x = s.gen == Gen::X;
    if is_x == want_x {
        Ok(())
    } else {
        Err(XC_GEN)
    }
}

/// DSDT 表自证：签名 "DSDT" + 长度自证。
pub fn dsdt_selfcheck(raw: &[u8]) -> Result<(), u32> {
    if raw.len() < 4 || &raw[0..4] != b"DSDT" {
        return Err(XC_SIG);
    }
    if raw.len() < 8 {
        return Err(XC_LEN);
    }
    let declared = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]) as usize;
    if declared < 36 || declared > raw.len() {
        return Err(XC_LEN);
    }
    Ok(())
}

/// 只读记账面：模拟定位器访问序列，只许 Read。
#[derive(Default, Debug, PartialEq, Eq)]
pub struct RoLedger {
    pub reads: u32,
    pub writes: u32,
}

impl RoLedger {
    pub fn read(&mut self) {
        self.reads += 1;
    }
    /// 写访问 = 违例。
    pub fn write(&mut self) -> Result<(), u32> {
        self.writes += 1;
        Err(XC_WRITE)
    }
    /// 纪律核：全程零写。
    pub fn clean(&self) -> Result<(), u32> {
        if self.writes == 0 {
            Ok(())
        } else {
            Err(XC_WRITE)
        }
    }
}

/// 定位演练：选面 → 读自证 → 记账核。
pub fn locate(s: &FacdSupply, dsdt: &[u8], led: &mut RoLedger) -> Result<(), u32> {
    let ptr = select_ptr(s)?;
    gen_face_ok(s, ptr > 0xFFFF_FFFF)?; // 64 位面要求 X 代
    led.read();
    dsdt_selfcheck(dsdt)?;
    led.read();
    led.clean()
}

/// 判据入口：UNX-F0449。
pub fn run_unx0449_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0449_dsdtloc");
    // 造 DSDT 表（夹具）。
    let mk_dsdt = |body_len: usize| {
        let mut v = vec![0u8; 36 + body_len];
        v[0..4].copy_from_slice(b"DSDT");
        v[4..8].copy_from_slice(&((36 + body_len) as u32).to_le_bytes());
        v
    };
    let dsdt_ok = mk_dsdt(100);
    // 族一：面选择。
    let sx = FacdSupply { declared_len: 148, dsdt32: 0x1000_0000, x_dsdt: 0x1_0000_0000, gen: Gen::X };
    cs.add("X 代走 64 位面", select_ptr(&sx) == Ok(0x1_0000_0000), "X_DSDT 优先");
    let sx32 = FacdSupply { declared_len: 148, dsdt32: 0x1000_0000, x_dsdt: 0, gen: Gen::X };
    cs.add("X 面零退 32", select_ptr(&sx32) == Ok(0x1000_0000), "退位面");
    let sr = FacdSupply { declared_len: 148, dsdt32: 0x1000_0000, x_dsdt: 0x1_0000_0000, gen: Gen::R };
    cs.add("R 代无视 X 面", select_ptr(&sr) == Ok(0x1000_0000), "32 位族纪律");
    let szero = FacdSupply { declared_len: 148, dsdt32: 0, x_dsdt: 0, gen: Gen::X };
    cs.add("双面全零立案", select_ptr(&szero) == Err(XC_NO_PTR), "无 DSDT");
    // 族二：短表退位面。
    let sshort = FacdSupply { declared_len: 140, dsdt32: 0x1000_0000, x_dsdt: 0x1_0000_0000, gen: Gen::X };
    cs.add("表长 <148 走 32", select_ptr(&sshort) == Ok(0x1000_0000), "X_DSDT 不生效");
    // 族三：代际面失配。
    cs.add("R 代讨 X 面立案", gen_face_ok(&sr, true) == Err(XC_GEN), "面代错配");
    cs.add("X 代讨 X 面过", gen_face_ok(&sx, true).is_ok(), "面代齐");
    // 族四：DSDT 自证。
    cs.add("签名 DSDT 过", dsdt_selfcheck(&dsdt_ok).is_ok(), "自证闸");
    cs.add("签名 FACP 拒", dsdt_selfcheck(b"FACPxxxx") == Err(XC_SIG), "拿错表");
    let mut badlen = mk_dsdt(10);
    badlen[4] = 0xFF; // declared 抬爆
    cs.add("declared 爆立案", dsdt_selfcheck(&badlen) == Err(XC_LEN), "长度自证破");
    let mut shortlen = mk_dsdt(10);
    shortlen[7] = 0; // declared < 36
    cs.add("declared <36 拒", dsdt_selfcheck(&shortlen) == Err(XC_LEN), "头最小 36");
    // 族五：只读纪律。
    let mut led = RoLedger::default();
    cs.add("定位演练过", locate(&sx, &dsdt_ok, &mut led).is_ok(), "选面+自证+零写");
    cs.add("记账 reads=2", led.reads == 2, "两次读");
    let mut led2 = RoLedger::default();
    let _ = led2.write();
    cs.add("写即违例", led2.clean() == Err(XC_WRITE), "只读红线");
    // 族六：端到端。
    let mut led3 = RoLedger::default();
    cs.add("R 代端到端", locate(&sr, &dsdt_ok, &mut led3).is_ok(), "32 位面全绿");
    // R 代 + 32 位指针 → gen_face_ok(s, false)：is_x=false==false ✓
    cs.add("记账 reads=2 复用", led3.reads == 2, "独立账面");
    // 族七：64 位指针面自证。
    let mut led4 = RoLedger::default();
    cs.add("X 代端到端", locate(&sx, &dsdt_ok, &mut led4).is_ok(), "64 位面全绿");
    // 族八：定位失败不写账（失败面零读增）。
    let mut led5 = RoLedger::default();
    cs.add("全零定位败", locate(&szero, &dsdt_ok, &mut led5) == Err(XC_NO_PTR), "无指针");
    cs.add("失败面 reads=0", led5.reads == 0, "先核后读");
    cs
}
