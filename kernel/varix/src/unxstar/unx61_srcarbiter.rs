//! unx61_srcarbiter —— UNX-F0061 三源冲突仲裁策略器（配置来源优先级仲裁面）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0061 · 三源冲突仲裁策略器 · 目标 320 行 · 已深化
//! 判据原文：UNX-F0061-J1 构造 100 组合成冲突样本，仲裁结果与设计优先级矩阵
//!           100% 一致，冲突有账
//! 组件：三源模型（固件源 F/策略源 P/用户源 U，优先级矩阵 U>P>F 逐字段闭集）
//!       + 字段级仲裁器（同字段多源写入 → 矩阵裁定唯一胜者，败者留痕）
//!       + 冲突账（append-only：字段/参与源/胜者/败者四列）
//!       + 100 组样本对拍核（LCG 确定性生成，与矩阵逐条核对 100/100）
//!       + 注入核（未知源/同源自冲突/越界字段三类注入必抓）。
//! 职责定位：配置合成前的唯一仲裁权威——B6 域串口面与 B7 降级面共享本裁定；
//!   优先级矩阵冻结使仲裁结果可复算可对拍；冲突账使每次分歧可追。
//! 性能逐项分解：仲裁 O(参与源数)；账开户 O(1) 均摊；样本对拍 O(样本数×字段数)。
//!   全整数确定性（LCG，零墙钟）。
//! 诊断码段：0xE080（0xE081~0xE088 闭集；空闲独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const SA_VERSION: &str = "unx61_srcarbiter-v1";

/// 诊断段基址（0xE080 段 · 本模块独占）。
pub const SA_SEG_BASE: u16 = 0xE080;

/// XcCode 闭集（0xE081~0xE088）。
pub const XC_SRC_BAD: u32 = 0xE081;
pub const XC_FIELD_BAD: u32 = 0xE082;
pub const XC_SELF_CONFLICT: u32 = 0xE083;
pub const XC_ARBITER_DRIFT: u32 = 0xE084;
pub const XC_MIN_SAMPLES: u32 = 0xE085;
pub const XC_INJECT_MISSED: u32 = 0xE086;
pub const XC_LEDGER_BROKEN: u32 = 0xE087;
pub const XC_NO_WINNER: u32 = 0xE088;

/// 中文诊断文本（闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_SRC_BAD => ("参与仲裁的源不在 F/P/U 三源闭集", "核对来源标记生成器"),
        XC_FIELD_BAD => ("仲裁字段号越界（超出字段表闭集）", "按字段注册表 0~15 闭集"),
        XC_SELF_CONFLICT => ("同一源内部自冲突（同源双值同字段）", "同源多值应先在源内合成"),
        XC_ARBITER_DRIFT => ("仲裁结果与优先级矩阵不一致（仲裁漂移）", "复核矩阵表与比较器实现"),
        XC_MIN_SAMPLES => ("对拍样本少于最小台账（100 组）", "补全样本生成器再测"),
        XC_INJECT_MISSED => ("注入故障未被检出（守卫失守）", "修复注入核比对逻辑后重跑"),
        XC_LEDGER_BROKEN => ("冲突账 append-only 性被破坏（序号跳号）", "检查开户路径是否绕过账务封装"),
        XC_NO_WINNER => ("仲裁无胜者（全源缺值）", "全缺值字段走默认值路径而非仲裁"),
        _ => ("未知诊断码", "查 0xE080 段闭集登记表"),
    }
}

/// 确定性伪随机（LCG）。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }
}

/// 三源闭集（设计优先级：U=2 > P=1 > F=0）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Src {
    /// 固件源（最低优先）。
    Firm,
    /// 策略源。
    Policy,
    /// 用户源（最高优先）。
    User,
}

impl Src {
    /// 优先级秩（越大越优先，矩阵冻结）。
    pub const fn rank(self) -> u8 {
        match self {
            Src::Firm => 0,
            Src::Policy => 1,
            Src::User => 2,
        }
    }
    pub const fn num(self) -> u8 {
        match self {
            Src::Firm => 0,
            Src::Policy => 1,
            Src::User => 2,
        }
    }
    pub fn from_num(n: u8) -> Option<Src> {
        match n {
            0 => Some(Src::Firm),
            1 => Some(Src::Policy),
            2 => Some(Src::User),
            _ => None,
        }
    }
}

/// 字段值（全整数确定性；字符串以 32 位哈希参与，零堆比较）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldVal {
    pub num: u64,
}

/// 一条待仲裁写入。
#[derive(Clone, Copy, Debug)]
pub struct Claim {
    /// 字段号（0~15 闭集）。
    pub field: u8,
    pub src: Src,
    pub val: FieldVal,
}

/// 冲突账一条（append-only）。
#[derive(Clone, Copy, Debug)]
pub struct ConflictRow {
    pub field: u8,
    /// 参与源位图 bit0=F bit1=P bit2=U。
    pub parties: u8,
    /// 胜者源。
    pub winner: u8,
    /// 胜者值。
    pub val: u64,
    /// 败者计数。
    pub losers: u8,
}

/// 仲裁结果错误（显性回退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArbError {
    /// 字段号越界。
    FieldBad(u8),
    /// 参与源为空。
    NoClaim,
    /// 同源自冲突。
    SelfConflict(u8),
    /// 账序跳号。
    LedgerBroken(u32),
    /// 全源缺值（不应进仲裁）。
    NoWinner,
}

/// 三源冲突仲裁器。
pub struct Arbiter {
    /// 字段胜出值表（0~15）。
    pub decided: [Option<FieldVal>; 16],
    /// 字段胜出源表。
    pub winner_src: [Option<u8>; 16],
    rows: Vec<ConflictRow>,
    next_seq: u32,
}

impl Arbiter {
    pub fn new() -> Arbiter {
        Arbiter {
            decided: [None; 16],
            winner_src: [None; 16],
            rows: Vec::new(),
            next_seq: 1,
        }
    }

    pub fn ledger_ok(&self) -> bool {
        self.next_seq as usize == self.rows.len() + 1
    }

    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    pub fn row_at(&self, i: usize) -> Option<ConflictRow> {
        self.rows.get(i).copied()
    }

    /// 一次字段仲裁：取同字段全部 claim，按 rank 降序裁定唯一胜者。
    /// 有冲突（≥2 源）才落账；单源直接生效不落账。
    pub fn arbitrate(&mut self, claims: &[Claim]) -> Result<(), ArbError> {
        if claims.is_empty() {
            return Err(ArbError::NoClaim);
        }
        let field = claims[0].field;
        if field > 15 {
            return Err(ArbError::FieldBad(field));
        }
        // 源闭集与自冲突检查
        let mut seen: [Option<Src>; 3] = [None; 3];
        let mut parties = 0u8;
        for c in claims {
            if c.field != field {
                return Err(ArbError::FieldBad(c.field));
            }
            let slot = c.src.num() as usize;
            if seen[slot].is_some() {
                return Err(ArbError::SelfConflict(field));
            }
            seen[slot] = Some(c.src);
            parties |= 1 << slot;
        }
        // rank 降序线性扫描裁定（矩阵：U>P>F）
        let mut best: Option<Claim> = None;
        let mut losers = 0u8;
        for c in claims {
            match best {
                None => best = Some(*c),
                Some(b) => {
                    if c.src.rank() > b.src.rank() {
                        best = Some(*c);
                        losers += 1;
                    } else {
                        losers += 1;
                    }
                }
            }
        }
        let win = match best {
            Some(w) => w,
            None => return Err(ArbError::NoWinner),
        };
        // 多源冲突才落账（J1：冲突有账）
        if claims.len() >= 2 {
            let seq = self.next_seq;
            self.rows.push(ConflictRow {
                field,
                parties,
                winner: win.src.num(),
                val: win.val.num,
                losers,
            });
            self.next_seq = seq + 1;
        }
        self.decided[field as usize] = Some(win.val);
        self.winner_src[field as usize] = Some(win.src.num());
        Ok(())
    }
}

/// 100 组合成冲突样本对拍核（LCG 确定性；与矩阵逐条核对）。
/// 返回 (对拍一致数, 冲突账行数)。
pub fn sample_100() -> (u32, u32) {
    let mut l = Lcg::new(0x61_61);
    let mut agree = 0u32;
    let mut rows_total = 0u32;
    for _ in 0..100 {
        // 每样本 2~3 源同字段竞争
        let field = l.below(16) as u8;
        let nsrc = 2 + l.below(2) as usize;
        let mut claims: Vec<Claim> = Vec::new();
        let mut used: [bool; 3] = [false; 3];
        let mut k = 0;
        while k < nsrc {
            let sn = l.below(3) as usize;
            if used[sn] {
                continue;
            }
            used[sn] = true;
            k += 1;
            let src = match Src::from_num(sn as u8) {
                Some(s) => s,
                None => continue,
            };
            claims.push(Claim {
                field,
                src,
                val: FieldVal {
                    num: l.next_u64(),
                },
            });
        }
        let mut arb = Arbiter::new();
        if arb.arbitrate(&claims).is_err() {
            continue;
        }
        // 矩阵复算：取参与源中 rank 最大者的值
        let mut expect: Option<(u8, u64)> = None;
        for c in &claims {
            let r = c.src.rank();
            let put = match expect {
                None => true,
                Some((er, _)) => r > er,
            };
            if put {
                expect = Some((r, c.val.num));
            }
        }
        // 冲突账核对：多源必有账且胜者=矩阵
        let row_ok = if claims.len() >= 2 {
            match (arb.row_at(0), arb.winner_src[field as usize]) {
                (Some(r), Some(ws)) => {
                    // 修正（真缺陷·恒真式）：原写 `rows_total >= 0`——`rows_total`
                    // 是 `u32`，无符号比较下界恒 0，该子句**永真**，等于没写。
                    // 它的本意是「本轮之前已落过账」，但 `rows_total` 在比较
                    // 之后才 `+= arb.rows()`（见下方 311 行），首轮合法值就是 0，
                    // 因此任何 `> 0` 的写法都会把首轮判红。正确的性质只有两条：
                    // 行归属字段与字段一致、行记录的胜者与仲裁账胜者一致。
                    r.field == field && ws == r.winner
                }
                _ => false,
            }
        } else {
            arb.rows() == 0
        };
        let win_ok = match (expect, arb.decided[field as usize]) {
            (Some((_, ev)), Some(dv)) => ev == dv.num,
            _ => false,
        };
        if win_ok && row_ok {
            agree += 1;
        }
        rows_total += arb.rows() as u32;
    }
    (agree, rows_total)
}

/// 注入核：三类非法输入必抓。
pub fn inject_probe() -> Result<bool, u32> {
    let mut caught = 0u32;
    // 越界字段
    let mut a = Arbiter::new();
    if a.arbitrate(&[Claim {
        field: 16,
        src: Src::User,
        val: FieldVal { num: 1 },
    }])
    .is_err()
    {
        caught += 1;
    }
    // 自冲突
    let mut a = Arbiter::new();
    if a.arbitrate(&[
        Claim {
            field: 0,
            src: Src::User,
            val: FieldVal { num: 1 },
        },
        Claim {
            field: 0,
            src: Src::User,
            val: FieldVal { num: 2 },
        },
    ])
    .is_err()
    {
        caught += 1;
    }
    // 空 claim
    let mut a = Arbiter::new();
    if a.arbitrate(&[]).is_err() {
        caught += 1;
    }
    // 合法三源全序必须放行且 U 胜
    let mut a = Arbiter::new();
    let r = a.arbitrate(&[
        Claim {
            field: 3,
            src: Src::Firm,
            val: FieldVal { num: 10 },
        },
        Claim {
            field: 3,
            src: Src::User,
            val: FieldVal { num: 30 },
        },
        Claim {
            field: 3,
            src: Src::Policy,
            val: FieldVal { num: 20 },
        },
    ]);
    if r.is_ok() && a.decided[3].unwrap_or(FieldVal { num: 0 }).num == 30 {
        caught += 1;
    }
    if caught != 4 {
        return Err(XC_INJECT_MISSED as u32);
    }
    Ok(true)
}

/// 域自检（`run_unx61_checks`）：J1 判据逐条可失败断言。
pub fn run_unx61_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx61");

    // C61-01 矩阵全序：U>P>F 逐对裁定一致
    {
        let mut a = Arbiter::new();
        let r = a.arbitrate(&[
            Claim {
                field: 0,
                src: Src::Firm,
                val: FieldVal { num: 1 },
            },
            Claim {
                field: 0,
                src: Src::Policy,
                val: FieldVal { num: 2 },
            },
            Claim {
                field: 0,
                src: Src::User,
                val: FieldVal { num: 3 },
            },
        ]);
        let ok = r.is_ok()
            && a.decided[0].unwrap_or(FieldVal { num: 0 }).num == 3
            && a.winner_src[0] == Some(Src::User.num());
        s.add("C61-01-矩阵全序裁定一致", ok, if ok { "" } else { "仲裁漂移" });
    }

    // C61-02 两源冲突：Policy 压 Firm
    {
        let mut a = Arbiter::new();
        let _ = a.arbitrate(&[
            Claim {
                field: 5,
                src: Src::Firm,
                val: FieldVal { num: 10 },
            },
            Claim {
                field: 5,
                src: Src::Policy,
                val: FieldVal { num: 20 },
            },
        ]);
        let ok = a.decided[5].unwrap_or(FieldVal { num: 0 }).num == 20
            && a.rows() == 1;
        s.add("C61-02-两源冲突落账正确", ok, if ok { "" } else { "两源裁定/账失守" });
    }

    // C61-03 冲突账行内容可追（parties/winner/losers）
    {
        let mut a = Arbiter::new();
        let _ = a.arbitrate(&[
            Claim {
                field: 7,
                src: Src::Firm,
                val: FieldVal { num: 1 },
            },
            Claim {
                field: 7,
                src: Src::User,
                val: FieldVal { num: 3 },
            },
        ]);
        let ok = match a.row_at(0) {
            Some(r) => r.field == 7 && r.parties == 0b101 && r.winner == Src::User.num() && r.losers == 1,
            None => false,
        };
        s.add("C61-03-冲突账四列可追", ok, if ok { "" } else { "账列缺失" });
    }

    // C61-04 单源不落账（无冲突无账）
    {
        let mut a = Arbiter::new();
        let _ = a.arbitrate(&[Claim {
            field: 9,
            src: Src::Firm,
            val: FieldVal { num: 8 },
        }]);
        let ok = a.rows() == 0
            && a.decided[9].unwrap_or(FieldVal { num: 0 }).num == 8
            && a.ledger_ok();
        s.add("C61-04-单源无账直生效", ok, if ok { "" } else { "单源误落账" });
    }

    // C61-05 100 组对拍 100/100 与矩阵一致（J1 核心）
    {
        let (agree, rows) = sample_100();
        let ok = agree == 100 && rows >= 100;
        s.add("C61-05-百组对拍100/100", ok, if ok { "" } else { "对拍未全绿" });
    }

    // C61-06 注入三类必抓
    {
        let ok = inject_probe() == Ok(true);
        s.add("C61-06-注入三类全抓", ok, if ok { "" } else { "注入核漏检" });
    }

    // C61-07 账序守恒
    {
        let mut a = Arbiter::new();
        for f in 0..6u8 {
            let _ = a.arbitrate(&[
                Claim {
                    field: f,
                    src: Src::Firm,
                    val: FieldVal { num: f as u64 },
                },
                Claim {
                    field: f,
                    src: Src::User,
                    val: FieldVal { num: f as u64 + 100 },
                },
            ]);
        }
        let ok = a.ledger_ok() && a.rows() == 6;
        s.add("C61-07-账序守恒", ok, if ok { "" } else { "append-only 被破坏" });
    }

    s
}
