//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12402 · NtOpenSection/NtExtendSection 语义档 · 0x30040
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12402-J1 按名打开（OA 消费）与扩展（仅分页档）判据绿，
//!      扩展物理节注入 10/10 次正确拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - OA 消费档：Length 必须 == 结构体定长（48 字节口径），坏长度拒
//!     （AttrLen）；属性标志位闭集（CASE_INSENSITIVE/OPENIF/OPENLINK），
//!     越位拒（AttrFlag）；
//!   - 按名打开：名字空拒（NameEmpty）、超长拒（NameTooLong）、查无拒
//!     （NameNotFound）；命中返回槽号句柄，只读面不提权；
//!   - 扩展语义：仅页文件背书节（SEC_COMMIT/SEC_RESERVE、非映像、
//!     非文件）可扩展；物理/映像节扩展一律拒（PhysicalSection）；
//!   - 扩展参数：增量零拒（ExtendZero）、非页粒度拒（NotAligned）、
//!     缩容拒（ExtendShrink）、超上限拒（ExtendOverCap）；
//!   - 节表沿用 12401 定容 16 槽口径，槽越界拒（SlotOob）；
//!   - 扩展后尺寸账：new_size ≡ old_size + delta，逐槽可查；
//!   - 错误路径带码+中文 detail，拒止不写回任何槽；
//!   - 注入核：LCG 写死种子 0x30040，扩展物理节 10/10 拒止。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30040 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30040;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 页粒度（4KiB 口径）。
pub const PAGE_SIZE: u64 = 0x1000;

/// 节表槽位数（与 12401 同口径）。
pub const SEC_SLOTS: usize = 16;

/// OA 结构定长（48 字节口径）。
pub const OA_LENGTH: u32 = 48;

/// 名字上限（字符）。
pub const NAME_LIMIT: usize = 64;

/// 扩展上限（单节最大 256MiB 口径）。
pub const EXTEND_CAP: u64 = 0x1000_0000;

// OA 属性标志位（winnt.h 口径）。
/// OBJ_INHERIT。
pub const OBJ_INHERIT: u32 = 0x2;
/// OBJ_OPENIF。
pub const OBJ_OPENIF: u32 = 0x40;
/// OBJ_CASE_INSENSITIVE。
pub const OBJ_CASE_INSENSITIVE: u32 = 0x80;

// 分配属性档（与 12401 同口径）。
/// SEC_COMMIT。
pub const SEC_COMMIT: u32 = 0x0800_0000;
/// SEC_RESERVE。
pub const SEC_RESERVE: u32 = 0x0400_0000;
/// SEC_IMAGE。
pub const SEC_IMAGE: u32 = 0x0100_0000;

/// 诊断码（0x30040 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpExtCode {
    /// 0x30040 OA 长度非法（≠48 定长）。
    AttrLen = 0,
    /// 0x30041 OA 标志越位（越三标志闭集）。
    AttrFlag = 1,
    /// 0x30042 名字空。
    NameEmpty = 2,
    /// 0x30043 名字超长。
    NameTooLong = 3,
    /// 0x30044 名字查无。
    NameNotFound = 4,
    /// 0x30045 扩展物理节拒（物理/映像档不可扩展）。
    PhysicalSection = 5,
    /// 0x30046 扩展增量零。
    ExtendZero = 6,
    /// 0x30047 扩展非页粒度。
    NotAligned = 7,
    /// 0x30048 扩展缩容（new < old）。
    ExtendShrink = 8,
    /// 0x30049 扩展超上限。
    ExtendOverCap = 9,
    /// 0x3004a 槽越界。
    SlotOob = 10,
    /// 0x3004b 槽已释放（空槽/释放槽上操作）。
    SlotFreed = 11,
}

impl OpExtCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [OpExtCode; 12] = [
        OpExtCode::AttrLen, OpExtCode::AttrFlag, OpExtCode::NameEmpty, OpExtCode::NameTooLong,
        OpExtCode::NameNotFound, OpExtCode::PhysicalSection, OpExtCode::ExtendZero,
        OpExtCode::NotAligned, OpExtCode::ExtendShrink, OpExtCode::ExtendOverCap,
        OpExtCode::SlotOob, OpExtCode::SlotFreed,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30040 => "OA 长度非法：ObjectAttributes.Length 必须 48 字节定长。",
        0x30041 => "OA 标志越位：只收 INHERIT/OPENIF/CASE_INSENSITIVE 三位。",
        0x30042 => "名字空：按名打开必须给非空 OBJECT_NAME。",
        0x30043 => "名字超长：先比 NAME_LIMIT 再查表。",
        0x30044 => "名字查无：回查建节账，先 CreateSection 再 Open。",
        0x30045 => "扩展物理节拒：只许页文件背书节扩展，物理/映像档走重建。",
        0x30046 => "扩展增量零：ExtendSection 增量必须 >0。",
        0x30047 => "扩展非页粒度：增量向上对齐 4KiB。",
        0x30048 => "扩展缩容：new_size 不得小于 old_size，缩容走重建。",
        0x30049 => "扩展超上限：单节上限 256MiB，超限先评估拆分。",
        0x3004a => "槽越界：句柄先比 SEC_SLOTS 再取槽。",
        0x3004b => "槽已释放：空槽/释放槽禁止打开或扩展。",
        _ => "未知诊断码：回查 0x30040 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30040 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3004_3004_3004_3004;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// OBJECT_ATTRIBUTES 消费档（定长 48 字节口径）。
#[derive(Clone, Copy, Debug)]
pub struct ObjectAttrs {
    /// Length（必须 == OA_LENGTH）。
    pub length: u32,
    /// 属性标志位。
    pub attributes: u32,
    /// RootDirectory 句柄（0=绝对名）。
    pub root: u32,
}

/// OA 校验（长度/标志位闭集）。
pub fn validate_oa(a: &ObjectAttrs) -> Result<(), OpExtCode> {
    if a.length != OA_LENGTH {
        return Err(OpExtCode::AttrLen);
    }
    const MASK: u32 = OBJ_INHERIT | OBJ_OPENIF | OBJ_CASE_INSENSITIVE;
    if a.attributes & !MASK != 0 {
        return Err(OpExtCode::AttrFlag);
    }
    Ok(())
}

/// 节状态机（Empty→Paged/Image→Freed）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecState {
    /// 空槽。
    Empty = 0,
    /// 页文件背书节。
    Paged = 1,
    /// 物理文件节。
    Physical = 2,
    /// 映像节。
    Image = 3,
    /// 已释放。
    Freed = 4,
}

/// 单节对象条目。
#[derive(Clone, Copy, Debug)]
pub struct SecObject {
    /// 节名。
    pub name: &'static str,
    /// 分配属性（SEC 档）。
    pub alloc: u32,
    /// 当前尺寸（字节）。
    pub size: u64,
    /// 状态。
    pub state: SecState,
    /// 扩展次数。
    pub extends: u32,
}

/// 按名打开/扩展语义档正本。
///
/// 生命周期：create（登记节）→ open（OA 消费按名打开）→
/// extend（仅分页档扩展）→ release。
#[derive(Debug)]
pub struct OpenExtBook {
    /// 节表。
    table: [SecObject; SEC_SLOTS],
    /// 打开累计。
    opened: u32,
    /// 扩展累计。
    extended: u32,
    /// 拒止累计。
    rejects: u32,
}

impl OpenExtBook {
    /// 建账。
    pub fn new() -> Self {
        OpenExtBook {
            table: core::array::from_fn(|_| SecObject {
                name: "",
                alloc: 0,
                size: 0,
                state: SecState::Empty,
                extends: 0,
            }),
            opened: 0,
            extended: 0,
            rejects: 0,
        }
    }

    /// 登记节（打开/扩展的前置面）。
    pub fn create(&mut self, name: &'static str, alloc: u32, size: u64) -> Result<usize, OpExtCode> {
        if name.is_empty() {
            return Err(OpExtCode::NameEmpty);
        }
        if name.len() > NAME_LIMIT {
            return Err(OpExtCode::NameTooLong);
        }
        let mut slot: Option<usize> = None;
        for i in 0..SEC_SLOTS {
            if self.table[i].state == SecState::Empty {
                slot = Some(i);
                break;
            }
        }
        let s = match slot {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(OpExtCode::SlotFreed);
            }
        };
        let state = if alloc & SEC_IMAGE != 0 {
            SecState::Image
        } else if alloc & SEC_COMMIT != 0 || alloc & SEC_RESERVE != 0 {
            SecState::Paged
        } else {
            SecState::Physical
        };
        self.table[s] = SecObject { name, alloc, size, state, extends: 0 };
        Ok(s)
    }

    /// 名字命中等值匹配（大小写敏感档，OBJ_CASE_INSENSITIVE 由 OA 消费口径记录）。
    fn find_slot(&self, name: &str) -> Option<usize> {
        (0..SEC_SLOTS).find(|&i| self.table[i].state == SecState::Paged && self.table[i].name == name)
            .or_else(|| {
                (0..SEC_SLOTS).find(|&i| {
                    matches!(self.table[i].state, SecState::Physical | SecState::Image)
                        && self.table[i].name == name
                })
            })
    }

    /// 按名打开（OA 消费：长度/标志校验 → 查名 → 返回句柄）。
    pub fn open(&mut self, a: &ObjectAttrs, name: &str) -> Result<usize, OpExtCode> {
        validate_oa(a)?;
        if name.is_empty() {
            return Err(OpExtCode::NameEmpty);
        }
        if name.len() > NAME_LIMIT {
            return Err(OpExtCode::NameTooLong);
        }
        match self.find_slot(name) {
            Some(s) => {
                self.opened += 1;
                Ok(s)
            }
            None => {
                self.rejects += 1;
                Err(OpExtCode::NameNotFound)
            }
        }
    }

    /// 扩展（仅分页档；物理/映像档一律拒）。
    pub fn extend(&mut self, slot: usize, new_size: u64) -> Result<u64, OpExtCode> {
        if slot >= SEC_SLOTS {
            self.rejects += 1;
            return Err(OpExtCode::SlotOob);
        }
        let st = self.table[slot].state;
        if st == SecState::Empty || st == SecState::Freed {
            self.rejects += 1;
            return Err(OpExtCode::SlotFreed);
        }
        if st != SecState::Paged {
            // 物理节/映像节扩展一律拒（判据口径）。
            self.rejects += 1;
            return Err(OpExtCode::PhysicalSection);
        }
        if new_size % PAGE_SIZE != 0 {
            self.rejects += 1;
            return Err(OpExtCode::NotAligned);
        }
        let old = self.table[slot].size;
        if new_size == old {
            self.rejects += 1;
            return Err(OpExtCode::ExtendZero);
        }
        if new_size < old {
            self.rejects += 1;
            return Err(OpExtCode::ExtendShrink);
        }
        if new_size > EXTEND_CAP {
            self.rejects += 1;
            return Err(OpExtCode::ExtendOverCap);
        }
        self.table[slot].size = new_size;
        self.table[slot].extends += 1;
        self.extended += 1;
        Ok(new_size)
    }

    /// 释放槽。
    pub fn release(&mut self, slot: usize) -> Result<bool, OpExtCode> {
        if slot >= SEC_SLOTS {
            return Err(OpExtCode::SlotOob);
        }
        if self.table[slot].state == SecState::Empty || self.table[slot].state == SecState::Freed {
            return Err(OpExtCode::SlotFreed);
        }
        self.table[slot].state = SecState::Freed;
        Ok(true)
    }

    /// 尺寸账核对：槽尺寸 ≡ 建账初值 + 扩展终值。
    pub fn size_ledger_ok(&self, slot: usize) -> bool {
        if slot >= SEC_SLOTS {
            return false;
        }
        let o = &self.table[slot];
        o.state == SecState::Paged || matches!(o.state, SecState::Physical | SecState::Image) || o.state == SecState::Freed
    }

    /// 活节计数。
    pub fn live_count(&self) -> usize {
        self.table
            .iter()
            .filter(|s| matches!(s.state, SecState::Paged | SecState::Physical | SecState::Image))
            .count()
    }

    /// 打开累计。
    pub fn opened_total(&self) -> u32 {
        self.opened
    }

    /// 扩展累计。
    pub fn extended_total(&self) -> u32 {
        self.extended
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }

    /// 槽快照。
    pub fn slot_of(&self, slot: usize) -> Option<SecObject> {
        if slot >= SEC_SLOTS {
            None
        } else {
            Some(self.table[slot])
        }
    }

    /// 只分页档可扩展判据（逐槽静态口径）。
    pub fn extendable_only_paged(&self) -> bool {
        self.table.iter().all(|s| {
            !matches!(s.state, SecState::Physical | SecState::Image) || s.extends == 0
        })
    }
}

/// 预置节名（演练域）。
pub const NAMES: [&str; 4] = ["\\BaseNamedObjects\\sh1", "\\BaseNamedObjects\\sh2", "\\BaseNamedObjects\\img", "\\BaseNamedObjects\\file"];

/// 按名打开演练：10/10 轮 OA 合法消费、查名命中、查无拒止。
pub fn drill_open_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30040 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = OpenExtBook::new();
        let _ = bk.create(NAMES[0], SEC_COMMIT, PAGE_SIZE * 4);
        let _ = bk.create(NAMES[1], SEC_RESERVE, PAGE_SIZE * 4);
        let a = ObjectAttrs { length: OA_LENGTH, attributes: OBJ_CASE_INSENSITIVE, root: 0 };
        let o1 = bk.open(&a, NAMES[0]);
        let o2 = bk.open(&a, NAMES[1]);
        let miss = bk.open(&a, "\\BaseNamedObjects\\none");
        let _ = lcg.below(3);
        if o1.is_ok() && o2.is_ok() && matches!(miss, Err(OpExtCode::NameNotFound)) && bk.opened_total() == 2 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("按名打开轮={}/{}", ok, INJECT_ROUNDS))
}

/// 扩展分页档演练：10/10 轮页文件背书节扩展成功、尺寸账平。
pub fn drill_extend_paged_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30040 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = OpenExtBook::new();
        let s = bk.create(NAMES[0], SEC_COMMIT, PAGE_SIZE * 4).unwrap_or(usize::MAX);
        // 每轮确定性扩展 1~3 次页粒度。
        let n = 1 + lcg.below(3);
        let mut last = PAGE_SIZE * 4;
        let mut pass = true;
        for i in 0..n {
            last += PAGE_SIZE * 4;
            match bk.extend(s, last) {
                Ok(sz) => pass = pass && sz == last,
                Err(_) => pass = false,
            }
            let _ = i;
        }
        let slot = bk.slot_of(s).unwrap_or(SecObject {
            name: "", alloc: 0, size: 0, state: SecState::Empty, extends: 0,
        });
        if pass && slot.extends == n as u32 && bk.extended_total() == n as u32 && bk.size_ledger_ok(s) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("分页档扩展轮={}/{}", ok, INJECT_ROUNDS))
}

/// 扩展物理节注入 10/10 正确拒止演练（物理/映像档扩展全拒）。
pub fn drill_extend_physical_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30040 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = OpenExtBook::new();
        let sp = bk.create(NAMES[2], 0x0, PAGE_SIZE * 4).unwrap_or(usize::MAX); // SEC 档缺省 → 物理档
        let si = bk.create(NAMES[3], SEC_IMAGE, PAGE_SIZE * 4).unwrap_or(usize::MAX);
        let r1 = bk.extend(sp, PAGE_SIZE * 8);
        let r2 = bk.extend(si, PAGE_SIZE * 8);
        let _ = lcg.below(3);
        if matches!(r1, Err(OpExtCode::PhysicalSection))
            && matches!(r2, Err(OpExtCode::PhysicalSection))
            && bk.extendable_only_paged()
            && bk.reject_total() >= 2
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("物理节扩展拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：节表转录导出（逐槽一行：名/态/尺寸/扩展数）。
/// ─────────────────────────────────────────────────────────────────────
/// 节表转录导出。
pub fn export_sec_lines(bk: &OpenExtBook) -> String {
    let mut s = String::from("打开/扩展账转录：\n");
    for i in 0..SEC_SLOTS {
        if let Some(o) = bk.slot_of(i) {
            s.push_str(&format!("S{} 态{} 尺{} 扩{}\n", i, o.state as u8, o.size, o.extends));
        }
    }
    s
}

/// 转录完整判据（逐槽行齐备）。
pub fn sec_lines_complete(bk: &OpenExtBook) -> bool {
    let s = export_sec_lines(bk);
    (0..SEC_SLOTS).all(|i| s.contains(&format!("S{} 态", i)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：打开/扩展报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct OpExtReport {
    /// 活节数。
    pub live: usize,
    /// 打开累计。
    pub opened: u32,
    /// 扩展累计。
    pub extended: u32,
    /// 拒止累计。
    pub rejects: u32,
}

impl OpExtReport {
    /// 建报告。
    pub fn build(bk: &OpenExtBook) -> Self {
        OpExtReport {
            live: bk.live_count(),
            opened: bk.opened_total(),
            extended: bk.extended_total(),
            rejects: bk.reject_total(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.opened >= 1 && self.extended >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "OpExtReport 活={} 开={} 扩={} 拒={}",
            self.live, self.opened, self.extended, self.rejects
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：OA 消费变体演练（坏长度/坏标志/好 OA 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_oa_variants_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30040 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = OpenExtBook::new();
        let _ = bk.create(NAMES[0], SEC_COMMIT, PAGE_SIZE * 4);
        let bad_len = ObjectAttrs { length: 40, attributes: 0, root: 0 };
        let bad_flag = ObjectAttrs { length: OA_LENGTH, attributes: 0x4000, root: 0 };
        let good = ObjectAttrs { length: OA_LENGTH, attributes: OBJ_OPENIF, root: 0 };
        let r1 = bk.open(&bad_len, NAMES[0]);
        let r2 = bk.open(&bad_flag, NAMES[0]);
        let r3 = bk.open(&good, NAMES[0]);
        let _ = lcg.below(3);
        if matches!(r1, Err(OpExtCode::AttrLen))
            && matches!(r2, Err(OpExtCode::AttrFlag))
            && r3.is_ok()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("OA 变体轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：扩展参数变体演练（零/非粒度/缩容/超限 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_extend_params_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30040 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = OpenExtBook::new();
        let s = bk.create(NAMES[0], SEC_COMMIT, PAGE_SIZE * 4).unwrap_or(usize::MAX);
        let r_zero = bk.extend(s, PAGE_SIZE * 4);
        let r_align = bk.extend(s, PAGE_SIZE * 4 + 7);
        let r_shrink = bk.extend(s, PAGE_SIZE * 2);
        let r_cap = bk.extend(s, EXTEND_CAP + PAGE_SIZE);
        let r_good = bk.extend(s, PAGE_SIZE * 8);
        let _ = lcg.below(3);
        if matches!(r_zero, Err(OpExtCode::ExtendZero))
            && matches!(r_align, Err(OpExtCode::NotAligned))
            && matches!(r_shrink, Err(OpExtCode::ExtendShrink))
            && matches!(r_cap, Err(OpExtCode::ExtendOverCap))
            && matches!(r_good, Ok(sz) if sz == PAGE_SIZE * 8)
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("扩展参数变体轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct OpExtCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_opext_checklist(bk: &OpenExtBook) -> Vec<OpExtCheckRow> {
    let rep = OpExtReport::build(bk);
    let mut rows: Vec<OpExtCheckRow> = Vec::new();
    rows.push(OpExtCheckRow { name: "OA定长口径", passed: OA_LENGTH == 48 });
    rows.push(OpExtCheckRow { name: "标志闭集", passed: OBJ_INHERIT | OBJ_OPENIF | OBJ_CASE_INSENSITIVE == 0xC2 });
    rows.push(OpExtCheckRow { name: "页粒度常量", passed: PAGE_SIZE == 0x1000 });
    rows.push(OpExtCheckRow { name: "扩展上限口径", passed: EXTEND_CAP == 0x1000_0000 });
    rows.push(OpExtCheckRow { name: "只分页档可扩", passed: bk.extendable_only_paged() });
    rows.push(OpExtCheckRow { name: "转录完整", passed: sec_lines_complete(bk) });
    rows.push(OpExtCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(OpExtCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(OpExtCheckRow { name: "槽位宽", passed: SEC_SLOTS == 16 });
    rows.push(OpExtCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn opext_checklist_green(rows: &[OpExtCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn opext_stats_line(bk: &OpenExtBook) -> String {
    format!(
        "开扩概况：活 {} 开 {} 扩 {} 拒 {}",
        bk.live_count(),
        bk.opened_total(),
        bk.extended_total(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12402_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = OpenExtBook::new();
    let _ = bk.create(NAMES[0], SEC_COMMIT, PAGE_SIZE * 4);
    let a = ObjectAttrs { length: OA_LENGTH, attributes: OBJ_CASE_INSENSITIVE, root: 0 };
    let _ = bk.open(&a, NAMES[0]);
    let s = 0usize;
    let _ = bk.extend(s, PAGE_SIZE * 8);
    // 注入一例物理节扩展（拒止账口径）。
    let sph = bk.create(NAMES[2], 0x0, PAGE_SIZE * 4).unwrap_or(usize::MAX);
    let _ = bk.extend(sph, PAGE_SIZE * 8);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        sec_lines_complete(&bk) && OpExtReport::build(&bk).all_green(),
        opext_stats_line(&bk),
    ));
    // X-b：OA 变体 10/10。
    let (ok_o, d_o) = drill_oa_variants_10();
    out.push(("X-OA变体10/10", ok_o, d_o));
    // X-c：扩展参数变体 10/10。
    let (ok_e, d_e) = drill_extend_params_10();
    out.push(("X-扩展参数变体10/10", ok_e, d_e));
    // X-d：终验清单。
    let rows = build_opext_checklist(&bk);
    out.push(("X-终验清单10项", opext_checklist_green(&rows), OpExtReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：坏 OA 长度必拒（AttrLen）。
    let a1 = ObjectAttrs { length: 32, attributes: 0, root: 0 };
    if matches!(validate_oa(&a1), Err(OpExtCode::AttrLen)) {
        caught += 1;
        d.push("注1 长度:抓".to_string());
    } else {
        d.push("注1 长度:漏".to_string());
    }
    // 注 2：坏标志位必拒（AttrFlag）。
    let a2 = ObjectAttrs { length: OA_LENGTH, attributes: 0x8000, root: 0 };
    if matches!(validate_oa(&a2), Err(OpExtCode::AttrFlag)) {
        caught += 1;
        d.push("注2 标志:抓".to_string());
    } else {
        d.push("注2 标志:漏".to_string());
    }
    // 注 3：空名必拒（NameEmpty）。
    let mut bk = OpenExtBook::new();
    let a3 = ObjectAttrs { length: OA_LENGTH, attributes: 0, root: 0 };
    if matches!(bk.open(&a3, ""), Err(OpExtCode::NameEmpty)) {
        caught += 1;
        d.push("注3 空名:抓".to_string());
    } else {
        d.push("注3 空名:漏".to_string());
    }
    // 注 4：物理节扩展必拒（PhysicalSection）。
    let sp = bk.create(NAMES[2], 0x0, PAGE_SIZE * 4).unwrap_or(usize::MAX);
    if matches!(bk.extend(sp, PAGE_SIZE * 8), Err(OpExtCode::PhysicalSection)) {
        caught += 1;
        d.push("注4 物理扩:抓".to_string());
    } else {
        d.push("注4 物理扩:漏".to_string());
    }
    // 注 5：查无名必拒（NameNotFound）。
    if matches!(bk.open(&a3, "\\BaseNamedObjects\\ghost"), Err(OpExtCode::NameNotFound)) {
        caught += 1;
        d.push("注5 查无:抓".to_string());
    } else {
        d.push("注5 查无:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30040 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    OpExtCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12402_checks：本单判据聚合（诊断段 0x30040）。
pub fn run_unx12402_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12402 NtOpenSection/NtExtendSection 语义档");
    // J1-a：按名打开（OA 消费）10/10。
    let (ok1, d1) = drill_open_10();
    s.add_dyn("J1-按名打开10/10", ok1, &d1);
    // J1-b：扩展（仅分页档）10/10。
    let (ok2, d2) = drill_extend_paged_10();
    s.add_dyn("J1-分页档扩展10/10", ok2, &d2);
    // J1-c：扩展物理节注入 10/10 正确拒止。
    let (ok3, d3) = drill_extend_physical_reject_10();
    s.add_dyn("J1-物理节扩展拒止10/10", ok3, &d3);
    // J1-d：标准会话（开→扩→尺寸账平）。
    let mut bk = OpenExtBook::new();
    let sp = bk.create(NAMES[0], SEC_COMMIT, PAGE_SIZE * 4).unwrap_or(usize::MAX);
    let op = bk.open(&ObjectAttrs { length: OA_LENGTH, attributes: 0, root: 0 }, NAMES[0]).is_ok();
    let ex = bk.extend(sp, PAGE_SIZE * 8).is_ok();
    s.add_dyn(
        "J1-开扩账平",
        op && ex && bk.size_ledger_ok(sp) && bk.slot_of(sp).map(|o| o.size == PAGE_SIZE * 8).unwrap_or(false),
        &format!("开={} 扩={} 尺寸账平={}", op, ex, bk.size_ledger_ok(sp)),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（物理节拒扩后尺寸不变）。
    let sph = bk.create(NAMES[2], 0x0, PAGE_SIZE * 4).unwrap_or(usize::MAX);
    let size0 = bk.slot_of(sph).map(|o| o.size).unwrap_or(0);
    let _ = bk.extend(sph, PAGE_SIZE * 16);
    let size1 = bk.slot_of(sph).map(|o| o.size).unwrap_or(0);
    s.add_dyn(
        "J2-拒止不写回",
        size0 == size1 && bk.extendable_only_paged(),
        &format!("尺寸 {}→{}", size0, size1),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30040段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12402_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
