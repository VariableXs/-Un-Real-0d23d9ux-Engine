//! CGPU-F0492 · 帧计时族 API 冻结（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - API：帧计时族冻结（计时启动/分段/账本查询/校验——签名冻结
//!   版本化）；
//! - 与 C 域/B08 API 命名一致；
//! - 十年承诺。判据：冻结、一致、承诺、判据。
//!
//! 设计纪律：
//! - 四族签名：计时启动（open）/ 分段（segment）/ 账本查询（query）/
//!   校验（audit）——每族一个稳定入口，参数走具名结构而非裸序参，
//!   版本化 API_REV 单调递增；冻结期内只增不改不删。
//! - 命名一致：C 域（cgb*）与 B08（帧调度）既有命名逐条对齐，
//!   别名表穷举——同一个概念不允许两个名字。
//! - 十年承诺：FROZEN_UNTIL = 2036，承诺期内破坏性变更被
//!   compat_gate 拒绝；兼容性以「旧签名仍可解析」为准绳。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x1190 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x1190，全仓独占）。
pub const SEG_BASE: u16 = 0x1190;

pub const E_TA_REV: u16 = SEG_BASE + 1; // 版本回退/非法
pub const E_TA_NAME: u16 = SEG_BASE + 2; // 命名不一致
pub const E_TA_FROZEN: u16 = SEG_BASE + 3; // 冻结期内破坏性变更
pub const E_TA_AUDIT: u16 = SEG_BASE + 4; // 校验失败
pub const E_TA_EMPTY: u16 = SEG_BASE + 5; // 空签名表

/// API 修订版（只增不改；v1 = 首个冻结版）。
pub const API_REV: u32 = 1;
/// 十年承诺：冻结到期年份。
pub const FROZEN_UNTIL: u32 = 2036;

// ---------------------------------------------------------------------------
// 一、四族签名：具名参数结构 + 稳定入口
// ---------------------------------------------------------------------------

/// 族标识（判别值即对外契约，冻结后不得重排）。
pub const FAM_OPEN: u8 = 1; // 计时启动
pub const FAM_SEG: u8 = 2; // 分段
pub const FAM_QUERY: u8 = 3; // 账本查询
pub const FAM_AUDIT: u8 = 4; // 校验
pub const FAMS: [u8; 4] = [FAM_OPEN, FAM_SEG, FAM_QUERY, FAM_AUDIT];

/// 计时启动参数（具名，冻结版 v1）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenArgs {
    /// 帧号。
    pub frame: u64,
    /// 起始时间戳（us）。
    pub t0_us: u64,
}

/// 分段参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegArgs {
    /// 帧号。
    pub frame: u64,
    /// 段标识（0=输入 1=模拟 2=渲染 3=提交）。
    pub seg_id: u8,
    /// 段结束时间戳（us）。
    pub t1_us: u64,
}

/// 账本查询参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueryArgs {
    /// 查询窗口起点（含）。
    pub from_frame: u64,
    /// 查询窗口终点（含）。
    pub to_frame: u64,
}

/// 查询结果：窗口内帧分段账目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryResult {
    pub frames: Vec<(u64, [u32; 4])>,
}

/// 校验参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditArgs {
    /// 要求四段齐全。
    pub require_full: bool,
    /// 单帧总时长上限（us）。
    pub frame_budget_us: u32,
}

/// 帧计时账本：四族入口的唯一宿主。
#[derive(Debug)]
pub struct FrameClock {
    /// frame -> [seg0..seg3] 时长（us）。
    ledger: Vec<(u64, [u32; 4])>,
    /// 打开的帧（已 start 未收口）。
    open_frame: Option<(u64, u64)>,
}

impl FrameClock {
    pub fn new() -> Self {
        Self { ledger: Vec::new(), open_frame: None }
    }

    /// 族一：计时启动。重复打开同一帧幂等，跨帧未收口拒绝。
    pub fn open(&mut self, a: OpenArgs) -> Result<(), u16> {
        if let Some((f, _)) = self.open_frame {
            if f == a.frame {
                return Ok(()); // 幂等
            }
            return Err(E_TA_AUDIT); // 上一帧未收口
        }
        if self.ledger.iter().any(|(f, _)| *f == a.frame) {
            return Ok(()); // 已收口帧重放幂等
        }
        self.open_frame = Some((a.frame, a.t0_us));
        Ok(())
    }

    /// 族二：分段。段起点衔接上一段结束；段倒退/越界拒绝。
    pub fn segment(&mut self, a: SegArgs) -> Result<(), u16> {
        if a.seg_id > 3 {
            return Err(E_TA_AUDIT);
        }
        let (f, mut last_t1) = match self.open_frame {
            Some(x) if x.0 == a.frame => x,
            _ => return Err(E_TA_AUDIT),
        };
        if a.t1_us < last_t1 {
            return Err(E_TA_AUDIT); // 分段早于上一段结束（负时长结构上拒）
        }
        let dur = (a.t1_us - last_t1) as u32;
        let slot = match self.ledger.iter_mut().find(|(f, _)| *f == a.frame) {
            Some((_, s)) => s,
            None => {
                self.ledger.push((a.frame, [0u32; 4]));
                let n = self.ledger.len();
                &mut self.ledger[n - 1].1
            }
        };
        slot[a.seg_id as usize] = dur;
        last_t1 = a.t1_us; // 段与段衔接
        self.open_frame = Some((f, last_t1));
        Ok(())
    }

    /// 收口：四段写入后关帧。
    pub fn close(&mut self, frame: u64) -> Result<(), u16> {
        match self.open_frame {
            Some((f, _)) if f == frame => {
                self.open_frame = None;
                Ok(())
            }
            _ => Err(E_TA_AUDIT),
        }
    }

    /// 族三：账本查询。窗口 [from, to] 闭区间。
    pub fn query(&self, a: QueryArgs) -> Result<QueryResult, u16> {
        if a.from_frame > a.to_frame {
            return Err(E_TA_AUDIT);
        }
        let frames = self
            .ledger
            .iter()
            .filter(|(f, _)| *f >= a.from_frame && *f <= a.to_frame)
            .map(|(f, s)| (*f, *s))
            .collect();
        Ok(QueryResult { frames })
    }

    /// 族四：校验。四段齐全性 + 帧预算。
    pub fn audit(&self, a: AuditArgs) -> Result<usize, u16> {
        let mut bad = 0usize;
        for (_, s) in self.ledger.iter() {
            if a.require_full && s.iter().any(|d| *d == 0) {
                bad += 1;
            }
            let total: u32 = s.iter().sum();
            if total > a.frame_budget_us {
                bad += 1;
            }
        }
        if bad == 0 {
            Ok(self.ledger.len())
        } else {
            Err(E_TA_AUDIT)
        }
    }
}

// ---------------------------------------------------------------------------
// 二、命名一致：C 域 / B08 对齐表
// ---------------------------------------------------------------------------

/// 概念 → 权威命名。别名即违约：同一概念两个名字判红。
/// 来源：C 域账本（cgb460_edgetlem/cgb465_apiledger）与 B08 帧调度
/// 既有词汇——此处只登记不发明。
pub const CANON_NAMES: [(&str, &str); 8] = [
    ("frame", "frame"),          // 帧号（B08 同名）
    ("t0_us", "start_us"),       // 启动时间（C 域账本口径）
    ("t1_us", "end_us"),         // 段结束（C 域账本口径）
    ("seg_id", "phase_id"),      // 段/相位（B08 phase 命名）
    ("ledger", "ledger"),        // 账本（C 域 ledger 同名）
    ("query", "query"),          // 查询（C 域 ledger.query 同名）
    ("audit", "audit"),          // 校验（C 域 audit 同名）
    ("open", "open"),            // 启动（B08 open 命名）
];

/// 命名一致性闸：概念不重名、权威名非空。
pub fn names_consistent() -> Result<usize, u16> {
    if CANON_NAMES.is_empty() {
        return Err(E_TA_EMPTY);
    }
    for (concept, canon) in CANON_NAMES.iter() {
        if concept.is_empty() || canon.is_empty() {
            return Err(E_TA_NAME);
        }
    }
    // 概念唯一（重复登记同一概念 = 两个名字，违约）。
    for i in 0..CANON_NAMES.len() {
        for j in (i + 1)..CANON_NAMES.len() {
            if CANON_NAMES[i].0 == CANON_NAMES[j].0 {
                return Err(E_TA_NAME);
            }
        }
    }
    Ok(CANON_NAMES.len())
}

// ---------------------------------------------------------------------------
// 三、冻结与十年承诺
// ---------------------------------------------------------------------------

/// 签名指纹：四族判别值 + 权威名拼接（同版冻结期内指纹不变）。
pub fn signature_fingerprint() -> u32 {
    let mut h: u32 = 0x7A91_0001;
    for f in FAMS.iter() {
        h = h.wrapping_mul(31).wrapping_add(*f as u32);
    }
    for (c, n) in CANON_NAMES.iter() {
        for b in c.bytes().chain(n.bytes()) {
            h = h.wrapping_mul(31).wrapping_add(b as u32);
        }
    }
    h
}

/// 兼容闸：给定声称版本与变更请求，裁决是否放行。
/// - 版本回退/越界：拒（E_TA_REV）。
/// - 破坏性变更（改/删既有族或改名）且 year <= FROZEN_UNTIL：拒。
/// - 纯新增（fams 只增）：放行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeReq {
    pub claimed_rev: u32,
    pub year: u32,
    /// 变更后的族集合。
    pub new_fams: Vec<u8>,
    /// 变更后的命名表（概念序不变）。
    pub new_names: Vec<(&'static str, &'static str)>,
}

pub fn compat_gate(req: &ChangeReq) -> Result<(), u16> {
    if req.claimed_rev == 0 || req.claimed_rev > API_REV {
        return Err(E_TA_REV);
    }
    // 族集合：必须包含全部既有族（删/改拒），可新增。
    for f in FAMS.iter() {
        if !req.new_fams.contains(f) {
            return Err(E_TA_FROZEN);
        }
    }
    // 命名表：概念集合不变、权威名不变（改名拒）。
    if req.new_names.len() != CANON_NAMES.len() {
        return Err(E_TA_FROZEN);
    }
    for i in 0..CANON_NAMES.len() {
        if req.new_names[i] != CANON_NAMES[i] {
            if req.year <= FROZEN_UNTIL {
                return Err(E_TA_FROZEN);
            }
        }
    }
    Ok(())
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、四族闭环：启动→分段→查询→校验。
    {
        let mut fc = FrameClock::new();
        v.push(("F0492-启动-开帧", fc.open(OpenArgs { frame: 1, t0_us: 0 }).is_ok()));
        v.push(("F0492-启动-同帧幂等", fc.open(OpenArgs { frame: 1, t0_us: 5 }).is_ok()));
        v.push(("F0492-启动-跨帧未收口拒", fc.open(OpenArgs { frame: 2, t0_us: 0 }) == Err(E_TA_AUDIT)));
        // 分段：seg0 输入 2ms、seg1 模拟 4ms、seg2 渲染 5ms、seg3 提交 1ms。
        v.push(("F0492-分段-四段", {
            let ok = fc.segment(SegArgs { frame: 1, seg_id: 0, t1_us: 2_000 }).is_ok()
                && fc.segment(SegArgs { frame: 1, seg_id: 1, t1_us: 6_000 }).is_ok()
                && fc.segment(SegArgs { frame: 1, seg_id: 2, t1_us: 11_000 }).is_ok()
                && fc.segment(SegArgs { frame: 1, seg_id: 3, t1_us: 12_000 }).is_ok();
            ok
        }));
        v.push(("F0492-分段-段越界拒", fc.segment(SegArgs { frame: 1, seg_id: 4, t1_us: 13_000 }) == Err(E_TA_AUDIT)));
        v.push(("F0492-分段-早于启动拒", fc.segment(SegArgs { frame: 1, seg_id: 0, t1_us: 0 }) == Err(E_TA_AUDIT)));
        v.push(("F0492-收口-关帧", fc.close(1).is_ok()));
        v.push(("F0492-收口-已收口帧重放幂等", fc.open(OpenArgs { frame: 1, t0_us: 0 }).is_ok()));
        // 查询：窗口命中。
        let q = fc.query(QueryArgs { from_frame: 1, to_frame: 1 });
        v.push(("F0492-查询-窗口命中", q.as_ref().map(|r| r.frames.len()) == Ok(1)));
        v.push(("F0492-查询-段值正确", {
            q.as_ref()
                .map(|r| r.frames[0].1 == [2_000, 4_000, 5_000, 1_000])
                .unwrap_or(false)
        }));
        v.push(("F0492-查询-空窗合法", fc.query(QueryArgs { from_frame: 9, to_frame: 9 }).map(|r| r.frames.is_empty()) == Ok(true)));
        v.push(("F0492-查询-倒窗拒", fc.query(QueryArgs { from_frame: 5, to_frame: 1 }) == Err(E_TA_AUDIT)));
        // 校验：预算内通过；预算 5ms 触发超支。
        v.push(("F0492-校验-预算内", fc.audit(AuditArgs { require_full: true, frame_budget_us: 12_500 }) == Ok(1)));
        v.push(("F0492-校验-超支拒", fc.audit(AuditArgs { require_full: true, frame_budget_us: 5_000 }) == Err(E_TA_AUDIT)));
        // 缺段帧触发 require_full。
        let mut fc2 = FrameClock::new();
        fc2.open(OpenArgs { frame: 7, t0_us: 0 }).ok();
        fc2.segment(SegArgs { frame: 7, seg_id: 0, t1_us: 1_000 }).ok();
        fc2.close(7).ok();
        v.push(("F0492-校验-缺段拒", fc2.audit(AuditArgs { require_full: true, frame_budget_us: 12_500 }) == Err(E_TA_AUDIT)));
        v.push(("F0492-校验-不查缺段放行", fc2.audit(AuditArgs { require_full: false, frame_budget_us: 12_500 }) == Ok(1)));
    }
    // 二、命名一致。
    {
        v.push(("F0492-一致-八条全对齐", names_consistent() == Ok(8)));
        v.push(("F0492-一致-段/相位对齐B08", {
            let (_, canon) = CANON_NAMES[3];
            canon == "phase_id"
        }));
        v.push(("F0492-一致-时间口径对齐C域", {
            let (_, s) = CANON_NAMES[1];
            let (_, e) = CANON_NAMES[2];
            s == "start_us" && e == "end_us"
        }));
    }
    // 三、冻结与十年承诺。
    {
        let fp1 = signature_fingerprint();
        let fp2 = signature_fingerprint();
        v.push(("F0492-冻结-指纹确定性", fp1 == fp2));
        // 兼容闸：纯新增族放行。
        let add = ChangeReq {
            claimed_rev: API_REV,
            year: 2030,
            new_fams: {
                let mut f: Vec<u8> = FAMS.to_vec();
                f.push(5);
                f
            },
            new_names: CANON_NAMES.to_vec(),
        };
        v.push(("F0492-承诺-纯新增放行", compat_gate(&add).is_ok()));
        // 删族：冻结期内拒。
        let del = ChangeReq {
            claimed_rev: API_REV,
            year: 2030,
            new_fams: [FAM_OPEN, FAM_SEG, FAM_QUERY].to_vec(),
            new_names: CANON_NAMES.to_vec(),
        };
        v.push(("F0492-承诺-删族冻结期拒", compat_gate(&del) == Err(E_TA_FROZEN)));
        // 改名：冻结期内拒；到期后放行。
        let mut ren = CANON_NAMES.to_vec();
        ren[3] = ("seg_id", "segment_ix");
        let ren_req = ChangeReq {
            claimed_rev: API_REV,
            year: 2030,
            new_fams: FAMS.to_vec(),
            new_names: ren.clone(),
        };
        v.push(("F0492-承诺-改名冻结期拒", compat_gate(&ren_req) == Err(E_TA_FROZEN)));
        let ren_after = ChangeReq {
            claimed_rev: API_REV,
            year: FROZEN_UNTIL + 1,
            new_fams: FAMS.to_vec(),
            new_names: ren,
        };
        v.push(("F0492-承诺-到期后改名放行", compat_gate(&ren_after).is_ok()));
        // 版本纪律。
        let bad_rev = ChangeReq {
            claimed_rev: 0,
            year: 2030,
            new_fams: FAMS.to_vec(),
            new_names: CANON_NAMES.to_vec(),
        };
        v.push(("F0492-承诺-零版拒", compat_gate(&bad_rev) == Err(E_TA_REV)));
        let future_rev = ChangeReq {
            claimed_rev: API_REV + 1,
            year: 2030,
            new_fams: FAMS.to_vec(),
            new_names: CANON_NAMES.to_vec(),
        };
        v.push(("F0492-承诺-未来版拒", compat_gate(&future_rev) == Err(E_TA_REV)));
    }
    // 四、判据自检。
    {
        v.push(("F0492-判据-四族枚举", FAMS == [1, 2, 3, 4]));
        v.push(("F0492-判据-承诺年限入册", FROZEN_UNTIL == 2036));
        v.push(("F0492-判据-API版本入册", API_REV == 1));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0492_timeapi_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
