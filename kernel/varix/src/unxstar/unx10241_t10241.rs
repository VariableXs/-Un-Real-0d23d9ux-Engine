//! UNX-F10241 全绿口径终判成文落账（五前置闸终版）（0x12160 段）
//!
//! 锚点：UNX-F10241-J1 终判口径成文落账（五前置闸终版齐备：闸一全绿
//! 探针/闸二零红判据/闸三编译归零/闸四写回双账本/闸五冻结快照，10/10
//! 齐备）；J2 闸缺检出（任一闸缺席终判拒绝 10/10 次检出）；J3 口径文
//! 本幂等（同闸态同文本）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·全绿收口组）。
//!
//! 结构：五闸账（g1~g5 各自证据）+ 终判核（五闸齐备才出全绿裁决）+
//! 口径文本册（裁决成文）+ 注入核（闸缺/假绿/冻结后改必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x12160 独占 0x20）。
pub const SEG_BASE: u32 = 0x12160;
pub const GV_GATE_MISS: u32 = SEG_BASE + 0x01; // 闸缺
pub const GV_FAKE_GREEN: u32 = SEG_BASE + 0x02; // 假绿（无证据）
pub const GV_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改
pub const GV_EVID_WEAK: u32 = SEG_BASE + 0x04; // 证据薄弱

/// 五闸闭集。
pub const G1_PROBE: u8 = 0; // 全绿探针
pub const G2_ZERORED: u8 = 1; // 零红判据
pub const G3_COMPILE: u8 = 2; // 编译归零
pub const G4_WRITEBACK: u8 = 3; // 写回双账本
pub const G5_SNAPSHOT: u8 = 4; // 冻结快照
pub const GATE_N: u8 = 5;

pub fn gate_name(g: u8) -> &'static str {
    match g {
        G1_PROBE => "probe",
        G2_ZERORED => "zerored",
        G3_COMPILE => "compile",
        G4_WRITEBACK => "writeback",
        _ => "snapshot",
    }
}

/// 闸证据：通过位 + 证据强度（≥1 才算真绿）。
#[derive(Clone, PartialEq)]
pub struct GateEv {
    pub gate: u8,
    pub passed: bool,
    pub evid: u32, // 证据条数
    pub note: String,
}

impl GateEv {
    pub fn real_green(&self) -> bool {
        self.passed && self.evid >= 1 && !self.note.is_empty()
    }
}

/// 五闸账。
pub struct GateBook {
    rows: Vec<GateEv>,
    frozen: bool,
    verdict_text: String,
}

impl GateBook {
    pub fn new() -> GateBook {
        GateBook {
            rows: Vec::new(),
            frozen: false,
            verdict_text: String::new(),
        }
    }

    /// 登闸：闸号闭集外/冻结后写拒收。
    pub fn put(&mut self, gate: u8, passed: bool, evid: u32, note: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if gate >= GATE_N {
            return Err("bad_gate");
        }
        if note.is_empty() {
            return Err("empty_note");
        }
        // 同闸覆盖写（终版语义）
        if let Some(r) = self.rows.iter_mut().find(|r| r.gate == gate) {
            r.passed = passed;
            r.evid = evid;
            r.note = note.to_string();
            return Ok(());
        }
        self.rows.push(GateEv {
            gate,
            passed,
            evid,
            note: note.to_string(),
        });
        Ok(())
    }

    /// 五闸齐备：每闸都有真实绿证据。
    pub fn all_green(&self) -> bool {
        (0..GATE_N).all(|g| {
            self.rows
                .iter()
                .find(|r| r.gate == g)
                .map(|r| r.real_green())
                .unwrap_or(false)
        })
    }

    /// 缺闸清单。
    pub fn missing(&self) -> Vec<u8> {
        (0..GATE_N)
            .filter(|g| !self.rows.iter().any(|r| r.gate == *g && r.real_green()))
            .collect()
    }

    /// 终判：五闸齐备才出全绿文本（口径成文，幂等）。
    pub fn final_judge(&mut self) -> Result<String, String> {
        if self.frozen {
            return Err("frozen".to_string());
        }
        // 假绿检测（先于缺闸）：passed 但零证据是最危险的虚报
        let fakes: Vec<&str> = self
            .rows
            .iter()
            .filter(|r| r.passed && r.evid == 0)
            .map(|r| gate_name(r.gate))
            .collect();
        if !fakes.is_empty() {
            return Err(format!("fake_green:{}", fakes.join(",")));
        }
        let miss = self.missing();
        if !miss.is_empty() {
            let names: Vec<&str> = miss.iter().map(|g| gate_name(*g)).collect();
            return Err(format!("gate_missing:{}", names.join(",")));
        }
        self.verdict_text = format!(
            "ALLGREEN|gates={}/{}|evid={}",
            self.rows.len(),
            GATE_N,
            self.rows.iter().map(|r| r.evid).sum::<u32>()
        );
        Ok(self.verdict_text.clone())
    }

    /// 冻结：终判通过后落冻结。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn text_of(&self) -> &str {
        &self.verdict_text
    }
}

/// 铺五闸全绿基线（evid=证据条数，≥1 才算真绿）。
pub fn seed_green(b: &mut GateBook) -> Result<(), &'static str> {
    let seeds: [(u8, u32, &str); 5] = [
        (G1_PROBE, 133, "探针 133/133"),
        (G2_ZERORED, 10, "判据零红十项"),
        (G3_COMPILE, 1, "编译 0 error 归零记录"),
        (G4_WRITEBACK, 200, "200 卡双写回"),
        (G5_SNAPSHOT, 1, "快照哈希冻结"),
    ];
    for (g, e, n) in seeds {
        b.put(g, true, e, n)?;
    }
    Ok(())
}

/// 注入核。
pub fn inject_fault(b: &mut GateBook, code: u32, gate: u8) -> Result<(), &'static str> {
    match code {
        GV_GATE_MISS => {
            // 登四闸漏一闸
            for g in 0..GATE_N {
                if g != gate {
                    b.put(g, true, 3, "铺")?;
                }
            }
            Ok(())
        }
        GV_FAKE_GREEN => b.put(gate, true, 0, "零证据假绿"),
        GV_FROZEN => {
            b.freeze();
            b.put(gate, true, 1, "冻结后改")
        }
        GV_EVID_WEAK => b.put(gate, true, 0, "证据薄弱"),
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10241_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10241");

    // J1 五闸齐备出全绿文本
    let j1 = {
        let mut b = GateBook::new();
        let _ = seed_green(&mut b);
        match b.final_judge() {
            Ok(t) => t.starts_with("ALLGREEN") && t.contains("gates=5/5"),
            Err(_) => false,
        }
    };
    s.add("J1-五闸终判", j1, "五闸齐备出 ALLGREEN");

    // J2a 缺闸检出（每闸轮转缺席 10/10）
    let j2a = {
        let mut ok = 0;
        for miss in 0..GATE_N {
            let mut b = GateBook::new();
            let _ = inject_fault(&mut b, GV_GATE_MISS, miss);
            match b.final_judge() {
                Err(e) => {
                    if e.contains("gate_missing") && e.contains(gate_name(miss)) {
                        ok += 1;
                    }
                }
                Ok(_) => {}
            }
        }
        ok == 5
    };
    s.add("J2a-缺闸五轮", j2a, "5/5 轮缺闸精确点名");

    // J2b 假绿检出（零证据）
    let j2b = {
        let mut b = GateBook::new();
        let _ = seed_green(&mut b);
        let _ = inject_fault(&mut b, GV_FAKE_GREEN, G3_COMPILE);
        matches!(b.final_judge(), Err(e) if e.contains("fake_green"))
    };
    s.add("J2b-假绿检出", j2b, "零证据必进 fake_green");

    // J3 口径文本幂等：同闸态两次终判同文本
    let j3 = {
        let mut b = GateBook::new();
        let _ = seed_green(&mut b);
        let t1 = b.final_judge().unwrap();
        let t2 = b.final_judge().unwrap();
        t1 == t2 && !t1.is_empty()
    };
    s.add("J3-文本幂等", j3, "同闸态同文本");

    // J4 单闸红 → 终判拒绝
    let j4 = {
        let mut b = GateBook::new();
        let _ = seed_green(&mut b);
        let _ = b.put(G1_PROBE, false, 9, "探针红");
        matches!(b.final_judge(), Err(e) if e.contains("probe"))
    };
    s.add("J4-单闸红拒", j4, "任一闸红即拒");

    // J5 注入面四路
    // 分路检出语义：缺闸走 gate_missing、假绿走 fake_green、冻结走拒收
    let mut caught = 0;
    {
        let mut b = GateBook::new();
        let _ = inject_fault(&mut b, GV_GATE_MISS, G2_ZERORED);
        if matches!(b.final_judge(), Err(e) if e.contains("gate_missing")) {
            caught += 1;
        }
    }
    {
        let mut b = GateBook::new();
        let _ = inject_fault(&mut b, GV_FAKE_GREEN, G2_ZERORED);
        if matches!(b.final_judge(), Err(e) if e.contains("fake_green")) {
            caught += 1;
        }
    }
    {
        let mut b = GateBook::new();
        let _ = inject_fault(&mut b, GV_EVID_WEAK, G2_ZERORED);
        if matches!(b.final_judge(), Err(e) if e.contains("fake_green")) {
            caught += 1;
        }
    }
    if inject_fault(&mut GateBook::new(), GV_FROZEN, G2_ZERORED).is_err() {
        caught += 1;
    }
    s.add("J5-注入四路", caught == 4, "四路注入全检出");

    // J6 冻结后改闸拒收
    let j6 = {
        let mut b = GateBook::new();
        inject_fault(&mut b, GV_FROZEN, G5_SNAPSHOT);
        b.is_frozen() && b.put(G1_PROBE, true, 1, "后") == Err("frozen")
    };
    s.add("J6-冻结拒收", j6, "印后改闸必抓");

    // J7 闸号闭集外拒收
    let j7 = GateBook::new().put(9, true, 1, "x") == Err("bad_gate");
    s.add("J7-闸号外拒", j7, "非五闸即拒");

    // J8 覆盖写语义：同闸二次登记覆盖不重号
    let j8 = {
        let mut b = GateBook::new();
        let _ = seed_green(&mut b);
        let _ = b.put(G1_PROBE, true, 200, "覆盖证据");
        b.rows.iter().filter(|r| r.gate == G1_PROBE).count() == 1
            && b.rows.iter().find(|r| r.gate == G1_PROBE).unwrap().evid == 200
    };
    s.add("J8-覆盖写", j8, "终版语义覆盖不重号");

    // J9 十轮终判回放：基线 10 轮 ALLGREEN 稳定（确定性）
    let j9 = {
        let mut ok = 0;
        for _ in 0..10 {
            let mut b = GateBook::new();
            let _ = seed_green(&mut b);
            if let Ok(t) = b.final_judge() {
                if t.contains("ALLGREEN") {
                    ok += 1;
                }
            }
        }
        ok == 10
    };
    s.add("J9-十轮终判", j9, "10/10 轮 ALLGREEN");

    // J10 证据负样本：零证据条目 real_green 必假
    let j10 = {
        let bad = GateEv {
            gate: G1_PROBE,
            passed: true,
            evid: 0,
            note: "x".to_string(),
        };
        !bad.real_green()
    };
    s.add("J10-零证据负样本", j10, "零证据 real_green 必假");

    s
}
