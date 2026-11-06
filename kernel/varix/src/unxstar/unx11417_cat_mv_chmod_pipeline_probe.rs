//! UNX-F11417 cat/mv/chmod 组合探针（管道与移动链）
//!
//! 组合探针不只问「单条命令成不成功」，而是问整条链是否仍然指向同一个对象：
//! `cat` 产出数据流，`mv` 沿移动链把同一个对象的名字一段段推进，`chmod` 必须
//! 落在链的末端。三条约束缺一不可：
//!
//! 1. **阶段次序**：`cat` 只能是第一段；`mv` 可以连续多段；`chmod` 只能收尾
//!    且只收一次。收尾之后再追加任何阶段都算乱序。
//! 2. **路径真值**：`mv` 的源必须等于当前链端；`chmod` 的目标也必须等于链端。
//!    旧名字虽然曾存在，但它已经从活集里消失，不能再当作目标。
//! 3. **移动回环**：链的历史里出现过的名字不许再次成为目的，否则对象会绕回
//!    起点，`mv a b; mv b a` 这种回环一律 `ECYCLE`。
//!
//! 另外 `cat` 的字节数为零表示上游断供：后续 `mv`/`chmod` 即使路径正确也
//! 必须 `EFLOW`，因为链上没有可搬的数据。
//!
//! 闸门次序（`push`）：工具 → 表满 → 次序 → 路径 → 回环/占用 → 断供。
//! `execute`：空链 → 断供 → 链端。`seed`：零路径 → 重复 → 活集满。
//!
//! 账本契约：十四格，八成功六失败；成功格记录活集数、阶段数、链端与执行
//! 结果，失败格覆盖乱序/路径/占用/回环/断供/空链六类。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf010;

/// 未知阶段工具。
pub const PP_ETOOL: u32 = SEG_BASE + 0x01;
/// 阶段次序非法。
pub const PP_EORDER: u32 = SEG_BASE + 0x02;
/// 路径不在活集或不是链端。
pub const PP_EPATH: u32 = SEG_BASE + 0x03;
/// 移动目的已被占用。
pub const PP_EDEST: u32 = SEG_BASE + 0x04;
/// 移动目的落在历史里，形成回环。
pub const PP_ECYCLE: u32 = SEG_BASE + 0x05;
/// 上游无数据，后续阶段断供。
pub const PP_EFLOW: u32 = SEG_BASE + 0x06;
/// 阶段表或活集满。
pub const PP_EFULL: u32 = SEG_BASE + 0x07;
/// 空链没有可执行内容。
pub const PP_ENODATA: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const PP_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const PP_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const PP_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const PP_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const PP_ENOBS: u32 = SEG_BASE + 0x0d;

/// 阶段/活集容量。
pub const CAP: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 阶段：cat。
pub const T_CAT: u8 = 1;
/// 阶段：mv。
pub const T_MV: u8 = 2;
/// 阶段：chmod。
pub const T_CHMOD: u8 = 3;

/// 操作码：播种活文件。
pub const OP_SEED: u8 = 1;
/// 操作码：追加阶段。
pub const OP_PUSH: u8 = 2;
/// 操作码：执行链条。
pub const OP_RUN: u8 = 3;
/// 操作码：观察链端。
pub const OP_TIP: u8 = 4;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 一个管道阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stage {
    /// 工具。
    pub tool: u8,
    /// 路径（cat 的源、mv 的源、chmod 的目标）。
    pub path: u32,
    /// 参数（cat 字节数、mv 目的、chmod 模式）。
    pub arg: u32,
}

/// 一条管道 + 移动链。
#[derive(Clone, Copy, Debug)]
pub struct Chain {
    stages: [Stage; CAP],
    n: usize,
    live: [u32; CAP],
    live_n: usize,
    hist: [u32; CAP],
    hist_n: usize,
    tip: u32,
    flow: bool,
    bytes: u32,
    mode: u32,
}

impl Chain {
    /// 空链。
    pub const fn new() -> Chain {
        Chain {
            stages: [Stage { tool: 0, path: 0, arg: 0 }; CAP],
            n: 0,
            live: [0u32; CAP],
            live_n: 0,
            hist: [0u32; CAP],
            hist_n: 0,
            tip: 0,
            flow: false,
            bytes: 0,
            mode: 0,
        }
    }

    /// 阶段数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空链。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 第 i 段。
    pub fn stage_at(&self, i: usize) -> Option<Stage> {
        if i >= self.n {
            return None;
        }
        self.stages.get(i).copied()
    }

    /// 活文件数。
    pub fn live_count(&self) -> usize {
        self.live_n
    }

    /// 历史长度。
    pub fn history_count(&self) -> usize {
        self.hist_n
    }

    /// 活集里是否有该路径。
    pub fn live_has(&self, path: u32) -> bool {
        let mut i = 0usize;
        while i < self.live_n {
            if let Some(v) = self.live.get(i) {
                if *v == path {
                    return true;
                }
            }
            i = i.saturating_add(1);
        }
        false
    }

    /// 链端。
    pub fn tip(&self) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        Some(self.tip)
    }

    /// 链端，以结果形式给出。
    pub fn tip_word(&self) -> Result<u32, u32> {
        if self.n == 0 {
            Err(PP_ENODATA)
        } else {
            Ok(self.tip)
        }
    }

    /// cat 产出的字节数。
    pub fn bytes(&self) -> u32 {
        self.bytes
    }

    /// 最近一次 chmod 的模式。
    pub fn mode(&self) -> u32 {
        self.mode
    }

    /// 最后一阶段的工具。
    pub fn last_tool(&self) -> Option<u8> {
        if self.n == 0 {
            return None;
        }
        self.stages.get(self.n - 1).map(|x| x.tool)
    }

    fn live_remove(&mut self, path: u32) {
        let mut i = 0usize;
        while i < self.live_n {
            let v = match self.live.get(i) {
                Some(x) => *x,
                None => return,
            };
            if v == path {
                let mut k = i;
                while k + 1 < self.live_n {
                    if let Some(next) = self.live.get(k + 1).copied() {
                        if let Some(dst) = self.live.get_mut(k) {
                            *dst = next;
                        }
                    }
                    k = k.saturating_add(1);
                }
                self.live_n = self.live_n.saturating_sub(1);
                return;
            }
            i = i.saturating_add(1);
        }
    }

    fn live_add(&mut self, path: u32) {
        if self.live_n < CAP {
            if let Some(dst) = self.live.get_mut(self.live_n) {
                *dst = path;
            }
            self.live_n = self.live_n.saturating_add(1);
        }
    }

    fn hist_has(&self, path: u32) -> bool {
        let mut i = 0usize;
        while i < self.hist_n {
            if let Some(v) = self.hist.get(i) {
                if *v == path {
                    return true;
                }
            }
            i = i.saturating_add(1);
        }
        false
    }

    fn hist_push(&mut self, path: u32) {
        if self.hist_n < CAP {
            if let Some(dst) = self.hist.get_mut(self.hist_n) {
                *dst = path;
            }
            self.hist_n = self.hist_n.saturating_add(1);
        }
    }

    /// 播种一个已存在的文件。
    ///
    /// 闸门次序：零路径 → 重复 → 活集满。
    pub fn seed(&mut self, path: u32) -> Result<usize, u32> {
        if path == 0 {
            return Err(PP_EPATH);
        }
        if self.live_has(path) {
            return Err(PP_EDEST);
        }
        if self.live_n >= CAP {
            return Err(PP_EFULL);
        }
        self.live_add(path);
        Ok(self.live_n)
    }

    /// 追加一个阶段。
    ///
    /// 闸门次序：工具 → 表满 → 次序 → 路径 → 回环/占用 → 断供。
    pub fn push(&mut self, tool: u8, path: u32, arg: u32) -> Result<usize, u32> {
        if tool != T_CAT && tool != T_MV && tool != T_CHMOD {
            return Err(PP_ETOOL);
        }
        if self.n >= CAP {
            return Err(PP_EFULL);
        }
        match tool {
            T_CAT => {
                if self.n != 0 {
                    return Err(PP_EORDER);
                }
            }
            _ => {
                if self.n == 0 {
                    return Err(PP_EORDER);
                }
                if self.last_tool() == Some(T_CHMOD) {
                    return Err(PP_EORDER);
                }
            }
        }
        match tool {
            T_CAT => {
                if !self.live_has(path) {
                    return Err(PP_EPATH);
                }
            }
            T_MV => {
                if self.n == 0 || path != self.tip {
                    return Err(PP_EPATH);
                }
                if arg == 0 {
                    return Err(PP_EPATH);
                }
                if self.hist_has(arg) {
                    return Err(PP_ECYCLE);
                }
                if self.live_has(arg) {
                    return Err(PP_EDEST);
                }
            }
            _ => {
                if !self.live_has(path) {
                    return Err(PP_EPATH);
                }
                if self.n == 0 || path != self.tip {
                    return Err(PP_EPATH);
                }
            }
        }
        if tool != T_CAT && !self.flow {
            return Err(PP_EFLOW);
        }
        if let Some(dst) = self.stages.get_mut(self.n) {
            *dst = Stage { tool, path, arg };
        }
        self.n = self.n.saturating_add(1);
        match tool {
            T_CAT => {
                self.flow = arg > 0;
                self.bytes = arg;
                self.tip = path;
                self.hist_push(path);
            }
            T_MV => {
                self.live_remove(path);
                self.live_add(arg);
                self.hist_push(arg);
                self.tip = arg;
            }
            _ => {
                self.mode = arg & 0o7777;
            }
        }
        Ok(self.n)
    }

    /// 执行链条：空链 → 断供 → 链端。
    pub fn execute(&self) -> Result<u32, u32> {
        if self.n == 0 {
            return Err(PP_ENODATA);
        }
        if !self.flow {
            return Err(PP_EFLOW);
        }
        Ok(self.tip)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n > CAP || self.live_n > CAP || self.hist_n > CAP {
            return false;
        }
        let mut i = 0usize;
        let mut chmods = 0usize;
        while i < self.n {
            let st = match self.stages.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if st.tool == T_CAT && i != 0 {
                return false;
            }
            if st.tool == T_CHMOD {
                chmods = chmods.saturating_add(1);
                if i + 1 != self.n {
                    return false;
                }
            }
            if st.tool != T_CAT && st.tool != T_MV && st.tool != T_CHMOD {
                return false;
            }
            i = i.saturating_add(1);
        }
        if chmods > 1 {
            return false;
        }
        if self.n > 0 {
            if !self.live_has(self.tip) || !self.hist_has(self.tip) {
                return false;
            }
        }
        let mut a = 0usize;
        while a < self.live_n {
            let v = match self.live.get(a) {
                Some(x) => *x,
                None => return false,
            };
            let mut b = a.saturating_add(1);
            while b < self.live_n {
                if let Some(w) = self.live.get(b) {
                    if *w == v {
                        return false;
                    }
                }
                b = b.saturating_add(1);
            }
            a = a.saturating_add(1);
        }
        true
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<usize, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u8, u32>` → 样本。
fn byte_cell(slot: u8, op: u8, r: Result<u8, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PP_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len = self.len.saturating_add(1);
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.rows.get(i) {
                if v.ok {
                    n = n.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len.saturating_sub(self.ok_count())
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(PP_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 1, 2, 3, 4, 12, 12];
        const CODES: [u32; 6] =
            [PP_EORDER, PP_EPATH, PP_EDEST, PP_ECYCLE, PP_EFLOW, PP_ENODATA];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let v = match self.rows.get(i) {
                Some(x) => *x,
                None => return Err(PP_EBAL),
            };
            if v.slot as usize != i {
                return Err(PP_ESLOT);
            }
            if v.ok {
                if v.code != 0 {
                    return Err(PP_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if v.value != *w {
                            return Err(PP_EVALUE);
                        }
                    }
                    None => return Err(PP_EBAL),
                }
                vi = vi.saturating_add(1);
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if v.code != *w {
                            return Err(PP_EVALUE);
                        }
                    }
                    None => return Err(PP_EBAL),
                }
                ci = ci.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        if vi != 8 || ci != 6 {
            return Err(PP_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.rows.get(i) {
                h = fold(h, v.slot as u64);
                h = fold(h, v.op as u64);
                h = fold(h, v.ok as u64);
                h = fold(h, v.code as u64);
                h = fold(h, v.value as u64);
            }
            i = i.saturating_add(1);
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(PP_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(PP_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十四格账本：播种 → 正链 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = Chain::new();

    let _ = l.register(cell(0, OP_SEED, c.seed(10)));
    let _ = l.register(cell(1, OP_SEED, c.seed(20)));
    let _ = l.register(cell(2, OP_PUSH, c.push(T_CAT, 10, 64)));
    let _ = l.register(cell(3, OP_PUSH, c.push(T_MV, 10, 11)));
    let _ = l.register(cell(4, OP_PUSH, c.push(T_MV, 11, 12)));
    let _ = l.register(cell(5, OP_PUSH, c.push(T_CHMOD, 12, 0o755)));
    let _ = l.register(word_cell(6, OP_RUN, c.execute()));
    let _ = l.register(word_cell(7, OP_TIP, c.tip_word()));

    let _ = l.register(cell(8, OP_PUSH, c.push(T_CAT, 10, 8)));

    let mut p = Chain::new();
    let _ = p.seed(40);
    let _ = p.push(T_CAT, 40, 8);
    let _ = p.push(T_MV, 40, 41);
    let _ = l.register(cell(9, OP_PUSH, p.push(T_MV, 40, 42)));

    let mut d = Chain::new();
    let _ = d.seed(50);
    let _ = d.seed(51);
    let _ = d.push(T_CAT, 50, 8);
    let _ = l.register(cell(10, OP_PUSH, d.push(T_MV, 50, 51)));

    let mut h = Chain::new();
    let _ = h.seed(60);
    let _ = h.push(T_CAT, 60, 8);
    let _ = h.push(T_MV, 60, 61);
    let _ = l.register(cell(11, OP_PUSH, h.push(T_MV, 61, 60)));

    let mut f = Chain::new();
    let _ = f.seed(30);
    let _ = f.push(T_CAT, 30, 0);
    let _ = l.register(cell(12, OP_PUSH, f.push(T_MV, 30, 31)));

    let empty = Chain::new();
    let _ = l.register(word_cell(13, OP_RUN, empty.execute()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：正链推进、链端、执行与活集。
    let mut a = Chain::new();
    let s1 = a.seed(10);
    let s2 = a.seed(20);
    let c1 = a.push(T_CAT, 10, 64);
    let c2 = a.push(T_MV, 10, 11);
    let c3 = a.push(T_MV, 11, 12);
    let c4 = a.push(T_CHMOD, 12, 0o755);
    out[1] = s1 == Ok(1)
        && s2 == Ok(2)
        && c1 == Ok(1)
        && c2 == Ok(2)
        && c3 == Ok(3)
        && c4 == Ok(4)
        && a.tip() == Some(12)
        && a.execute() == Ok(12)
        && a.mode() == 0o755
        && a.bytes() == 64
        && a.live_has(20)
        && a.live_has(12)
        && !a.live_has(10)
        && !a.live_has(11)
        && a.live_count() == 2
        && a.history_count() == 3
        && a.consistent();

    // 二路：八类失败各自可辨。
    let mut b = Chain::new();
    let _ = b.seed(1);
    let tool = b.push(9, 1, 1) == Err(PP_ETOOL);
    let _ = b.push(T_CAT, 1, 4);
    let order_cat = b.push(T_CAT, 1, 4) == Err(PP_EORDER);
    let path_miss = Chain::new().push(T_CAT, 9, 4) == Err(PP_EPATH);
    let path_mv = b.push(T_MV, 2, 3) == Err(PP_EPATH);
    let _ = b.push(T_MV, 1, 2);
    let cycle = b.push(T_MV, 2, 2) == Err(PP_ECYCLE);
    let mut e = Chain::new();
    let _ = e.seed(5);
    let _ = e.seed(6);
    let _ = e.push(T_CAT, 5, 4);
    let dest = e.push(T_MV, 5, 6) == Err(PP_EDEST);
    let _ = b.push(T_CHMOD, 2, 0o644);
    let order_after = b.push(T_MV, 2, 3) == Err(PP_EORDER);
    let mut z = Chain::new();
    let _ = z.seed(7);
    let _ = z.push(T_CAT, 7, 0);
    let flow = z.push(T_MV, 7, 8) == Err(PP_EFLOW);
    let no_data = Chain::new().execute() == Err(PP_ENODATA);
    let mut full = Chain::new();
    let _ = full.seed(1);
    let _ = full.push(T_CAT, 1, 1);
    let mut k = 0u32;
    while k < 4 {
        let _ = full.push(T_MV, 1 + k, 2 + k);
        k = k.saturating_add(1);
    }
    let _ = full.push(T_CHMOD, 5, 0o600);
    let full_err = full.push(T_MV, 5, 99) == Err(PP_EFULL);
    out[2] = tool
        && order_cat
        && path_miss
        && path_mv
        && dest
        && cycle
        && order_after
        && flow
        && no_data
        && full_err;

    // 三路：回环、断供与仅 cat+chmod 的短链。
    let mut q = Chain::new();
    let _ = q.seed(50);
    let _ = q.push(T_CAT, 50, 8);
    let _ = q.push(T_MV, 50, 51);
    let cycle = q.push(T_MV, 51, 50) == Err(PP_ECYCLE);
    let stay = q.tip() == Some(51) && q.live_has(51) && !q.live_has(50);

    let mut r = Chain::new();
    let _ = r.seed(60);
    let _ = r.push(T_CAT, 60, 4);
    let chmod_only = r.push(T_CHMOD, 60, 0o640) == Ok(2);
    let twice = r.push(T_CHMOD, 60, 0o600) == Err(PP_EORDER);

    let mut t = Chain::new();
    let _ = t.seed(70);
    let _ = t.push(T_CAT, 70, 0);
    let exec_flow = t.execute() == Err(PP_EFLOW);
    let chmod_flow = t.push(T_CHMOD, 70, 0o600) == Err(PP_EFLOW);
    out[3] = cycle
        && stay
        && chmod_only
        && twice
        && r.execute() == Ok(60)
        && r.mode() == 0o640
        && exec_flow
        && chmod_flow
        && q.consistent()
        && r.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11417_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11417.cat_mv_chmod_pipeline");
    let l = build_ledger();

    s.add_dyn(
        "PP1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "PP2-正链逐段推进",
        {
            let mut c = Chain::new();
            let _ = c.seed(10);
            let _ = c.seed(20);
            let _ = c.push(T_CAT, 10, 64);
            let a = c.push(T_MV, 10, 11);
            let b = c.push(T_MV, 11, 12);
            let d = c.push(T_CHMOD, 12, 0o755);
            a == Ok(2)
                && b == Ok(3)
                && d == Ok(4)
                && c.len() == 4
                && c.execute() == Ok(12)
                && c.tip() == Some(12)
                && l.sample(6).map(|x| x.value) == Some(12)
                && l.sample(7).map(|x| x.value) == Some(12)
        },
        "static",
    );

    s.add(
        "PP3-移动链末端推进",
        {
            let mut c = Chain::new();
            let _ = c.seed(1);
            let _ = c.push(T_CAT, 1, 8);
            let _ = c.push(T_MV, 1, 2);
            let _ = c.push(T_MV, 2, 3);
            c.tip() == Some(3)
                && c.stage_at(1) == Some(Stage { tool: T_MV, path: 1, arg: 2 })
                && c.stage_at(2) == Some(Stage { tool: T_MV, path: 2, arg: 3 })
                && c.live_has(3)
                && !c.live_has(1)
                && !c.live_has(2)
                && c.history_count() == 3
        },
        "static",
    );

    s.add(
        "PP4-源路径真值",
        {
            let mut c = Chain::new();
            let _ = c.seed(4);
            let _ = c.push(T_CAT, 4, 8);
            let _ = c.push(T_MV, 4, 5);
            c.push(T_MV, 4, 6) == Err(PP_EPATH)
                && c.push(T_CHMOD, 4, 0o600) == Err(PP_EPATH)
                && c.push(T_CHMOD, 5, 0o600) == Ok(3)
                && Chain::new().push(T_CAT, 9, 1) == Err(PP_EPATH)
                && l.sample(9).map(|x| x.code) == Some(PP_EPATH)
        },
        "static",
    );

    s.add(
        "PP5-目标占用",
        {
            let mut c = Chain::new();
            let _ = c.seed(5);
            let _ = c.seed(6);
            let _ = c.push(T_CAT, 5, 8);
            let r = c.push(T_MV, 5, 6);
            r == Err(PP_EDEST)
                && c.tip() == Some(5)
                && c.live_has(5)
                && c.live_has(6)
                && l.sample(10).map(|x| x.code) == Some(PP_EDEST)
        },
        "static",
    );

    s.add(
        "PP6-移动回环被拒",
        {
            let mut c = Chain::new();
            let _ = c.seed(7);
            let _ = c.push(T_CAT, 7, 8);
            let _ = c.push(T_MV, 7, 8);
            let r = c.push(T_MV, 8, 7);
            r == Err(PP_ECYCLE)
                && c.tip() == Some(8)
                && c.live_has(8)
                && !c.live_has(7)
                && l.sample(11).map(|x| x.code) == Some(PP_ECYCLE)
        },
        "static",
    );

    s.add(
        "PP7-上游断供闸门",
        {
            let mut c = Chain::new();
            let _ = c.seed(9);
            let _ = c.push(T_CAT, 9, 0);
            let mv = c.push(T_MV, 9, 10);
            let ch = c.push(T_CHMOD, 9, 0o600);
            let run = c.execute();
            mv == Err(PP_EFLOW)
                && ch == Err(PP_EFLOW)
                && run == Err(PP_EFLOW)
                && c.len() == 1
                && l.sample(12).map(|x| x.code) == Some(PP_EFLOW)
        },
        "static",
    );

    s.add(
        "PP8-空链无可执行",
        {
            let c = Chain::new();
            c.execute() == Err(PP_ENODATA)
                && c.tip_word() == Err(PP_ENODATA)
                && c.tip().is_none()
                && c.consistent()
                && l.sample(13).map(|x| x.code) == Some(PP_ENODATA)
        },
        "static",
    );

    s.add(
        "PP9-阶段次序闸门",
        {
            let mut c = Chain::new();
            let _ = c.seed(11);
            c.push(T_MV, 11, 12) == Err(PP_EORDER)
                && c.push(T_CHMOD, 11, 0o600) == Err(PP_EORDER)
                && c.push(T_CAT, 11, 4) == Ok(1)
                && c.push(T_CAT, 11, 4) == Err(PP_EORDER)
                && c.push(T_CHMOD, 11, 0o600) == Ok(2)
                && c.push(T_CHMOD, 11, 0o644) == Err(PP_EORDER)
                && c.push(T_MV, 11, 12) == Err(PP_EORDER)
                && l.sample(8).map(|x| x.code) == Some(PP_EORDER)
        },
        "static",
    );

    s.add(
        "PP10-工具与表满",
        {
            let mut c = Chain::new();
            let _ = c.seed(1);
            let tool = c.push(9, 1, 1) == Err(PP_ETOOL);
            let _ = c.push(T_CAT, 1, 1);
            let mut k = 0u32;
            while k < 4 {
                let _ = c.push(T_MV, 1 + k, 2 + k);
                k = k.saturating_add(1);
            }
            let _ = c.push(T_CHMOD, 5, 0o600);
            let full = c.push(T_MV, 5, 99) == Err(PP_EFULL);
            let mut d = Chain::new();
            let mut j = 0u32;
            while j < CAP as u32 {
                let _ = d.seed(j + 1);
                j = j.saturating_add(1);
            }
            tool && full && d.seed(CAP as u32 + 1) == Err(PP_EFULL) && d.live_count() == CAP
        },
        "static",
    );

    s.add(
        "PP11-活集与历史账",
        {
            let mut c = Chain::new();
            let _ = c.seed(1);
            let _ = c.seed(2);
            let _ = c.push(T_CAT, 1, 16);
            let _ = c.push(T_MV, 1, 3);
            c.live_count() == 2
                && c.history_count() == 2
                && c.live_has(2)
                && c.live_has(3)
                && !c.live_has(1)
                && c.tip() == Some(3)
                && c.bytes() == 16
        },
        "static",
    );

    s.add(
        "PP12-不变式自检",
        {
            let mut c = Chain::new();
            let _ = c.seed(1);
            let _ = c.push(T_CAT, 1, 8);
            let _ = c.push(T_MV, 1, 2);
            let good = c.consistent();
            let mut d = Chain::new();
            let _ = d.seed(5);
            let _ = d.push(T_CAT, 5, 0);
            good && d.consistent() && !d.is_empty() && d.len() == 1
        },
        "static",
    );

    s.add(
        "PP13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.rejects(), 0);
        assert!(l.audit().is_ok());
        assert_ne!(l.seal(), 0);
        assert_eq!(l.replay(), Ok(l.seal()));
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11417_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn pipeline_happy_path() {
        let mut c = Chain::new();
        assert_eq!(c.seed(10), Ok(1));
        assert_eq!(c.seed(20), Ok(2));
        assert_eq!(c.push(T_CAT, 10, 64), Ok(1));
        assert_eq!(c.push(T_MV, 10, 11), Ok(2));
        assert_eq!(c.push(T_MV, 11, 12), Ok(3));
        assert_eq!(c.push(T_CHMOD, 12, 0o755), Ok(4));
        assert_eq!(c.execute(), Ok(12));
        assert_eq!(c.tip(), Some(12));
        assert_eq!(c.mode(), 0o755);
        assert!(c.consistent());
    }

    #[test]
    fn order_gates() {
        let mut c = Chain::new();
        let _ = c.seed(1);
        assert_eq!(c.push(T_MV, 1, 2), Err(PP_EORDER));
        assert_eq!(c.push(T_CAT, 1, 4), Ok(1));
        assert_eq!(c.push(T_CAT, 1, 4), Err(PP_EORDER));
        assert_eq!(c.push(T_CHMOD, 1, 0o600), Ok(2));
        assert_eq!(c.push(T_CHMOD, 1, 0o644), Err(PP_EORDER));
        assert_eq!(c.push(9, 1, 0), Err(PP_ETOOL));
    }

    #[test]
    fn move_chain_cycle_and_dest() {
        let mut c = Chain::new();
        let _ = c.seed(1);
        let _ = c.seed(2);
        let _ = c.push(T_CAT, 1, 8);
        assert_eq!(c.push(T_MV, 1, 2), Err(PP_EDEST));
        assert_eq!(c.push(T_MV, 1, 3), Ok(2));
        assert_eq!(c.push(T_MV, 3, 1), Err(PP_ECYCLE));
        assert_eq!(c.push(T_MV, 3, 2), Err(PP_EDEST));
        assert_eq!(c.tip(), Some(3));
        assert!(!c.live_has(1));
    }

    #[test]
    fn flow_and_empty() {
        let mut c = Chain::new();
        assert_eq!(c.execute(), Err(PP_ENODATA));
        assert_eq!(c.tip_word(), Err(PP_ENODATA));
        let _ = c.seed(1);
        let _ = c.push(T_CAT, 1, 0);
        assert_eq!(c.push(T_MV, 1, 2), Err(PP_EFLOW));
        assert_eq!(c.execute(), Err(PP_EFLOW));
        let mut d = Chain::new();
        let _ = d.seed(2);
        let _ = d.push(T_CAT, 2, 1);
        let _ = d.push(T_MV, 2, 3);
        let _ = d.push(T_CHMOD, 3, 0o644);
        assert_eq!(d.execute(), Ok(3));
    }

    #[test]
    fn path_gates() {
        let mut c = Chain::new();
        assert_eq!(c.push(T_CAT, 9, 1), Err(PP_EPATH));
        let _ = c.seed(1);
        let _ = c.push(T_CAT, 1, 1);
        let _ = c.push(T_MV, 1, 2);
        assert_eq!(c.push(T_MV, 1, 3), Err(PP_EPATH));
        assert_eq!(c.push(T_CHMOD, 1, 0o600), Err(PP_EPATH));
        assert_eq!(c.push(T_CHMOD, 2, 0o600), Ok(3));
    }

    #[test]
    fn capacity_limits() {
        let mut c = Chain::new();
        let _ = c.seed(1);
        let _ = c.push(T_CAT, 1, 1);
        let mut k = 0u32;
        while k < 4 {
            assert_eq!(c.push(T_MV, 1 + k, 2 + k), Ok((k + 2) as usize));
            k += 1;
        }
        assert_eq!(c.push(T_CHMOD, 5, 0o600), Ok(6));
        assert_eq!(c.push(T_MV, 5, 9), Err(PP_EFULL));
        assert_eq!(c.len(), CAP);
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
