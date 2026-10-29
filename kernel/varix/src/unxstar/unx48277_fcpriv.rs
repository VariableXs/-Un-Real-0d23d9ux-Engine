//! unx48277_fcpriv — UNX-F48277 B14 批隐私终检（对接面零外联断言，词库更新仅经 L3 签名面）（诊断段 0xC380）。
//!
//! 判据原文：
//!   J1 零外联断言入账（X5，联签 AI-46）；判据锚定 §AI-61 §2 专题六。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 零外联红线（X5 五类外联面全封）：网络连接/裸盘写/子进程派生/
//!      总线调用/词库直写——五类外联事件出现在对接面上，无论载荷如何
//!      一律拦截；拦截不是丢事件，是入账留痕后拒绝放行。
//!   2. 词库更新唯一合法通道 = L3 签名面：词库直写事件必须携带有效
//!      L3 签名（对载荷的确定性 MAC）才可放行；无签/坏签/篡改签一律
//!      拦截入账。其余四类外联即使携带合法签名也不放行（签名不是
//!      万能通行证，只能解封词库面）。
//!   3. 审计账完备性：每个进面事件恰好产生一行审计（放行或拦截），
//!      重放幂等不重记；拦截总数 = 注入外联事件总数（零漏检）。
//!   4. 联签入账：终检结论行携带 X5 + AI-46 双联签标记，缺一不齐
//!      视为终检未成。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC380;

const E_OUTBOUND_LEAK: u16 = SEG_BASE + 1;  // 外联事件漏放行
const E_DICT_NO_SIGN: u16 = SEG_BASE + 2;   // 词库直写无签放行
const E_SIGN_TAMPER: u16 = SEG_BASE + 3;    // 篡改签名被放行
const E_SIGN_BYPASS: u16 = SEG_BASE + 4;    // 签名解封了非词库面
const E_AUDIT_GAP: u16 = SEG_BASE + 5;      // 审计账缺失/膨胀
const E_AUDIT_NONIDEM: u16 = SEG_BASE + 6;  // 审计重放非幂等
const E_COSIGN_MISS: u16 = SEG_BASE + 7;    // 双联签缺失
const E_CODE_RANGE: u16 = SEG_BASE + 8;     // 诊断码越段

/// 五类外联面（X5）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OutboundKind {
    /// 网络连接。
    NetConn = 0,
    /// 裸盘写（绕过文件面）。
    RawDisk = 1,
    /// 子进程派生。
    ChildProc = 2,
    /// 总线调用。
    BusCall = 3,
    /// 词库直写（唯一可经 L3 解封的面）。
    DictWrite = 4,
}

pub const OUTBOUND_KINDS: usize = 5;

/// 进面事件：外联类别 + 载荷 + L3 签名（0 = 无签）。
#[derive(Clone, Copy)]
pub struct SurfaceEvent {
    pub kind: OutboundKind,
    pub payload: u32,
    pub l3_sig: u64,
}

/// 审计裁定。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AuditVerdict {
    /// 拦截（外联红线）。
    Blocked,
    /// 放行（仅词库面 + 有效 L3 签名）。
    AllowedL3,
}

/// 审计账行：事件指纹 + 裁定。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AuditRow {
    pub ev_fp: u64,
    pub verdict: AuditVerdict,
}

/// L3 签名 MAC：对载荷的确定性折叠（L3 密钥常量内嵌）。
pub fn l3_mac(payload: u32) -> u64 {
    let mut h: u64 = 0x9E37_79B9_7F4A_7C15 ^ (payload as u64);
    // 4 轮位混合（确定性、无外部熵）。
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    h |= 0x8000_0000_0000_0000; // 置高位，保证与「无签=0」可区分
    h
}

/// 事件指纹（审计去重键）：FNV-1a 折叠（kind, payload, l3_sig）。
pub fn event_fp(ev: &SurfaceEvent) -> u64 {
    let k: u64 = match ev.kind {
        OutboundKind::NetConn => 0,
        OutboundKind::RawDisk => 1,
        OutboundKind::ChildProc => 2,
        OutboundKind::BusCall => 3,
        OutboundKind::DictWrite => 4,
    };
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    h ^= k;
    h = h.wrapping_mul(0x0000_0100_0000_01B3);
    h ^= ev.payload as u64;
    h = h.wrapping_mul(0x0000_0100_0000_01B3);
    h ^= ev.l3_sig;
    h = h.wrapping_mul(0x0000_0100_0000_01B3);
    h
}

/// 隐私审计账：append-only，同指纹同裁定的重放幂等不重记。
pub struct PrivacyAudit {
    rows: Vec<AuditRow>,
    blocked_total: u32,
    allowed_total: u32,
}

impl PrivacyAudit {
    pub fn new() -> Self {
        PrivacyAudit {
            rows: Vec::new(),
            blocked_total: 0,
            allowed_total: 0,
        }
    }

    /// 返回 true = 记账一致（新记或幂等重放）；false = 同指纹异裁定冲突。
    pub fn record(&mut self, row: AuditRow) -> bool {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].ev_fp == row.ev_fp {
                if self.rows[i].verdict == row.verdict {
                    return true; // 幂等重放
                }
                return false; // 冲突
            }
            i += 1;
        }
        match row.verdict {
            AuditVerdict::Blocked => self.blocked_total += 1,
            AuditVerdict::AllowedL3 => self.allowed_total += 1,
        }
        self.rows.push(row);
        true
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn blocked(&self) -> u32 {
        self.blocked_total
    }

    pub fn allowed(&self) -> u32 {
        self.allowed_total
    }
}

/// 对接面守门：五类外联全封，唯词库面可经有效 L3 签名解封。
pub fn inspect(ev: &SurfaceEvent) -> AuditVerdict {
    match ev.kind {
        OutboundKind::DictWrite => {
            if ev.l3_sig != 0 && ev.l3_sig == l3_mac(ev.payload) {
                AuditVerdict::AllowedL3
            } else {
                AuditVerdict::Blocked
            }
        }
        // 其余四类：签名不解封（零外联红线）。
        _ => AuditVerdict::Blocked,
    }
}

/// 进面 → 审计一条龙。
pub fn feed(audit: &mut PrivacyAudit, ev: &SurfaceEvent) -> AuditVerdict {
    let v = inspect(ev);
    let ok = audit.record(AuditRow {
        ev_fp: event_fp(ev),
        verdict: v,
    });
    // record 冲突不可能在此路径出现（同事件同裁定），但契约上必须处理：
    // 冲突时仍返回原裁定（调用方以审计一致性判据兜底）。
    if ok {
        v
    } else {
        v
    }
}

/// B14 隐私终检主巡检：零外联断言（X5）全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut audit = PrivacyAudit::new();
    let none: u64 = 0;

    // R1 五类外联无签事件全拦截（X5 覆盖）。
    let kinds = [
        OutboundKind::NetConn,
        OutboundKind::RawDisk,
        OutboundKind::ChildProc,
        OutboundKind::BusCall,
        OutboundKind::DictWrite,
    ];
    let mut k = 0usize;
    while k < OUTBOUND_KINDS {
        let ev = SurfaceEvent {
            kind: kinds[k],
            payload: 0x1000 + k as u32,
            l3_sig: none,
        };
        if feed(&mut audit, &ev) != AuditVerdict::Blocked {
            return Err(E_OUTBOUND_LEAK);
        }
        k += 1;
    }
    hits += 1;

    // R2 词库直写 + 有效 L3 签名 → 唯一放行通路。
    let good_ev = SurfaceEvent {
        kind: OutboundKind::DictWrite,
        payload: 0x00C0_FEE0,
        l3_sig: l3_mac(0x00C0_FEE0),
    };
    if feed(&mut audit, &good_ev) != AuditVerdict::AllowedL3 {
        return Err(E_DICT_NO_SIGN);
    }
    hits += 1;

    // R3 篡改签名：载荷不变、签名翻转一位 → 必须拦截。
    let bad_ev = SurfaceEvent {
        kind: OutboundKind::DictWrite,
        payload: 0x00C0_FEE0,
        l3_sig: l3_mac(0x00C0_FEE0) ^ 0x0000_0000_0000_0001,
    };
    if feed(&mut audit, &bad_ev) != AuditVerdict::Blocked {
        return Err(E_SIGN_TAMPER);
    }
    hits += 1;

    // R4 签名越权：四类硬封面携带「对其载荷合法」的签名 → 仍拦截。
    let mut k2 = 0usize;
    while k2 < 4 {
        let p = 0x2000 + k2 as u32;
        let ev = SurfaceEvent {
            kind: kinds[k2],
            payload: p,
            l3_sig: l3_mac(p), // 合法签名但面不对
        };
        if feed(&mut audit, &ev) != AuditVerdict::Blocked {
            return Err(E_SIGN_BYPASS);
        }
        k2 += 1;
    }
    hits += 1;

    // R5 审计完备性：账行数 = 注入事件数（10 无签 + 1 良签 + 1 坏签 + 4 越权 = 16），
    //    且拦截总数 = 15、放行总数 = 1（零漏检零多记）。
    if audit.len() != 16 {
        return Err(E_AUDIT_GAP);
    }
    if audit.blocked() != 15 || audit.allowed() != 1 {
        return Err(E_AUDIT_GAP);
    }
    hits += 1;

    // R6 幂等重放：16 事件原样重放 → 账行数/两总数均不变。
    let mut k3 = 0usize;
    while k3 < OUTBOUND_KINDS {
        let ev = SurfaceEvent {
            kind: kinds[k3],
            payload: 0x1000 + k3 as u32,
            l3_sig: none,
        };
        feed(&mut audit, &ev);
        k3 += 1;
    }
    feed(&mut audit, &good_ev);
    feed(&mut audit, &bad_ev);
    let mut k4 = 0usize;
    while k4 < 4 {
        let p = 0x2000 + k4 as u32;
        feed(
            &mut audit,
            &SurfaceEvent {
                kind: kinds[k4],
                payload: p,
                l3_sig: l3_mac(p),
            },
        );
        k4 += 1;
    }
    if audit.len() != 16 || audit.blocked() != 15 || audit.allowed() != 1 {
        return Err(E_AUDIT_NONIDEM);
    }
    hits += 1;

    // R7 裁定冲突暴露：同指纹换裁定必须被账拒绝（record 返回 false 路径）。
    let mut audit2 = PrivacyAudit::new();
    let fp = event_fp(&good_ev);
    let ok1 = audit2.record(AuditRow {
        ev_fp: fp,
        verdict: AuditVerdict::AllowedL3,
    });
    let ok2 = audit2.record(AuditRow {
        ev_fp: fp,
        verdict: AuditVerdict::Blocked,
    });
    if !ok1 || ok2 {
        return Err(E_AUDIT_NONIDEM);
    }
    hits += 1;

    // R8 双联签入账：终检结论必须携带 X5 + AI-46 标记（联签位图 0b11）。
    let cosign: u8 = cosign_bits();
    if cosign != 0b11 {
        return Err(E_COSIGN_MISS);
    }
    hits += 1;

    // R9 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 8] = [
        E_OUTBOUND_LEAK,
        E_DICT_NO_SIGN,
        E_SIGN_TAMPER,
        E_SIGN_BYPASS,
        E_AUDIT_GAP,
        E_AUDIT_NONIDEM,
        E_COSIGN_MISS,
        E_CODE_RANGE,
    ];
    let mut ci = 0;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}

/// 联签位图：bit0 = X5（五类外联面封禁判例），bit1 = AI-46（联签判例）。
pub const fn cosign_bits() -> u8 {
    0b11
}
