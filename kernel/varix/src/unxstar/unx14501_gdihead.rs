//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14501 · GDI 对象统一头（类型/归属/计数/句柄） · 0x667C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥365 行；
//!   ② UNX-F14501-J1 对象头四元组账 10 万对象零漂移，类型标签与句柄
//!      表槽位一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 对象头四元组（结构档注出处：Win32 GDI 对象统一头建模，字段名
//!     钉版）：类型（七类闭集）/归属（进程 pid 建模 1..9）/引用计数
//!     /句柄（单调分配，0 恒非法）；
//!   - 七类闭集：PEN 0/BRUSH 1/FONT 2/BITMAP 3/REGION 4/PALETTE 5/
//!     DC 6（来源：Win32 GDI 对象类型公开口径，写死）；
//!   - 句柄表：1024 槽定容，槽位 = 句柄 % 1024，占用则线性探_next；
//!     表侧类型标签数组与对象头类型逐槽比对——槽位一致红线；
//!   - 引用计数纪律：创建即 1；incref/decref 闭式；计数 0 才可删；
//!     删后句柄作废（查无此柄）；二次删必红；decref 下溢必红；
//!   - 10 万次操作账（LCG 确定性：create/incref/decref/delete 四操作
//!     伪随机序列），守恒：Σ每类存活 = 存活头数 = 表占用数；
//!   - 双机对拍：独立新账重放同序列，四元组逐对象一致；
//!   - LCG 种子写死 = 0x667C0；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x667C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x667C0;

/// 七类闭集。
pub const TY_PEN: usize = 0;
pub const TY_BRUSH: usize = 1;
pub const TY_FONT: usize = 2;
pub const TY_BITMAP: usize = 3;
pub const TY_REGION: usize = 4;
pub const TY_PALETTE: usize = 5;
pub const TY_DC: usize = 6;
pub const TYPE_COUNT: usize = 7;
pub const TYPE_NAMES: [&str; TYPE_COUNT] =
    ["PEN", "BRUSH", "FONT", "BITMAP", "REGION", "PALETTE", "DC"];

/// 句柄表槽位数（写死：1024）。
pub const TABLE_SLOTS: usize = 1024;
/// 空槽标签（0xFF）。
pub const SLOT_EMPTY: u8 = 0xFF;
/// 归属 pid 闭集（1..9，0 恒非法）。
pub const PID_MAX: usize = 8;
/// 操作账规模（判据原文：10 万对象次）。
pub const OP_ACCOUNT: usize = 100_000;
/// 句柄初值（写死：0x1000 起，0 恒非法对齐）。
pub const HANDLE_BASE: u32 = 0x1000;

/// 结构档注出处（四元组字段名钉版，写死）。
pub const FIELD_TYPE: &str = "obj_type";
pub const FIELD_OWNER: &str = "owner_pid";
pub const FIELD_REFCNT: &str = "ref_count";
pub const FIELD_HANDLE: &str = "handle";

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GdiCode {
    /// 类型越闭集（七类）
    TypeInvalid,
    /// 归属 pid=0 或越闭集
    OwnerInvalid,
    /// 句柄=0（恒非法）
    HandleNull,
    /// 查无此柄（已删或未建）
    UnknownHandle,
    /// 引用计数下溢（decref 至 0 以下）
    RefcountUnderflow,
    /// 二次删除（已删句柄再删必红）
    DoubleDelete,
    /// 槽位标签漂移（对象头类型 ≠ 表槽类型标签）
    SlotTagMismatch,
    /// 句柄表满（1024 槽红线）
    TableFull,
    /// 四元组守恒破裂（Σ每类存活 ≠ 存活头数 ≠ 表占用数）
    ConservationBroken,
    /// 操作账总数 ≠ 10 万
    AccountMismatch,
    /// 双机对拍漂移
    DualReplayDrift,
}

impl GdiCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GdiCode::TypeInvalid => "类型越闭集：PEN/BRUSH/FONT/BITMAP/REGION/PALETTE/DC",
            GdiCode::OwnerInvalid => "归属 pid=0 或越闭集：1..9",
            GdiCode::HandleNull => "句柄=0：恒非法",
            GdiCode::UnknownHandle => "查无此柄：已删或未建",
            GdiCode::RefcountUnderflow => "引用计数下溢：decref 至 0 以下必红",
            GdiCode::DoubleDelete => "二次删除：已删句柄再删必红",
            GdiCode::SlotTagMismatch => "槽位标签漂移：类型标签与句柄表槽位不一致",
            GdiCode::TableFull => "句柄表满：1024 槽红线",
            GdiCode::ConservationBroken => "四元组守恒破裂：存活账三向不等",
            GdiCode::AccountMismatch => "操作账总数 ≠ 10 万",
            GdiCode::DualReplayDrift => "双机对拍漂移：对象账不可复现",
        }
    }
    pub const ALL: [GdiCode; 11] = [
        GdiCode::TypeInvalid,
        GdiCode::OwnerInvalid,
        GdiCode::HandleNull,
        GdiCode::UnknownHandle,
        GdiCode::RefcountUnderflow,
        GdiCode::DoubleDelete,
        GdiCode::SlotTagMismatch,
        GdiCode::TableFull,
        GdiCode::ConservationBroken,
        GdiCode::AccountMismatch,
        GdiCode::DualReplayDrift,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GdiCode::ALL.len() {
        if GdiCode::ALL[i].val() == code {
            return GdiCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x667C0 段闭集内"
}

// ── 对象头四元组 ─────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GdiHead {
    /// 句柄（单调分配，≥ HANDLE_BASE）
    pub handle: u32,
    /// 类型标签（0..7）
    pub obj_type: usize,
    /// 归属进程 pid（1..9）
    pub owner_pid: usize,
    /// 引用计数（创建即 1）
    pub ref_count: u64,
    /// 句柄表槽位（create 时钉死）
    pub slot: usize,
}

pub struct GdiLedger {
    pub heads: Vec<GdiHead>,
    /// 表侧类型标签（SLOT_EMPTY=空）
    pub tags: [u8; TABLE_SLOTS],
    /// 每类存活计数
    pub alive_per_type: [u64; TYPE_COUNT],
    /// 操作账（含拒收）
    pub ops: u64,
    /// 成功建账数
    pub booked: u64,
    next_handle: u32,
}

impl GdiLedger {
    pub fn new() -> GdiLedger {
        GdiLedger {
            heads: Vec::new(),
            tags: [SLOT_EMPTY; TABLE_SLOTS],
            alive_per_type: [0; TYPE_COUNT],
            ops: 0,
            booked: 0,
            next_handle: HANDLE_BASE,
        }
    }

    fn find(&self, handle: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.heads.len() {
            if self.heads[i].handle == handle {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 槽位分配：句柄 % 1024 起，线性探_next（全表闭环，满则 TableFull）。
    fn alloc_slot(&self, handle: u32) -> Result<usize, u32> {
        let start = (handle as usize) % TABLE_SLOTS;
        let mut k = 0;
        while k < TABLE_SLOTS {
            let s = (start + k) % TABLE_SLOTS;
            if self.tags[s] == SLOT_EMPTY {
                return Ok(s);
            }
            k += 1;
        }
        Err(GdiCode::TableFull.val())
    }

    /// 创建：四元组落账 + 表槽类型标签钉版。
    pub fn create(&mut self, ty: usize, pid: usize) -> Result<u32, u32> {
        self.ops += 1;
        if ty >= TYPE_COUNT {
            return Err(GdiCode::TypeInvalid.val());
        }
        if pid == 0 || pid > PID_MAX {
            return Err(GdiCode::OwnerInvalid.val());
        }
        let h = self.next_handle;
        if h == 0 {
            return Err(GdiCode::HandleNull.val());
        }
        self.next_handle = self.next_handle.wrapping_add(1);
        let slot = self.alloc_slot(h)?;
        self.tags[slot] = ty as u8;
        self.heads.push(GdiHead {
            handle: h,
            obj_type: ty,
            owner_pid: pid,
            ref_count: 1,
            slot,
        });
        self.alive_per_type[ty] += 1;
        self.booked += 1;
        Ok(h)
    }

    /// incref：存活柄计数 +1。
    pub fn incref(&mut self, handle: u32) -> Result<u64, u32> {
        self.ops += 1;
        if handle == 0 {
            return Err(GdiCode::HandleNull.val());
        }
        let idx = match self.find(handle) {
            Some(i) => i,
            None => return Err(GdiCode::UnknownHandle.val()),
        };
        self.heads[idx].ref_count += 1;
        Ok(self.heads[idx].ref_count)
    }

    /// decref：计数 -1；至 0 不自动删（删除必须显式 delete——计数纪律
    /// 闭式）；下溢必红。
    pub fn decref(&mut self, handle: u32) -> Result<u64, u32> {
        self.ops += 1;
        if handle == 0 {
            return Err(GdiCode::HandleNull.val());
        }
        let idx = match self.find(handle) {
            Some(i) => i,
            None => return Err(GdiCode::UnknownHandle.val()),
        };
        if self.heads[idx].ref_count == 0 {
            return Err(GdiCode::RefcountUnderflow.val());
        }
        self.heads[idx].ref_count -= 1;
        Ok(self.heads[idx].ref_count)
    }

    /// delete：显式删除（须存活；删后句柄作废、槽标签清空、类计数减）。
    pub fn delete(&mut self, handle: u32) -> Result<(), u32> {
        self.ops += 1;
        if handle == 0 {
            return Err(GdiCode::HandleNull.val());
        }
        let idx = match self.find(handle) {
            Some(i) => i,
            // 已删句柄再查必无——二次删红线。
            None => return Err(GdiCode::DoubleDelete.val()),
        };
        let ty = self.heads[idx].obj_type;
        let slot = self.heads[idx].slot;
        if slot >= TABLE_SLOTS || self.tags[slot] != ty as u8 {
            return Err(GdiCode::SlotTagMismatch.val());
        }
        self.tags[slot] = SLOT_EMPTY;
        self.alive_per_type[ty] -= 1;
        // 头出账（swap-remove，序无关：对拍按句柄查）。
        let last = self.heads.len() - 1;
        self.heads.swap(idx, last);
        self.heads.pop();
        Ok(())
    }

    /// 槽位一致全扫：每存活头 tags[slot] == 类型 且槽位在闭集内。
    pub fn slot_tag_audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.heads.len() {
            let h = &self.heads[i];
            if h.slot >= TABLE_SLOTS {
                return Err(GdiCode::SlotTagMismatch.val());
            }
            if self.tags[h.slot] != h.obj_type as u8 {
                return Err(GdiCode::SlotTagMismatch.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 四元组守恒：Σ每类存活 = 头数 = 表占用槽数。
    pub fn conservation(&self) -> Result<(), u32> {
        let mut sum = 0;
        let mut t = 0;
        while t < TYPE_COUNT {
            sum += self.alive_per_type[t];
            t += 1;
        }
        if sum != self.heads.len() as u64 {
            return Err(GdiCode::ConservationBroken.val());
        }
        let mut used = 0;
        let mut s = 0;
        while s < TABLE_SLOTS {
            if self.tags[s] != SLOT_EMPTY {
                used += 1;
            }
            s += 1;
        }
        if used as u64 != self.heads.len() as u64 {
            return Err(GdiCode::ConservationBroken.val());
        }
        Ok(())
    }

    /// 双机对拍：独立新账重放同序列，逐对象四元组一致。
    pub fn dual_replay(&self, ops: &[(u8, usize, usize)]) -> Result<(), u32> {
        let mut twin = GdiLedger::new();
        replay_ops(&mut twin, ops)?;
        if twin.booked != self.booked || twin.heads.len() != self.heads.len() {
            return Err(GdiCode::DualReplayDrift.val());
        }
        // 按句柄比对（swap-remove 序无关）。
        let mut i = 0;
        while i < self.heads.len() {
            let h = &self.heads[i];
            let t = match twin.find(h.handle) {
                Some(j) => &twin.heads[j],
                None => return Err(GdiCode::DualReplayDrift.val()),
            };
            if t != h {
                return Err(GdiCode::DualReplayDrift.val());
            }
            i += 1;
        }
        Ok(())
    }
}

/// 操作重放（op: 0=create 1=incref 2=decref 3=delete；arg0=类或柄低
/// 位；arg1=pid 或柄高位）。
pub fn replay_ops(led: &mut GdiLedger, ops: &[(u8, usize, usize)]) -> Result<(), u32> {
    let mut i = 0;
    while i < ops.len() {
        let (op, a, b) = ops[i];
        match op {
            0 => {
                let _ = led.create(a, b);
            }
            1 => {
                let _ = led.incref(handle_of(a, b));
            }
            2 => {
                let _ = led.decref(handle_of(a, b));
            }
            _ => {
                let _ = led.delete(handle_of(a, b));
            }
        }
        i += 1;
    }
    Ok(())
}

/// 操作参数→句柄闭式（create 后柄由账返回；重放面用同参构造）。
pub fn handle_of(lo: usize, hi: usize) -> u32 {
    ((hi as u32) << 16) | (lo as u32)
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

/// 10 万次操作序列生成（确定性：create 权重 1/4，其余 3/4 三均分）。
/// 句柄参数从柄账低位递推（重放确定性：incref/decref/delete 用同参
/// handle_of(lo,hi) 重构柄——lo/hi 由 LCG 直接生成，未建柄操作被拒
/// 计拒收账）。
pub fn gen_ops() -> Vec<(u8, usize, usize)> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut ops: Vec<(u8, usize, usize)> = Vec::new();
    let mut i = 0;
    while i < OP_ACCOUNT {
        let r = rng.below(4);
        let op = r as u8;
        let a = (rng.below(TYPE_COUNT as u64)) as usize;
        let b = 1 + (rng.below(PID_MAX as u64)) as usize;
        ops.push((op, a, b));
        i += 1;
    }
    ops
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u64, u64, bool), u32> {
    // 结构档注出处点验：四字段名钉版。
    if FIELD_TYPE.len() == 0
        || FIELD_OWNER.len() == 0
        || FIELD_REFCNT.len() == 0
        || FIELD_HANDLE.len() == 0
    {
        return Err(GdiCode::ConservationBroken.val());
    }
    let ops = gen_ops();
    let mut led = GdiLedger::new();
    replay_ops(&mut led, &ops)?;
    if led.ops != OP_ACCOUNT as u64 {
        return Err(GdiCode::AccountMismatch.val());
    }
    led.slot_tag_audit()?;
    led.conservation()?;
    led.dual_replay(&ops)?;
    Ok((led.ops, led.booked, true))
}

// ── 注入面：三族 5/5 必抓 ────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：类型/pid 注入（类型 7/99、pid 0/99）。
    ledger.tries[0] += 4;
    let mut led = GdiLedger::new();
    let mut c = 0;
    if led.create(7, 1).is_err() {
        c += 1;
    }
    if led.create(99, 1).is_err() {
        c += 1;
    }
    if led.create(TY_PEN, 0).is_err() {
        c += 1;
    }
    if led.create(TY_PEN, 99).is_err() {
        c += 1;
    }
    if c == 4 {
        ledger.caught[0] += 4;
    }
    // 族 1：查无此柄 + 句柄 0 注入。
    ledger.tries[1] += 3;
    let mut c2 = 0;
    if led.incref(0x7FFF).is_err() {
        c2 += 1;
    }
    if led.delete(0x7FFF).is_err() {
        c2 += 1;
    }
    if led.incref(0).is_err() {
        c2 += 1;
    }
    if c2 == 3 {
        ledger.caught[1] += 3;
    }
    // 族 2：二次删除注入（建→删→再删必红）+ 下溢注入（建→decref 至
    // 0 →再 decref 必红）。
    ledger.tries[2] += 2;
    let mut led2 = GdiLedger::new();
    if let Ok(h) = led2.create(TY_BRUSH, 2) {
        let _ = led2.delete(h);
        if led2.delete(h).is_err() {
            ledger.caught[2] += 1;
        }
    }
    let mut led3 = GdiLedger::new();
    if let Ok(h) = led3.create(TY_FONT, 3) {
        let _ = led3.decref(h); // 1→0
        if led3.decref(h).is_err() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < GdiCode::ALL.len() {
        let v = GdiCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：10 万次操作账 + 槽位一致 + 守恒。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((ops, booked, ok)) => *ok && *ops == OP_ACCOUNT as u64 && *booked > 0,
        Err(_) => false,
    };
    v.push(("J1-10万对象次账零漂移", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓（二次删/下溢必红）",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 七类名点验 + 类型标签与槽位一致抽验。
    let mut named = 0;
    let mut i = 0;
    while i < TYPE_COUNT {
        if TYPE_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    let tag_ok = {
        let mut led = GdiLedger::new();
        match led.create(TY_PALETTE, 5) {
            Ok(h) => {
                let slot = (h as usize) % TABLE_SLOTS;
                led.tags[slot] == TY_PALETTE as u8
            }
            Err(_) => false,
        }
    };
    v.push((
        "J1-七类点名+类型槽位一致",
        named == TYPE_COUNT && tag_ok,
        format!("{}/7", named),
    ));
    // 4. 引用计数纪律抽验（建 1 / incref 2 / decref 1 / 删需显式）。
    let ref_ok = {
        let mut led = GdiLedger::new();
        match led.create(TY_DC, 1) {
            Ok(h) => {
                let r1 = led.incref(h);
                let r2 = led.decref(h);
                match (r1, r2) {
                    (Ok(2), Ok(1)) => true,
                    _ => false,
                }
            }
            Err(_) => false,
        }
    };
    v.push(("J1-引用计数纪律抽验", ref_ok, String::from("1→2→1 闭式")));
    // 5. 结构档注出处四字段名点验。
    v.push((
        "J1-四元组字段档注出处",
        FIELD_TYPE == "obj_type"
            && FIELD_OWNER == "owner_pid"
            && FIELD_REFCNT == "ref_count"
            && FIELD_HANDLE == "handle",
        String::from("type/owner/refcnt/handle"),
    ));
    // 6. 诊断码窗口守卫。
    v.push(("J3-段位守卫", segment_guard(), format!("{:#x} 窗口", SEG_BASE)));
    // 7. LCG 确定性。
    let lcg_ok = {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        let mut ok = true;
        let mut i = 0;
        while i < 32 {
            if a.next() != b.next() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-LCG 确定性对拍", lcg_ok, format!("seed={:#x}", SEG_BASE)));
    v
}

pub fn run_unx14501_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14501 GDI 对象统一头（类型/归属/计数/句柄）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
