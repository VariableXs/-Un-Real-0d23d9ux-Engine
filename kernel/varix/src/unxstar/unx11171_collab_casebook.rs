//! UNX-F11171 域经快照三（并发协作判例汇编）（0x1f470 段）
//!
//! 模型：把一次并发协作写成一条判例。每条判例登记并行会话数、参与者编号、
//! 每位参与者的写入量与本人同意额度、裹挟见证数、合流结论码、事实链哈希
//! 与前序哈希。规则四则：会话数须在二至三之间；参与者至少两人且编号严格
//! 递增；写入量不得越过本人同意额度且额度非零；裹挟见证数必须为零。
//! 账本按判例号升序收案，前序哈希须接续上一条判例的链尾；收案记一次受理，
//! 越界、重复、倒序、链断、形状不符、越额与裹挟一律拒收并记一次拒止。
//! 终核逐条复算事实链并折叠为单一指纹；冻结后二次冻结返回冻结码。
//! 拒收只针对结构问题，探针故意给出的越额与裹挟值由规则函数判定，
//! 使检查项能够真实触达两条拒收路径。

use crate::checks::CheckSet;

/// 段基址（B39 段 · 并发协作判例）。
const SEG_BASE: u32 = 0x1f470;

/// 判例号越界。
pub const CC_ENO: u32 = SEG_BASE + 0x01;
/// 判例号重复。
pub const CC_EDUP: u32 = SEG_BASE + 0x02;
/// 判例号倒序。
pub const CC_EORDER: u32 = SEG_BASE + 0x03;
/// 参与者形状不符。
pub const CC_EPART: u32 = SEG_BASE + 0x04;
/// 写入越过同意额度。
pub const CC_EQUOTA: u32 = SEG_BASE + 0x05;
/// 存在裹挟见证。
pub const CC_ECOERCE: u32 = SEG_BASE + 0x06;
/// 事实链断裂。
pub const CC_ECHAIN: u32 = SEG_BASE + 0x07;
/// 判例条数不齐。
pub const CC_ECOUNT: u32 = SEG_BASE + 0x08;
/// 终值已冻结。
pub const CC_EFROZEN: u32 = SEG_BASE + 0x09;

/// 判例条数。
pub const CASES: usize = 7;
/// 单判例参与者容量。
pub const PARTS: usize = 4;
/// 并行会话数下界。
pub const MIN_SESSIONS: u8 = 2;
/// 并行会话数上界（含）。
pub const MAX_SESSIONS: u8 = 3;
/// 单参与者同意额度上界。
pub const CONSENT_MAX: u32 = 96;
/// 合流结论码种类。
pub const MERGES: u8 = 3;
/// 参与者人数下界。
pub const MIN_PARTS: u8 = 2;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 事实链盖章：判例号、合流码、前序与写量合计卷成 32 位指纹。
pub fn stamp(case_no: u8, merge: u8, prev: u32, writes: u32) -> u32 {
    let mut x = prev ^ (case_no as u32).wrapping_mul(0x9e37_79b1);
    x = x.rotate_left(5) ^ (merge as u32);
    x = x.wrapping_mul(0x85eb_ca6b) ^ writes;
    if x == 0 {
        1
    } else {
        x
    }
}

/// 一例并发协作判例。
#[derive(Clone, Copy, Debug)]
pub struct CollabCase {
    pub case_no: u8,
    pub sessions: u8,
    pub nparts: u8,
    pub merge: u8,
    pub coercion: u8,
    pub prev: u32,
    pub trace: u32,
    pub participants: [u8; PARTS],
    pub writes: [u32; PARTS],
    pub consent: [u32; PARTS],
}

impl CollabCase {
    /// 全零空例，供账本数组初始化。
    pub const fn blank() -> CollabCase {
        CollabCase {
            case_no: 0,
            sessions: 0,
            nparts: 0,
            merge: 0,
            coercion: 0,
            prev: 0,
            trace: 0,
            participants: [0u8; PARTS],
            writes: [0u32; PARTS],
            consent: [0u32; PARTS],
        }
    }

    /// 第 i 位参与者编号。
    pub fn participant(&self, i: usize) -> Option<u8> {
        self.participants.get(i).copied()
    }

    /// 第 i 位参与者的写入量。
    pub fn write_of(&self, i: usize) -> Option<u32> {
        self.writes.get(i).copied()
    }

    /// 第 i 位参与者的同意额度。
    pub fn consent_of(&self, i: usize) -> Option<u32> {
        self.consent.get(i).copied()
    }

    /// 全例写入合计。
    pub fn writes_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < PARTS {
            if let Some(v) = self.writes.get(i) {
                sum = sum.wrapping_add(*v);
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 全例同意额度合计。
    pub fn consent_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < PARTS {
            if let Some(v) = self.consent.get(i) {
                sum = sum.wrapping_add(*v);
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 会话数、合流码、人数与链尾非零。
    pub fn shape_ok(&self) -> bool {
        self.sessions >= MIN_SESSIONS
            && self.sessions <= MAX_SESSIONS
            && self.merge < MERGES
            && self.nparts >= MIN_PARTS
            && (self.nparts as usize) <= PARTS
            && self.trace != 0
    }

    /// 参与者编号在人数内严格递增且非零。
    pub fn parts_ok(&self) -> bool {
        let n = self.nparts as usize;
        if n < MIN_PARTS as usize || n > PARTS {
            return false;
        }
        let mut i = 0usize;
        while i < n {
            match self.participants.get(i) {
                Some(p) => {
                    if *p == 0 {
                        return false;
                    }
                    if i > 0 {
                        match self.participants.get(i - 1) {
                            Some(prev) => {
                                if *p <= *prev {
                                    return false;
                                }
                            }
                            None => return false,
                        }
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 本人写入不越本人同意额度，且额度非零不超上界。
    pub fn quota_ok(&self) -> bool {
        let n = self.nparts as usize;
        if n == 0 || n > PARTS {
            return false;
        }
        let mut i = 0usize;
        while i < n {
            let w = match self.write_of(i) {
                Some(v) => v,
                None => return false,
            };
            let c = match self.consent_of(i) {
                Some(v) => v,
                None => return false,
            };
            if c == 0 || c > CONSENT_MAX || w > c {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 零裹挟：无任何裹挟见证。
    pub fn zero_coercion(&self) -> bool {
        self.coercion == 0
    }

    /// 链尾可由判例字段复算。
    pub fn trace_ok(&self) -> bool {
        self.trace == stamp(self.case_no, self.merge, self.prev, self.writes_total())
    }
}

/// 协作判例账本。
#[derive(Clone, Copy, Debug)]
pub struct CollabLedger {
    cases: [CollabCase; CASES],
    len: usize,
    accepts: u32,
    rejects: u32,
    frozen: bool,
    settled: Option<u64>,
}

impl CollabLedger {
    /// 空账本。
    pub const fn new() -> CollabLedger {
        CollabLedger {
            cases: [CollabCase::blank(); CASES],
            len: 0,
            accepts: 0,
            rejects: 0,
            frozen: false,
            settled: None,
        }
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已收判例数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 受理计数。
    pub fn accepted(&self) -> u32 {
        self.accepts
    }

    /// 拒止计数。
    pub fn rejected(&self) -> u32 {
        self.rejects
    }

    /// 末条判例（空账为 None）。
    pub fn last(&self) -> Option<CollabCase> {
        if self.len == 0 {
            return None;
        }
        self.cases.get(self.len - 1).copied()
    }

    /// 第 i 条判例。
    pub fn case_at(&self, i: usize) -> Option<CollabCase> {
        if i >= self.len {
            return None;
        }
        self.cases.get(i).copied()
    }

    /// 按判例号查找。
    pub fn find(&self, case_no: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.cases.get(i) {
                if c.case_no == case_no {
                    return Some(i);
                }
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 收案：冻结 → 越界 → 重复 → 倒序 → 链断 → 形状 → 额度 → 裹挟 → 复算。
    pub fn register(&mut self, c: CollabCase) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(CC_EFROZEN));
        }
        if c.case_no == 0 || c.case_no as usize > CASES {
            return Err(self.fail(CC_ENO));
        }
        if self.find(c.case_no).is_some() {
            return Err(self.fail(CC_EDUP));
        }
        match self.last() {
            Some(last) => {
                if c.case_no <= last.case_no {
                    return Err(self.fail(CC_EORDER));
                }
                if c.prev != last.trace {
                    return Err(self.fail(CC_ECHAIN));
                }
            }
            None => {
                if c.prev != 0 {
                    return Err(self.fail(CC_ECHAIN));
                }
            }
        }
        if !c.shape_ok() || !c.parts_ok() {
            return Err(self.fail(CC_EPART));
        }
        if !c.quota_ok() {
            return Err(self.fail(CC_EQUOTA));
        }
        if !c.zero_coercion() {
            return Err(self.fail(CC_ECOERCE));
        }
        if !c.trace_ok() {
            return Err(self.fail(CC_ECHAIN));
        }
        match self.cases.get_mut(self.len) {
            Some(slot) => *slot = c,
            None => return Err(self.fail(CC_ECOUNT)),
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 全账写入合计。
    pub fn total_writes(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.cases.get(i) {
                sum = sum.wrapping_add(c.writes_total());
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 全账同意额度合计。
    pub fn total_consent(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.cases.get(i) {
                sum = sum.wrapping_add(c.consent_total());
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 全账裹挟见证合计。
    pub fn coercion_witnesses(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.cases.get(i) {
                sum = sum.wrapping_add(c.coercion as u32);
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 全账形状齐备。
    pub fn all_shape_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            match self.cases.get(i) {
                Some(c) => {
                    if !c.shape_ok() || !c.parts_ok() {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 全账额度齐备。
    pub fn all_quota_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            match self.cases.get(i) {
                Some(c) => {
                    if !c.quota_ok() {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 零裹挟断言：全账无裹挟见证。
    pub fn coercion_free(&self) -> bool {
        self.coercion_witnesses() == 0
    }

    /// 事实链逐条接续且链尾可复算。
    pub fn chain_ok(&self) -> bool {
        let mut prev = 0u32;
        let mut i = 0usize;
        while i < self.len {
            match self.cases.get(i) {
                Some(c) => {
                    if c.prev != prev || !c.trace_ok() {
                        return false;
                    }
                    prev = c.trace;
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 账本指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.cases.get(i) {
                h = (h ^ c.case_no as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.sessions as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.nparts as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.merge as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.coercion as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.prev as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.trace as u64).wrapping_mul(FNV_PRIME);
                let mut j = 0usize;
                while j < PARTS {
                    if let Some(v) = c.writes.get(j) {
                        h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
                    }
                    if let Some(v) = c.consent.get(j) {
                        h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
                    }
                    j = j.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        h = (h ^ self.len as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 重放：条数齐、号连续、规则全过、链接续，返回重算指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len != CASES {
            return Err(CC_ECOUNT);
        }
        let mut prev = 0u32;
        let mut i = 0usize;
        while i < self.len {
            let c = match self.cases.get(i) {
                Some(v) => *v,
                None => return Err(CC_ENO),
            };
            if c.case_no as usize != i.wrapping_add(1) {
                return Err(CC_EORDER);
            }
            if !c.shape_ok() || !c.parts_ok() {
                return Err(CC_EPART);
            }
            if !c.quota_ok() {
                return Err(CC_EQUOTA);
            }
            if !c.zero_coercion() {
                return Err(CC_ECOERCE);
            }
            if c.prev != prev || !c.trace_ok() {
                return Err(CC_ECHAIN);
            }
            prev = c.trace;
            i = i.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 冻结：条数齐且重放通过才锁值，二次冻结返回冻结码。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(CC_EFROZEN));
        }
        if self.len != CASES {
            return Err(self.fail(CC_ECOUNT));
        }
        let v = match self.replay() {
            Ok(v) => v,
            Err(e) => return Err(self.fail(e)),
        };
        self.settled = Some(v);
        self.frozen = true;
        Ok(v)
    }

    /// 终值。
    pub fn settled_value(&self) -> Option<u64> {
        self.settled
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 按号观察收案：由规格表生成判例并接在链尾。
    pub fn observe(&mut self, id: usize) -> Result<u64, u32> {
        if id == 0 || id > CASES {
            return Err(self.fail(CC_ENO));
        }
        let prev = match self.last() {
            Some(c) => c.trace,
            None => 0,
        };
        let c = case_spec(id, prev);
        self.register(c)
    }
}

/// 规格写量与额度合计（第 n 例）。
fn spec_sums(n: u8) -> (u32, u32) {
    let nparts = MIN_PARTS + (n % 3);
    let mut i = 0usize;
    let mut w = 0u32;
    let mut c = 0u32;
    while i < nparts as usize {
        w = w.wrapping_add(8 + (n as u32) * 2 + (i as u32) * 3);
        c = c.wrapping_add(CONSENT_MAX - (i as u32) * 4);
        i = i.wrapping_add(1);
    }
    (w, c)
}

/// 判例规格：会话数、参与者人数、写量合计、额度合计。
pub fn spec_of(id: usize) -> (u8, u8, u32, u32) {
    if id == 0 || id > CASES {
        return (0, 0, 0, 0);
    }
    let n = id as u8;
    let sessions = MIN_SESSIONS + (n & 1);
    let nparts = MIN_PARTS + (n % 3);
    let (w, c) = spec_sums(n);
    (sessions, nparts, w, c)
}

/// 由规格生成一条判例，接在前序链尾之后。
fn case_spec(id: usize, prev: u32) -> CollabCase {
    let mut c = CollabCase::blank();
    if id == 0 || id > CASES {
        return c;
    }
    let n = id as u8;
    let spec = spec_of(id);
    c.case_no = n;
    c.sessions = spec.0;
    c.nparts = spec.1;
    c.merge = n % MERGES;
    c.prev = prev;
    let mut i = 0usize;
    while i < c.nparts as usize {
        if let Some(p) = c.participants.get_mut(i) {
            *p = (i as u8).wrapping_add(1);
        }
        if let Some(w) = c.writes.get_mut(i) {
            *w = 8 + (n as u32) * 2 + (i as u32) * 3;
        }
        if let Some(q) = c.consent.get_mut(i) {
            *q = CONSENT_MAX - (i as u32) * 4;
        }
        i = i.wrapping_add(1);
    }
    c.trace = stamp(c.case_no, c.merge, c.prev, c.writes_total());
    c
}

/// 构建标准判例账：七条顺序收案，链尾接续。
pub fn build_ledger() -> CollabLedger {
    let mut l = CollabLedger::new();
    let mut id = 1usize;
    while id <= CASES {
        let _ = l.observe(id);
        id = id.wrapping_add(1);
    }
    l
}

/// 探针判例：人数二人，写量可按 extra 抬高以触达越额路径。
fn probe_case(case_no: u8, merge: u8, sessions: u8, coercion: u8, extra: u32, prev: u32) -> CollabCase {
    let mut c = CollabCase::blank();
    c.case_no = case_no;
    c.sessions = sessions;
    c.merge = merge;
    c.coercion = coercion;
    c.nparts = MIN_PARTS;
    c.prev = prev;
    if let Some(p) = c.participants.get_mut(0) {
        *p = 1;
    }
    if let Some(p) = c.participants.get_mut(1) {
        *p = 2;
    }
    if let Some(w) = c.writes.get_mut(0) {
        *w = 12u32.wrapping_add(extra);
    }
    if let Some(w) = c.writes.get_mut(1) {
        *w = 10;
    }
    if let Some(q) = c.consent.get_mut(0) {
        *q = CONSENT_MAX;
    }
    if let Some(q) = c.consent.get_mut(1) {
        *q = CONSENT_MAX;
    }
    c.trace = stamp(c.case_no, c.merge, prev, c.writes_total());
    c
}

/// 四探针：越界、重复/倒序、越额/裹挟、冻结后写，各以本模块错误码拒绝。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut a = CollabLedger::new();
    let a1 = probe_case(1, 0, MIN_SESSIONS, 0, 0, 0);
    let _ = a.register(a1);
    if a.register(probe_case(0, 0, MIN_SESSIONS, 0, 0, a1.trace)) == Err(CC_ENO)
        && a.register(probe_case((CASES + 1) as u8, 0, MIN_SESSIONS, 0, 0, a1.trace)) == Err(CC_ENO)
    {
        r[0] = true;
    }

    let mut b = CollabLedger::new();
    let b2 = probe_case(2, 0, MIN_SESSIONS, 0, 0, 0);
    let _ = b.register(b2);
    if b.register(b2) == Err(CC_EDUP)
        && b.register(probe_case(1, 0, MIN_SESSIONS, 0, 0, b2.trace)) == Err(CC_EORDER)
    {
        r[1] = true;
    }

    let mut c = CollabLedger::new();
    let c1 = probe_case(1, 0, MIN_SESSIONS, 0, 0, 0);
    let _ = c.register(c1);
    if c.register(probe_case(2, 0, MIN_SESSIONS, 0, 200, c1.trace)) == Err(CC_EQUOTA)
        && c.register(probe_case(2, 0, MIN_SESSIONS, 1, 0, c1.trace)) == Err(CC_ECOERCE)
    {
        r[2] = true;
    }

    let mut d = build_ledger();
    if d.freeze().is_ok()
        && d.freeze() == Err(CC_EFROZEN)
        && d.register(probe_case(1, 0, MIN_SESSIONS, 0, 0, 0)) == Err(CC_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11171_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11171");
    let base = build_ledger();

    s.add(
        "S1-判例留痕",
        base.len() == CASES && base.accepted() == CASES as u32 && base.rejected() == 0,
        "七条判例全收、零拒止",
    );

    let head_ok = match base.case_at(0) {
        Some(c) => c.prev == 0,
        None => false,
    };
    s.add(
        "S2-事实链连续",
        head_ok && base.chain_ok(),
        "首条前序为零且逐条链尾接续可复算",
    );

    let mut sess_ok = true;
    let mut i = 0usize;
    while i < base.len() {
        match base.case_at(i) {
            Some(c) => {
                if c.sessions < MIN_SESSIONS || c.sessions > MAX_SESSIONS {
                    sess_ok = false;
                }
            }
            None => sess_ok = false,
        }
        i = i.wrapping_add(1);
    }
    s.add("S3-会话数合法", sess_ok, "并行会话数均在二至三之间");

    s.add(
        "S4-参与者递增",
        base.all_shape_ok(),
        "人数不少于二且参与者编号严格递增",
    );

    s.add(
        "S5-写量不越额",
        base.all_quota_ok(),
        "每位参与者写入不越本人同意额度",
    );

    s.add(
        "S6-零裹挟",
        base.coercion_free() && base.coercion_witnesses() == 0,
        "全账裹挟见证为零",
    );

    let mut spec_w = 0u32;
    let mut id = 1usize;
    while id <= CASES {
        spec_w = spec_w.wrapping_add(spec_of(id).2);
        id = id.wrapping_add(1);
    }
    s.add(
        "S7-合计对平",
        base.total_writes() == spec_w && base.total_writes() <= base.total_consent(),
        "全账写量与规格合计相等且不越额度合计",
    );

    s.add(
        "S8-折叠稳定",
        base.fold() == base.fold() && base.fold() != 0,
        "两次折叠同值且非零",
    );

    let mut fz = base.clone();
    let v1 = fz.freeze();
    let v2 = fz.freeze();
    let single = match v1 {
        Ok(v) => v2 == Err(CC_EFROZEN) && fz.settled_value() == Some(v),
        Err(_) => false,
    };
    s.add("S9-冻结单值", single && fz.is_frozen(), "首冻锁值、二次冻结返回冻结码");

    s.add(
        "S10-重放一致",
        base.replay() == Ok(base.fold()),
        "重放逐号对平且指纹一致",
    );

    let mut la = CollabLedger::new();
    let a1 = probe_case(1, 0, MIN_SESSIONS, 0, 0, 0);
    let _ = la.register(a1);
    let r0 = la.register(probe_case(0, 0, MIN_SESSIONS, 0, 0, a1.trace));
    let r1 = la.register(probe_case((CASES + 1) as u8, 0, MIN_SESSIONS, 0, 0, a1.trace));
    s.add(
        "X11-越界拒收",
        r0 == Err(CC_ENO) && r1 == Err(CC_ENO) && la.accepted() == 1 && la.rejected() == 2,
        "判例号 0 与 8 均判越界",
    );

    let mut lb = CollabLedger::new();
    let b2 = probe_case(2, 0, MIN_SESSIONS, 0, 0, 0);
    let _ = lb.register(b2);
    let dup = lb.register(b2);
    let ord = lb.register(probe_case(1, 0, MIN_SESSIONS, 0, 0, b2.trace));
    s.add(
        "X12-重复倒序",
        dup == Err(CC_EDUP) && ord == Err(CC_EORDER) && lb.accepted() == 1 && lb.rejected() == 2,
        "重收二号判重复、二号后写一号判倒序",
    );

    let mut lc = CollabLedger::new();
    let c1 = probe_case(1, 0, MIN_SESSIONS, 0, 0, 0);
    let _ = lc.register(c1);
    let q = lc.register(probe_case(2, 0, MIN_SESSIONS, 0, 200, c1.trace));
    let co = lc.register(probe_case(2, 0, MIN_SESSIONS, 1, 0, c1.trace));
    let mut ld = build_ledger();
    let locked = ld.freeze().is_ok()
        && ld.freeze() == Err(CC_EFROZEN)
        && ld.register(probe_case(1, 0, MIN_SESSIONS, 0, 0, 0)) == Err(CC_EFROZEN);
    s.add(
        "X13-越额裹挟与冻结",
        q == Err(CC_EQUOTA)
            && co == Err(CC_ECOERCE)
            && lc.accepted() == 1
            && lc.rejected() == 2
            && locked,
        "越额与裹挟分码拒收，冻结后收案拒收",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11171_flow() {
        let l = build_ledger();
        assert_eq!(l.len(), CASES);
        assert_eq!(l.accepted(), CASES as u32);
        assert_eq!(l.rejected(), 0);
        assert!(l.chain_ok());
        assert!(l.all_shape_ok());
        assert!(l.all_quota_ok());
        assert!(l.coercion_free());
        assert!(l.total_writes() > 0);
        assert!(l.total_writes() <= l.total_consent());
        assert_eq!(l.case_at(0).map(|c| c.prev), Some(0));
        assert!(l.case_at(CASES).is_none());
        assert!(l.find(3).is_some());
        assert!(l.find(0).is_none());
        assert!(l.fold() != 0);
    }

    #[test]
    fn f11171_spec() {
        assert_eq!(spec_of(0), (0, 0, 0, 0));
        assert_eq!(spec_of(CASES + 1), (0, 0, 0, 0));
        let mut total = 0u32;
        let mut id = 1usize;
        while id <= CASES {
            let (s, p, w, c) = spec_of(id);
            assert!(s >= MIN_SESSIONS && s <= MAX_SESSIONS);
            assert!(p >= MIN_PARTS && (p as usize) <= PARTS);
            assert!(w <= c);
            total = total.wrapping_add(w);
            id = id.wrapping_add(1);
        }
        assert_eq!(build_ledger().total_writes(), total);
    }

    #[test]
    fn f11171_gates() {
        let mut a = CollabLedger::new();
        let c1 = probe_case(1, 0, MIN_SESSIONS, 0, 0, 0);
        assert!(a.register(c1).is_ok());
        assert_eq!(a.register(probe_case(1, 0, MIN_SESSIONS, 0, 0, c1.trace)), Err(CC_EDUP));
        assert_eq!(a.register(probe_case(2, 0, MAX_SESSIONS + 1, 0, 0, c1.trace)), Err(CC_EPART));
        assert_eq!(a.register(probe_case(2, 0, MIN_SESSIONS, 0, 200, c1.trace)), Err(CC_EQUOTA));
        assert_eq!(a.register(probe_case(2, 0, MIN_SESSIONS, 1, 0, c1.trace)), Err(CC_ECOERCE));
        assert_eq!(a.register(probe_case(2, 0, MIN_SESSIONS, 0, 0, c1.trace ^ 1)), Err(CC_ECHAIN));
        assert_eq!(a.accepted(), 1);
        assert_eq!(a.rejected(), 5);
    }

    #[test]
    fn f11171_freeze() {
        let mut l = build_ledger();
        assert_eq!(l.replay(), Ok(l.fold()));
        assert!(!l.is_frozen());
        let v = l.freeze();
        assert!(v.is_ok());
        assert!(l.is_frozen());
        assert_eq!(l.settled_value(), v.ok());
        assert_eq!(l.freeze(), Err(CC_EFROZEN));
        assert_eq!(l.settled_value(), v.ok());
    }

    #[test]
    fn f11171_checks_green() {
        let s = run_unx11171_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{:?}", s);
    }
}
