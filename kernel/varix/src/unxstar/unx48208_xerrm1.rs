//! unx48208_xerrm1 — UNX-F48208 Xwayland 错误码族 M1 段（10 条）（诊断段 0xBAE0）。
//!
//! 判据原文：
//!   J1 错误码 ×10 零漂移；判据锚定 D1 同源体系。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 同源体系：错误码值 = SEG_BASE + 序号，10 条 M1 段常量写死，
//!      编译期与运行期双面可查；任何映射函数不得改写码值。
//!   2. 零漂移判据：码值 → 名称 → 码值 往返 ×10 全等；名称表与码表
//!      由同一张登记册派生，禁止两处维护。
//!   3. 段位守卫：全部 10 码必须落在本段 [SEG_BASE+1, SEG_BASE+0x1F]
//!      窗内，越窗即漂移（与其他段撞号 = 全域账污染）。
//!   4. 错误呈现三要素：每码携带 (码值, 稳定名, 用户可操作提示)，
//!      缺任一要素不许入册。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;      // vec! 宏（坑 2 三件套）
use crate::alloc::vec::Vec;
use crate::alloc::string::String;

pub const SEG_BASE: u16 = 0xBAE0;

// ── Xwayland M1 段 10 条（同源单点定义）──────────────────────
pub const XE_BRIDGE_DOWN: u16 = SEG_BASE + 1;  // 桥进程失联
pub const XE_CTX_LOST: u16 = SEG_BASE + 2;     // context 悬空引用
pub const XE_PROTO_BAD: u16 = SEG_BASE + 3;    // 协议事件畸形
pub const XE_FOCUS_RACE: u16 = SEG_BASE + 4;   // 焦点竞态冲突
pub const XE_CAND_CLIP: u16 = SEG_BASE + 5;    // 候选窗钳制越界
pub const XE_TRIGGER_MISS: u16 = SEG_BASE + 6; // 触发键映射缺失
pub const XE_COMMIT_CROSS: u16 = SEG_BASE + 7; // commit 跨窗串扰
pub const XE_MULTI_CLI: u16 = SEG_BASE + 8;    // 多客户并发超限
pub const XE_GEO_DRIFT: u16 = SEG_BASE + 9;    // 屏幕几何漂移
pub const XE_LEDGER_UNBAL: u16 = SEG_BASE + 10; // 对账账本不平

/// 错误呈现三要素。
#[derive(Debug, Clone)]
pub struct ErrEntry {
    pub code: u16,
    pub name: &'static str,
    pub hint: &'static str, // 用户可操作提示（非空）
}

/// 同源登记册：单一真源，名称/提示/码值三列一体。
#[derive(Debug)]
pub struct ErrRegistry {
    entries: Vec<ErrEntry>,
}

impl ErrRegistry {
    /// 装载 M1 段 10 条（唯一装载点，顺序即序号）。
    pub fn m1() -> Self {
        let entries = vec![
            ErrEntry {
                code: XE_BRIDGE_DOWN,
                name: "XE_BRIDGE_DOWN",
                hint: "重启输入桥后重试",
            },
            ErrEntry {
                code: XE_CTX_LOST,
                name: "XE_CTX_LOST",
                hint: "切换窗口后重新聚焦输入框",
            },
            ErrEntry {
                code: XE_PROTO_BAD,
                name: "XE_PROTO_BAD",
                hint: "上报应用名与操作序列给维护面",
            },
            ErrEntry {
                code: XE_FOCUS_RACE,
                name: "XE_FOCUS_RACE",
                hint: "稍候片刻重试切换输入",
            },
            ErrEntry {
                code: XE_CAND_CLIP,
                name: "XE_CAND_CLIP",
                hint: "调整屏幕布局后候选窗自动复位",
            },
            ErrEntry {
                code: XE_TRIGGER_MISS,
                name: "XE_TRIGGER_MISS",
                hint: "在设置里重新绑定切换热键",
            },
            ErrEntry {
                code: XE_COMMIT_CROSS,
                name: "XE_COMMIT_CROSS",
                hint: "重新聚焦目标窗口后继续输入",
            },
            ErrEntry {
                code: XE_MULTI_CLI,
                name: "XE_MULTI_CLI",
                hint: "关闭多余 X 客户端后重试",
            },
            ErrEntry {
                code: XE_GEO_DRIFT,
                name: "XE_GEO_DRIFT",
                hint: "重新登记屏幕几何",
            },
            ErrEntry {
                code: XE_LEDGER_UNBAL,
                name: "XE_LEDGER_UNBAL",
                hint: "导出诊断账并联系维护面",
            },
        ];
        ErrRegistry { entries }
    }

    /// 码 → 名称（线性查册，同源）。
    pub fn name_of(&self, code: u16) -> Option<&'static str> {
        self.entries.iter().find(|e| e.code == code).map(|e| e.name)
    }

    /// 码 → 提示。
    pub fn hint_of(&self, code: u16) -> Option<&'static str> {
        self.entries.iter().find(|e| e.code == code).map(|e| e.hint)
    }

    /// 名称 → 码（反查，供零漂移往返）。
    pub fn code_of(&self, name: &str) -> Option<u16> {
        self.entries.iter().find(|e| e.name == name).map(|e| e.code)
    }

    /// 段位守卫：全部码落在本段窗内。
    pub fn in_window(&self) -> bool {
        self.entries
            .iter()
            .all(|e| e.code > SEG_BASE && e.code <= SEG_BASE + 0x1F)
    }

    /// 三要素完整性：码唯一 + 名称唯一 + 提示非空。
    pub fn complete(&self) -> Result<(), u16> {
        for i in 0..self.entries.len() {
            let a = &self.entries[i];
            if a.hint.is_empty() || a.name.is_empty() {
                return Err(XE_LEDGER_UNBAL);
            }
            for j in (i + 1)..self.entries.len() {
                let b = &self.entries[j];
                if a.code == b.code || a.name == b.name {
                    return Err(XE_LEDGER_UNBAL);
                }
            }
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// 码值往返器：code → name → code（零漂移往返）。
pub fn roundtrip(reg: &ErrRegistry, code: u16) -> Result<u16, u16> {
    let n = reg.name_of(code).ok_or(XE_LEDGER_UNBAL)?;
    let c = reg.code_of(n).ok_or(XE_LEDGER_UNBAL)?;
    if c != code {
        return Err(XE_LEDGER_UNBAL); // 漂移！
    }
    Ok(c)
}

/// D1 同源体系挂账：M1 段 10 码的 (段, 序号) 抬头账。
pub fn d1_manifest() -> Vec<(u16, &'static str)> {
    let reg = ErrRegistry::m1();
    let mut out = Vec::new();
    for e in &reg.entries {
        out.push((e.code, e.name));
    }
    out
}

/// 自例程判据：落实 J1「错误码 ×10 零漂移」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let reg = ErrRegistry::m1();

    // R1 场景：×10 往返零漂移 —— 10 码逐个 code→name→code 全等。
    let codes = [
        XE_BRIDGE_DOWN,
        XE_CTX_LOST,
        XE_PROTO_BAD,
        XE_FOCUS_RACE,
        XE_CAND_CLIP,
        XE_TRIGGER_MISS,
        XE_COMMIT_CROSS,
        XE_MULTI_CLI,
        XE_GEO_DRIFT,
        XE_LEDGER_UNBAL,
    ];
    for c in codes {
        let back = roundtrip(&reg, c)?;
        if back != c {
            return Err(XE_LEDGER_UNBAL);
        }
    }
    hits += 1;

    // R2 场景：段位守卫 —— 10 码全落 [SEG_BASE+1, SEG_BASE+0x1F]。
    if !reg.in_window() {
        return Err(XE_LEDGER_UNBAL);
    }
    for (i, c) in codes.iter().enumerate() {
        if *c != SEG_BASE + (i as u16) + 1 {
            return Err(XE_LEDGER_UNBAL); // 序号连续性
        }
    }
    hits += 1;

    // R3 场景：三要素完整性 —— 码唯一/名唯一/提示非空，10/10。
    reg.complete()?;
    if reg.len() != 10 {
        return Err(XE_LEDGER_UNBAL);
    }
    hits += 1;

    // R4 场景：D1 抬头账 —— 清单 10 行且与登记册逐行一致。
    let m = d1_manifest();
    if m.len() != 10 {
        return Err(XE_LEDGER_UNBAL);
    }
    for (i, (c, n)) in m.iter().enumerate() {
        if *c != codes[i] || reg.name_of(*c) != Some(*n) {
            return Err(XE_LEDGER_UNBAL);
        }
    }
    hits += 1;

    // R5 场景：未知码查册 → None（不猜不编）。
    if reg.name_of(SEG_BASE + 20).is_some() || reg.hint_of(0).is_some() {
        return Err(XE_LEDGER_UNBAL);
    }
    if reg.code_of("XE_NOT_EXIST").is_some() {
        return Err(XE_LEDGER_UNBAL);
    }
    hits += 1;

    // R6 场景：往返器对未知码显性报错（不静默返回 0）。
    match roundtrip(&reg, SEG_BASE + 0x1F) {
        Err(XE_LEDGER_UNBAL) => {}
        _ => return Err(XE_LEDGER_UNBAL),
    }
    hits += 1;

    // R7 场景：提示面 —— 每码提示可检索且互不雷同（10 条独立提示）。
    let mut hints: Vec<&str> = Vec::new();
    for c in codes {
        let h = reg.hint_of(c).ok_or(XE_LEDGER_UNBAL)?;
        if hints.iter().any(|x| *x == h) {
            return Err(XE_LEDGER_UNBAL); // 提示雷同=模板残留
        }
        hints.push(h);
    }
    hits += 1;

    // R8 场景：码值恒等性 —— 双次装载登记册，同码同名（无运行期改写）。
    let reg2 = ErrRegistry::m1();
    for c in codes {
        if reg.name_of(c) != reg2.name_of(c) {
            return Err(XE_LEDGER_UNBAL);
        }
    }
    hits += 1;

    // R9 场景：与段基座的隔离 —— 本段 10 码不越过 0x1F 容量界。
    let last = codes[9];
    if last > SEG_BASE + 0x1F {
        return Err(XE_LEDGER_UNBAL);
    }
    hits += 1;

    // R10 场景：名称表逐条非空前缀纪律 —— 全部以 XE_ 开头（族标识）。
    for e in &reg.entries {
        if !e.name.starts_with("XE_") {
            return Err(XE_LEDGER_UNBAL);
        }
    }
    hits += 1;

    Ok(hits)
}
