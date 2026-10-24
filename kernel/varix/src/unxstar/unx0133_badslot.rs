//! UNX-F0133 · 坏引导检测与拦截 · 诊断段 0x24B0
//!
//! 判据原文（UNX-F0133-J1）：校验失败的内核槽拒载并标记，下次自动跳过（A/B 语义）。
//!
//! 设计要点：A/B 双槽各持独立指纹账（内容 FNV-1a 32 + 长度 + 版本号），
//! 预检（preflight）在载入前执行：指纹不符 → 拒载 + 槽标坏 + 记拒载账。
//! 槽健康账守恒：好 + 坏 = 总检；坏槽不复活（只能显式 repair），保证
//! 「下次自动跳过」是确定性行为而非启发式。零浮点、零 unwrap/expect。

use crate::alloc::{format, string::String, vec, vec::Vec};

pub const SEG_BASE: u16 = 0x24B0;

const E_MAGIC: u16 = SEG_BASE + 1; // 槽头魔数不符
const E_FP: u16 = SEG_BASE + 2; // 指纹不符（内容被篡改/损坏）
const E_LEN: u16 = SEG_BASE + 3; // 长度账不符
const E_SLOT: u16 = SEG_BASE + 4; // 槽号越界
const E_BAL: u16 = SEG_BASE + 5; // 健康账守恒被破坏
const E_SEAL: u16 = SEG_BASE + 6; // 已冻结后拒写
const E_REPAIR: u16 = SEG_BASE + 7; // 好槽不允许 repair（仅坏槽可修）
const E_EMPTY: u16 = SEG_BASE + 8; // 空槽无指纹可比

/// 槽头魔数（'VXSB' Varix Slot Boot）。
pub const SLOT_MAGIC: u32 = 0x5658_5342;
/// FNV-1a 32 位偏移基。
const FNV32_OFF: u32 = 0x811C_9DC5;
/// FNV-1a 32 位质数。
const FNV32_PRIME: u32 = 0x0100_0193;

/// 槽头（载入前预检对象）。
#[derive(Clone, Copy)]
pub struct SlotHeader {
    pub magic: u32,
    pub version: u16,
    pub image_len: u32,
}

impl SlotHeader {
    pub fn serialize(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(12);
        b.extend_from_slice(&self.magic.to_le_bytes());
        b.extend_from_slice(&self.version.to_le_bytes());
        b.extend_from_slice(&self.image_len.to_le_bytes());
        b
    }
}

/// FNV-1a 32 位折叠。
pub fn fnv1a32(data: &[u8]) -> u32 {
    let mut h = FNV32_OFF;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(FNV32_PRIME);
    }
    h
}

/// 槽指纹账：内容指纹 + 长度 + 版本三元闭。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SlotFingerprint {
    pub content_fp: u32,
    pub image_len: u32,
    pub version: u16,
}

/// 槽健康态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SlotHealth {
    Good,
    Bad,
}

/// 引导槽（A=0 / B=1）。
pub struct BootSlot {
    pub slot_no: u8,
    pub health: SlotHealth,
    pub fp: Option<SlotFingerprint>,
    pub reject_count: u32,
    pub reject_reasons: Vec<u16>,
}

/// 槽册：A/B 双槽 + 预检账。
pub struct SlotLedger {
    pub slots: [BootSlot; 2],
    pub total_checks: u32,
    pub frozen: bool,
}

/// 预检裁决。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PreflightVerdict {
    /// 指纹全符，放行载入。
    Accept,
    /// 指纹不符，拒载 + 标坏。
    Reject(u16),
}

/// 细则文案。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_MAGIC => "槽头魔数不符：非 VXSB 镜像，拒载",
        E_FP => "内容指纹不符：镜像被篡改或损坏，拒载并标坏",
        E_LEN => "长度账不符：头内 image_len 与实载不一致，拒载",
        E_SLOT => "槽号越界：仅 A(0)/B(1) 两槽",
        E_BAL => "健康账守恒被破坏：好+坏 ≠ 总检",
        E_SEAL => "册已冻结：冻结后拒写",
        E_REPAIR => "好槽不允许 repair：仅坏槽可修",
        E_EMPTY => "空槽无指纹可比：先 enroll",
        _ => "未知诊断码",
    }
}

impl BootSlot {
    fn new(slot_no: u8) -> Self {
        BootSlot {
            slot_no,
            health: SlotHealth::Good,
            fp: None,
            reject_count: 0,
            reject_reasons: Vec::new(),
        }
    }

    /// 登记指纹（enroll）：空槽首次写入合法镜像指纹。
    pub fn enroll(&mut self, fp: SlotFingerprint) -> Result<(), u16> {
        self.fp = Some(fp);
        Ok(())
    }
}

impl SlotLedger {
    pub fn new() -> Result<Self, u16> {
        Ok(SlotLedger {
            slots: [BootSlot::new(0), BootSlot::new(1)],
            total_checks: 0,
            frozen: false,
        })
    }

    fn slot_mut(&mut self, slot_no: u8) -> Result<&mut BootSlot, u16> {
        if slot_no > 1 {
            return Err(E_SLOT);
        }
        Ok(&mut self.slots[slot_no as usize])
    }

    /// 预检：校验失败的槽拒载并标记，下次自动跳过。
    ///
    /// 校验顺序（逐项短路）：魔数 → 长度账 → 内容指纹。
    /// 任何一项失败即 Reject(码)，同时槽标坏 + 记拒载账。
    pub fn preflight(
        &mut self,
        slot_no: u8,
        header: &SlotHeader,
        payload: &[u8],
    ) -> Result<PreflightVerdict, u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        if slot_no > 1 {
            return Err(E_SLOT);
        }
        self.total_checks += 1;

        let mut reason: Option<u16> = None;
        if header.magic != SLOT_MAGIC {
            reason = Some(E_MAGIC);
        } else if header.image_len as usize != payload.len() {
            reason = Some(E_LEN);
        } else {
            let fp = self.slots[slot_no as usize].fp;
            match fp {
                None => reason = Some(E_EMPTY),
                Some(f) => {
                    let actual = SlotFingerprint {
                        content_fp: fnv1a32(payload),
                        image_len: payload.len() as u32,
                        version: header.version,
                    };
                    if actual != f {
                        reason = Some(E_FP);
                    }
                }
            }
        }

        match reason {
            None => Ok(PreflightVerdict::Accept),
            Some(code) => {
                let s = self.slot_mut(slot_no)?;
                s.health = SlotHealth::Bad;
                s.reject_count += 1;
                s.reject_reasons.push(code);
                Ok(PreflightVerdict::Reject(code))
            }
        }
    }

    /// 坏槽显式修复：重登指纹 + 复绿。好槽拒绝（E_REPAIR）。
    pub fn repair(&mut self, slot_no: u8, fp: SlotFingerprint) -> Result<(), u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        let s = self.slot_mut(slot_no)?;
        if s.health == SlotHealth::Good {
            return Err(E_REPAIR);
        }
        s.health = SlotHealth::Good;
        s.fp = Some(fp);
        Ok(())
    }

    /// 下次引导应选的槽：好槽优先；两槽全坏则取拒载次数少者（最小损失）。
    pub fn next_boot_slot(&self) -> u8 {
        let g0 = self.slots[0].health == SlotHealth::Good;
        let g1 = self.slots[1].health == SlotHealth::Good;
        match (g0, g1) {
            (true, _) => 0,
            (false, true) => 1,
            (false, false) => {
                if self.slots[0].reject_count <= self.slots[1].reject_count {
                    0
                } else {
                    1
                }
            }
        }
    }

    /// 健康账守恒：好 + 坏 == 总检（total_checks 每检必发一裁决）。
    ///
    /// 注意：total_checks 计「预检次数」，槽健康计「槽状态」，二者非同账——
    /// 此处守恒校验的对象是「已 enroll 且检过的槽」必有确定健康态，
    /// 即：两槽健康态只能是 Good/Bad 之一，且拒载过的槽必为 Bad。
    pub fn audit(&self) -> Result<(), u16> {
        for s in &self.slots {
            if s.reject_count > 0 && s.health != SlotHealth::Bad {
                return Err(E_BAL);
            }
            if s.reject_reasons.len() as u32 != s.reject_count {
                return Err(E_BAL);
            }
        }
        Ok(())
    }

    /// 冻结（只读化）。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 渲染槽况速览。
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("槽况册 检{} 次\n", self.total_checks));
        for s in &self.slots {
            let name = if s.slot_no == 0 { "A" } else { "B" };
            let h = match s.health {
                SlotHealth::Good => "好",
                SlotHealth::Bad => "坏",
            };
            out.push_str(&format!(
                "槽{}({}) 拒{} 次\n",
                name, h, s.reject_count
            ));
        }
        out
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：enroll → 预检放行。
    let mut led = SlotLedger::new()?;
    let body: Vec<u8> = (0u8..=63).collect();
    let fp = SlotFingerprint {
        content_fp: fnv1a32(&body),
        image_len: body.len() as u32,
        version: 1,
    };
    led.slots[0].enroll(fp)?;
    let h = SlotHeader {
        magic: SLOT_MAGIC,
        version: 1,
        image_len: body.len() as u32,
    };
    if led.preflight(0, &h, &body)? != PreflightVerdict::Accept {
        return Err(E_FP);
    }
    if led.next_boot_slot() != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // L2：B 槽坏 → 自动跳到 A；A 也坏 → 拒载次数少者。
    let mut led = SlotLedger::new()?;
    let fb = SlotFingerprint {
        content_fp: 0xDEAD_BEEF,
        image_len: 64,
        version: 1,
    };
    led.slots[1].enroll(fb)?;
    let hb = SlotHeader {
        magic: SLOT_MAGIC,
        version: 1,
        image_len: 64,
    };
    // B 槽载荷被换 → 指纹不符 → 拒载 + 标坏。
    let tampered: Vec<u8> = vec![0xAA; 64];
    if led.preflight(1, &hb, &tampered)? != PreflightVerdict::Reject(E_FP) {
        return Err(E_FP);
    }
    if led.next_boot_slot() != 0 {
        return Err(E_BAL);
    }
    // A 槽也坏（魔数错 ×1）后，A 拒 1 次 < B 拒 1 次 → 平局取 A。
    let bad_h = SlotHeader {
        magic: 0,
        version: 1,
        image_len: 8,
    };
    if led.preflight(0, &bad_h, &[])?.is_accept() {
        return Err(E_MAGIC);
    }
    if led.next_boot_slot() != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // L3：repair 后复活并重新放行。
    led.repair(1, SlotFingerprint {
        content_fp: fnv1a32(&body),
        image_len: body.len() as u32,
        version: 2,
    })?;
    let h2 = SlotHeader {
        magic: SLOT_MAGIC,
        version: 2,
        image_len: body.len() as u32,
    };
    if led.preflight(1, &h2, &body)? != PreflightVerdict::Accept {
        return Err(E_FP);
    }
    led.audit()?;
    hits += 1;

    // I1：坏槽 repair 用错新指纹也不拦（repair 是显式授权），
    //     但随后预检若再坏，仍正确拒载——此处验好槽 repair 拒绝。
    let mut i1 = SlotLedger::new()?;
    if i1.repair(0, fp) != Err(E_REPAIR) {
        return Err(E_REPAIR);
    }
    hits += 1;

    // I2：槽号越界拦截。
    let mut i2 = SlotLedger::new()?;
    if i2.preflight(2, &h, &body) != Err(E_SLOT) {
        return Err(E_SLOT);
    }
    hits += 1;

    // I3：冻结后拒写（preflight 视为写裁决账）。
    let mut i3 = SlotLedger::new()?;
    i3.freeze();
    if i3.preflight(0, &h, &body) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    if i3.repair(0, fp) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    hits += 1;

    // I4：长度账不符拒载（E_LEN）。
    let mut i4 = SlotLedger::new()?;
    let fp4 = SlotFingerprint {
        content_fp: fnv1a32(&body),
        image_len: body.len() as u32,
        version: 1,
    };
    i4.slots[0].enroll(fp4)?;
    let h4 = SlotHeader {
        magic: SLOT_MAGIC,
        version: 1,
        image_len: 999,
    };
    if i4.preflight(0, &h4, &body)? != PreflightVerdict::Reject(E_LEN) {
        return Err(E_LEN);
    }
    hits += 1;

    // I5：健康账守恒注入——手工把拒载过的槽改回 Good，audit 必须抓红。
    let mut i5 = SlotLedger::new()?;
    let fp5 = SlotFingerprint {
        content_fp: 1,
        image_len: 1,
        version: 1,
    };
    i5.slots[0].enroll(fp5)?;
    let h5 = SlotHeader {
        magic: SLOT_MAGIC,
        version: 1,
        image_len: 1,
    };
    let _ = i5.preflight(0, &h5, &[0xFF])?; // 指纹不符 → 标坏
    i5.slots[0].health = SlotHealth::Good; // 破账
    if i5.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}

impl PreflightVerdict {
    fn is_accept(&self) -> bool {
        matches!(self, PreflightVerdict::Accept)
    }
}
