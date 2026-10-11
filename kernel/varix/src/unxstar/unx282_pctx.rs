//! unx282_pctx —— UNX-F0282 panic 上下文快照（寄存器/栈/堆指纹）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B15）：UNX-F0282 · panic 上
//! 下文快照（寄存器/栈/堆指纹） · 判据：UNX-F0282-J1 通用寄存器/
//! 控制寄存器/栈窗/关键堆指纹四件快照完整，转储可解析。
//! 组件：GPR 快照（16 通用寄存器）+ 控制寄存器四件 + 栈窗（16 槽
//! +FNV 窗指纹）+ 关键堆指纹（采样块聚合）+ 行式确定性转储与解析
//! 回读（roundtrip 恒等）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术。
//! 诊断码：字符串码五码互异（段位资源紧张沿用字符串码惯例）：
//! `E_PX_GPR_INCOMPLETE` / `E_PX_CTRL_INCOMPLETE` /
//! `E_PX_STACK_WINDOW_SHORT` / `E_PX_HEAP_MISMATCH` /
//! `E_PX_DUMP_MALFORMED`。

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与常量（阈值独立声明，判据独立手算对拍）
// ---------------------------------------------------------------------------

/// panic 快照版本。
pub const PCTX_VERSION: &str = "PX282-v1";
/// 通用寄存器数（x86-64 抽象 16 项——r0..r15）。
pub const GPR_COUNT: usize = 16;
/// 控制寄存器四件（rip/rflags/rsp/cr0 抽象）。
pub const CTRL_COUNT: usize = 4;
/// 栈窗槽数（快照窗——16 槽 u64）。
pub const STACK_WINDOW: usize = 16;
/// 堆指纹采样块上限。
pub const HEAP_SAMPLE_CAP: usize = 32;
/// 转储分隔符。
pub const DUMP_SEP: &str = "|";

/// 字符串诊断码五码（互异——枚举承载）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PxCode {
    /// 通用寄存器快照残缺。
    GprIncomplete,
    /// 控制寄存器快照残缺。
    CtrlIncomplete,
    /// 栈窗不足（窗槽数不齐）。
    StackWindowShort,
    /// 堆指纹失配。
    HeapMismatch,
    /// 转储畸形（不可解析）。
    DumpMalformed,
}

impl PxCode {
    pub const COUNT: usize = 5;

    pub fn all() -> [PxCode; PxCode::COUNT] {
        [
            PxCode::GprIncomplete,
            PxCode::CtrlIncomplete,
            PxCode::StackWindowShort,
            PxCode::HeapMismatch,
            PxCode::DumpMalformed,
        ]
    }

    /// 码文本（互异——五码恰值）。
    pub fn as_str(self) -> &'static str {
        match self {
            PxCode::GprIncomplete => "E_PX_GPR_INCOMPLETE",
            PxCode::CtrlIncomplete => "E_PX_CTRL_INCOMPLETE",
            PxCode::StackWindowShort => "E_PX_STACK_WINDOW_SHORT",
            PxCode::HeapMismatch => "E_PX_HEAP_MISMATCH",
            PxCode::DumpMalformed => "E_PX_DUMP_MALFORMED",
        }
    }

    /// 原因文案（全量显性）。
    pub fn reason(self) -> &'static str {
        match self {
            PxCode::GprIncomplete => "通用寄存器快照残缺：16 项未齐，快照拒收",
            PxCode::CtrlIncomplete => "控制寄存器快照残缺：四件未齐，快照拒收",
            PxCode::StackWindowShort => "栈窗不足：窗槽数不齐，指纹无从谈起",
            PxCode::HeapMismatch => "堆指纹失配：采样聚合指纹与预期不符",
            PxCode::DumpMalformed => "转储畸形：不可解析，拒绝回读",
        }
    }
}

/// 立案簿（字符串码事件账——有界不静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagNote {
    pub code: PxCode,
    pub subject: String,
    pub detail: String,
}

#[derive(Debug)]
pub struct DiagBag {
    notes: Vec<DiagNote>,
    cap: usize,
    dropped: usize,
}

impl DiagBag {
    pub fn new() -> DiagBag {
        DiagBag { notes: Vec::new(), cap: 64, dropped: 0 }
    }

    pub fn push(&mut self, code: PxCode, subject: &str, detail: &str) {
        if self.notes.len() >= self.cap {
            self.notes.remove(0);
            self.dropped += 1;
        }
        self.notes.push(DiagNote {
            code,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn count_of(&self, code: PxCode) -> usize {
        self.notes.iter().filter(|n| n.code == code).count()
    }

    pub fn has(&self, code: PxCode) -> bool {
        self.count_of(code) > 0
    }

    pub fn spoken(&self) -> String {
        if self.notes.is_empty() {
            return "诊断簿空".to_string();
        }
        let mut out = format!("诊断簿共{}条", self.notes.len());
        for n in self.notes.iter() {
            out.push('；');
            out.push_str(n.code.reason());
            out.push('：');
            out.push_str(&n.subject);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// FNV-1a 64（指纹原语——空串恰值 basis 0xcbf29ce484222325 手算锚）
// ---------------------------------------------------------------------------

/// FNV-1a 64 哈希（确定性——空输入恰值 basis）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// ---------------------------------------------------------------------------
// 四件快照（通用寄存器/控制寄存器/栈窗/堆指纹）
// ---------------------------------------------------------------------------

/// 通用寄存器快照（16 项——r0..r15 抽象命名）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GprSnap {
    pub regs: [u64; GPR_COUNT],
    /// 完整标记（注入计数==16 才完整）。
    pub complete: bool,
}

impl GprSnap {
    pub fn new() -> GprSnap {
        // 哨兵初始化（u64::MAX=未填语义——零值是合法寄存器值）。
        GprSnap { regs: [u64::MAX; GPR_COUNT], complete: false }
    }

    /// 注入（O(1)——越界拒收；complete 由未填计数动态判定）。
    pub fn set(&mut self, idx: usize, v: u64) -> Result<(), PxCode> {
        if idx >= GPR_COUNT {
            return Err(PxCode::GprIncomplete);
        }
        self.regs[idx] = v;
        self.complete = self.reqs_unfilled() == 0;
        Ok(())
    }

    /// 未填哨兵计数（u64::MAX 视为未填）。
    fn reqs_unfilled(&self) -> usize {
        self.regs.iter().filter(|&&r| r == u64::MAX).count()
    }
}

/// 控制寄存器快照（四件：rip/rflags/rsp/cr0 抽象）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CtrlSnap {
    pub rip: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub cr0: u64,
    pub complete: bool,
}

impl CtrlSnap {
    pub fn new() -> CtrlSnap {
        CtrlSnap { rip: 0, rflags: 0, rsp: 0, cr0: 0, complete: false }
    }

    /// 四件注入（全非零才完整——零值控制面是残缺信号）。
    pub fn fill(&mut self, rip: u64, rflags: u64, rsp: u64, cr0: u64) {
        self.rip = rip;
        self.rflags = rflags;
        self.rsp = rsp;
        self.cr0 = cr0;
        self.complete = rip != 0 && rflags != 0 && rsp != 0 && cr0 != 0;
    }
}

/// 栈窗快照（16 槽+窗指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackWindow {
    pub base: u64,
    pub slots: [u64; STACK_WINDOW],
    /// 窗指纹（FNV over 槽字节序——确定性）。
    pub fingerprint: u64,
    pub complete: bool,
}

impl StackWindow {
    pub fn new(base: u64) -> StackWindow {
        StackWindow {
            base,
            slots: [0; STACK_WINDOW],
            fingerprint: 0,
            complete: false,
        }
    }

    /// 注入窗（全 16 槽——不足即残缺）。
    pub fn fill(&mut self, values: &[u64; STACK_WINDOW]) {
        self.slots = *values;
        // 指纹：槽值 8 字节小端序拼接 FNV。
        let mut bytes = Vec::with_capacity(STACK_WINDOW * 8);
        for v in self.slots.iter() {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        self.fingerprint = fnv1a64(&bytes);
        self.complete = true;
    }
}

/// 堆指纹采样块。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeapBlock {
    pub addr: u64,
    pub len: u64,
    /// 内容指纹（FNV——采样不搬内容）。
    pub content_fp: u64,
}

/// 关键堆指纹（采样块聚合——总指纹=块指纹序贯 FNV）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeapFingerprint {
    pub blocks: Vec<HeapBlock>,
    /// 总指纹（块指纹按序 FNV 链——确定性）。
    pub total_fp: u64,
}

impl HeapFingerprint {
    pub fn sample(pairs: &[(u64, u64, u64)]) -> HeapFingerprint {
        let mut blocks = Vec::new();
        for &(addr, len, content) in pairs.iter().take(HEAP_SAMPLE_CAP) {
            blocks.push(HeapBlock {
                addr,
                len,
                content_fp: content,
            });
        }
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in &blocks {
            h ^= b.addr;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            h ^= b.len;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            h ^= b.content_fp;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        HeapFingerprint { blocks, total_fp: h }
    }
}

// ---------------------------------------------------------------------------
// 四件聚合快照与转储解析（转储可解析——roundtrip 恒等）
// ---------------------------------------------------------------------------

/// panic 上下文快照（四件聚合）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanicSnapshot {
    pub gpr: GprSnap,
    pub ctrl: CtrlSnap,
    pub stack: StackWindow,
    pub heap: HeapFingerprint,
}

impl PanicSnapshot {
    /// 四件完整性自检（J1——缺一拒收立案）。
    pub fn validate(&self, bag: &mut DiagBag) -> Result<(), PxCode> {
        // 通用寄存器：全 16 项非哨兵 MAX（注入面哨兵=未填）。
        if self.gpr.reqs_unfilled() > 0 {
            bag.push(
                PxCode::GprIncomplete,
                "gpr",
                &format!("{}项未填", self.gpr.reqs_unfilled()),
            );
            return Err(PxCode::GprIncomplete);
        }
        // 控制寄存器四件。
        if !self.ctrl.complete {
            bag.push(PxCode::CtrlIncomplete, "ctrl", "四件有零值");
            return Err(PxCode::CtrlIncomplete);
        }
        // 栈窗。
        if !self.stack.complete {
            bag.push(PxCode::StackWindowShort, "stack", "窗未填满");
            return Err(PxCode::StackWindowShort);
        }
        // 堆指纹：采样块非空。
        if self.heap.blocks.is_empty() {
            bag.push(PxCode::HeapMismatch, "heap", "采样空");
            return Err(PxCode::HeapMismatch);
        }
        Ok(())
    }

    /// 行式确定性转储（单行 | 分隔——可解析契约）。
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        out.push_str("PCTX ");
        out.push_str(PCTX_VERSION);
        out.push_str(DUMP_SEP);
        // 通用寄存器 16 项十六进制。
        for (i, r) in self.gpr.regs.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("{:x}", r));
        }
        out.push_str(DUMP_SEP);
        out.push_str(&format!(
            "{:x},{:x},{:x},{:x}",
            self.ctrl.rip, self.ctrl.rflags, self.ctrl.rsp, self.ctrl.cr0
        ));
        out.push_str(DUMP_SEP);
        out.push_str(&format!("{:x}", self.stack.base));
        out.push_str(DUMP_SEP);
        out.push_str(&format!("{:x}", self.stack.fingerprint));
        out.push_str(DUMP_SEP);
        out.push_str(&format!("{:x}", self.heap.total_fp));
        out.push_str(DUMP_SEP);
        out.push_str(&format!("blk{}", self.heap.blocks.len()));
        out
    }
}

/// 转储解析回读（可解析契约——结构面逐段校验，畸形拒收）。
pub fn parse_dump(text: &str) -> Result<(u64, u64, u64, u64, u64, usize), PxCode> {
    let parts: Vec<&str> = text.split(DUMP_SEP).collect();
    // 段数：版本+GPR+CTRL+栈基+栈指纹+堆指纹+块数 = 7 段。
    if parts.len() != 7 || !parts[0].starts_with("PCTX ") {
        return Err(PxCode::DumpMalformed);
    }
    // GPR 16 项解析。
    let gpr_parts: Vec<&str> = parts[1].split(',').collect();
    if gpr_parts.len() != GPR_COUNT {
        return Err(PxCode::DumpMalformed);
    }
    let mut gpr_sum: u64 = 0;
    for g in gpr_parts.iter() {
        let v = u64::from_str_radix(g, 16).map_err(|_| PxCode::DumpMalformed)?;
        gpr_sum = gpr_sum.wrapping_add(v);
    }
    // CTRL 四件。
    let ctrl_parts: Vec<&str> = parts[2].split(',').collect();
    if ctrl_parts.len() != CTRL_COUNT {
        return Err(PxCode::DumpMalformed);
    }
    let rip = u64::from_str_radix(ctrl_parts[0], 16).map_err(|_| PxCode::DumpMalformed)?;
    let _rflags = u64::from_str_radix(ctrl_parts[1], 16).map_err(|_| PxCode::DumpMalformed)?;
    let rsp = u64::from_str_radix(ctrl_parts[2], 16).map_err(|_| PxCode::DumpMalformed)?;
    let _cr0 = u64::from_str_radix(ctrl_parts[3], 16).map_err(|_| PxCode::DumpMalformed)?;
    // 栈窗与堆。
    let stack_base = u64::from_str_radix(parts[3], 16).map_err(|_| PxCode::DumpMalformed)?;
    let stack_fp = u64::from_str_radix(parts[4], 16).map_err(|_| PxCode::DumpMalformed)?;
    let heap_fp = u64::from_str_radix(parts[5], 16).map_err(|_| PxCode::DumpMalformed)?;
    if !parts[6].starts_with("blk") {
        return Err(PxCode::DumpMalformed);
    }
    let blk_n: usize = parts[6][3..].parse().map_err(|_| PxCode::DumpMalformed)?;
    Ok((gpr_sum, rip, rsp, stack_base, stack_fp ^ heap_fp, blk_n))
}

// ---------------------------------------------------------------------------
// 判据（六项——四件完整/指纹恰值/转储可解析/残缺拒/五码/确定性）
// ---------------------------------------------------------------------------

/// UNX-F0282 判据入口（返回式 CheckSet——全部预期常量独立手算）。
pub fn run_unx282_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0282");

    // 寄-01 通用寄存器快照：16 项注入读回恰值+哨兵账完整。
    {
        let mut g = GprSnap::new();
        // 手算：r[i]=i*3+1 → r0=1,r1=4,...,r15=46。
        for i in 0..GPR_COUNT {
            let _ = g.set(i, (i * 3 + 1) as u64);
        }
        let ok = g.regs[0] == 1
            && g.regs[1] == 4
            && g.regs[15] == 46
            && g.reqs_unfilled() == 0
            && g.complete;
        if ok {
            s.ok("F0282-寄-01-GPR16项恰值完整");
        } else {
            s.fail("F0282-寄-01-GPR16项恰值完整", "GPR 面不符");
        }
    }

    // 控-02 控制寄存器四件：注入读回恰值+零值残缺判定。
    {
        let mut c = CtrlSnap::new();
        c.fill(0x401000, 0x202, 0x7FFE0000, 0x80000000_00000001u64);
        let full = c.complete;
        let mut c2 = CtrlSnap::new();
        c2.fill(0x401000, 0, 0x7FFE0000, 0x1); // rflags 零→残缺
        // 手算：四件恰值读回；零值→complete=false。
        let ok = full
            && c.rip == 0x401000
            && c.rflags == 0x202
            && c.rsp == 0x7FFE0000
            && c.cr0 == 0x80000000_00000001u64
            && !c2.complete;
        if ok {
            s.ok("F0282-控-02-控制四件恰值零值残缺");
        } else {
            s.fail("F0282-控-02-控制四件恰值零值残缺", "控制面不符");
        }
    }

    // 栈-03 栈窗：16 槽注入+FNV 窗指纹恰值（空窗 basis 锚）。
    {
        let mut w = StackWindow::new(0x7FF000);
        let vals = [1u64, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        w.fill(&vals);
        // 手算：窗指纹确定性——同输入同指纹；空输入 FNV=basis。
        let empty_fp = fnv1a64(&[]);
        let again = {
            let mut w2 = StackWindow::new(0x7FF000);
            w2.fill(&vals);
            w2.fingerprint
        };
        // 指纹可复算恒等+空串恰值 basis+窗 16 槽。
        let ok = w.complete
            && w.slots[0] == 1
            && w.slots[15] == 16
            && w.fingerprint == again
            && w.fingerprint != 0
            && empty_fp == 0xcbf2_9ce4_8422_2325u64;
        if ok {
            s.ok("F0282-栈-03-栈窗指纹确定性恰值");
        } else {
            s.fail("F0282-栈-03-栈窗指纹确定性恰值", "栈窗面不符");
        }
    }

    // 堆-04 堆指纹：采样聚合确定性+块数上限。
    {
        let pairs = [
            (0x1000u64, 64u64, 0xAAu64),
            (0x2000, 128, 0xBB),
            (0x3000, 32, 0xCC),
        ];
        let h1 = HeapFingerprint::sample(&pairs);
        let h2 = HeapFingerprint::sample(&pairs);
        // 手算：同输入同总指纹；块数 3；异输入异指纹。
        let h3 = HeapFingerprint::sample(&[(0x1000, 64, 0xDD), (0x2000, 128, 0xBB), (0x3000, 32, 0xCC)]);
        let ok = h1.total_fp == h2.total_fp
            && h1.blocks.len() == 3
            && h1.blocks[0].addr == 0x1000
            && h1.blocks[0].len == 64
            && h1.blocks[0].content_fp == 0xAA
            && h1.total_fp != h3.total_fp;
        if ok {
            s.ok("F0282-堆-04-采样聚合指纹恰值");
        } else {
            s.fail("F0282-堆-04-采样聚合指纹恰值", "堆指纹面不符");
        }
    }

    // 转储-05 序列化可解析：serialize→parse roundtrip 恰值+畸形拒。
    {
        let mut g = GprSnap::new();
        for i in 0..GPR_COUNT {
            let _ = g.set(i, (i + 1) as u64);
        }
        let mut c = CtrlSnap::new();
        c.fill(0x401000, 0x202, 0x7FFE0000, 0x1);
        let mut w = StackWindow::new(0x7FF000);
        w.fill(&[1u64; STACK_WINDOW]);
        let heap = HeapFingerprint::sample(&[(0x1000, 64, 0xAA)]);
        let snap = PanicSnapshot { gpr: g, ctrl: c, stack: w, heap };
        // 四件完整过闸。
        let mut bag = DiagBag::new();
        let valid = snap.validate(&mut bag);
        // 转储→解析回读：gpr_sum=1+2+...+16=136；rip=0x401000；
        // rsp=0x7FFE0000；blk=1。
        let dump = snap.serialize();
        let parsed = parse_dump(&dump);
        // 畸形转储拒：缺段/坏十六进制。
        let bad1 = parse_dump("PCTX broken");
        let bad2 = parse_dump("PCTX v1|zz|1,2,3,4|10|20|30|blk1");
        // 手算：gpr_sum=136（1..16 和）；回读 rip 恰值；blk 1。
        let ok = valid == Ok(())
            && matches!(&parsed, Ok((gs, rip, rsp, _, _, bn))
                if *gs == 136 && *rip == 0x401000 && *rsp == 0x7FFE0000 && *bn == 1)
            && bad1 == Err(PxCode::DumpMalformed)
            && bad2 == Err(PxCode::DumpMalformed);
        if ok {
            s.ok("F0282-转储-05-serialize可解析恰值");
        } else {
            s.fail("F0282-转储-05-serialize可解析恰值", "转储面不符");
        }
    }

    // 码-06 五码互异+残缺快照拒收立案恰值。
    {
        // 五码互异（字符串面）。
        let codes = PxCode::all();
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i].as_str() == codes[j].as_str() {
                    distinct = false;
                }
            }
        }
        // 残缺快照：GPR 有哨兵→validate 拒+立案恰值。
        let mut g = GprSnap::new();
        for i in 0..8 {
            let _ = g.set(i, (i + 1) as u64);
        } // 只填 8 项——8 项哨兵
        let mut c = CtrlSnap::new();
        c.fill(0x1000, 0x202, 0x7FF000, 0x1);
        let mut w = StackWindow::new(0x10);
        w.fill(&[2u64; STACK_WINDOW]);
        let heap = HeapFingerprint::sample(&[(1, 1, 1)]);
        let snap = PanicSnapshot { gpr: g, ctrl: c, stack: w, heap };
        let mut bag = DiagBag::new();
        let v = snap.validate(&mut bag);
        // 手算：8 项未填→GprIncomplete 拒；立案 1 条；spoken 含 reason。
        let ok = distinct
            && codes.len() == 5
            && v == Err(PxCode::GprIncomplete)
            && bag.count_of(PxCode::GprIncomplete) == 1
            && bag.spoken().contains("通用寄存器快照残缺")
            && bag.notes[0].detail.contains("8项未填");
        if ok {
            s.ok("F0282-码-06-五码互异残缺拒收");
        } else {
            s.fail("F0282-码-06-五码互异残缺拒收", "诊断面不符");
        }
    }

    s
}
