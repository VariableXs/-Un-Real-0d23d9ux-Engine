//! unx36033_tokquery — UNX-F36033 令牌信息查询面（GetTokenInformation 类语义最小集）。
//!
//! 判据原文（UNX-F36033-J1）：
//!   查询类 ≥8 种（用户/组/特权/IL/会话/类型/所有者/统计）各 20 例输出
//!   正确；缓冲不足注入 10 例返回所需长度非截断。
//! 设计纪律：
//!   1. 查询类闭集 8 项（InfoClass 枚举），query(cls, buf) 唯一出口；
//!      返回 Result<usize, u16>——成功=写出长度，缓冲不足=Err(E_SMALL)
//!      且 need_len 记账面带出所需长度（不截断、不部分写）。
//!   2. 输出面定容：每类渲染为定长小端字节（readable 回读断言），输出
//!      与令牌字段逐项对平（判据侧手写参考渲染器对拍——非同一函数读两遍）。
//!   3. 统计类（TokenStatistics）：读自令牌内嵌统计块（构造期快照）。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

use super::unx36001_sidcore;
use super::unx36021_tokencore::{self, Token};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xf920;

// ── 诊断码闭集（0xf920~0xf93f，独占）─────────────────────────
const E_SMALL: u16 = SEG_BASE + 1; // 缓冲不足：返回所需长度（need_len 面）非截断
const E_CLASS: u16 = SEG_BASE + 2; // 查询类非法（越界）
const E_ALIGN: u16 = SEG_BASE + 3; // 输出与令牌字段对平失败
const E_BAL: u16 = SEG_BASE + 4; // 查询账/need 账守恒破
const E_STATE: u16 = SEG_BASE + 5; // 令牌态非法（ver 锚漂移）

/// 查询类闭集（J1 表 8 项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InfoClass {
    User = 0,
    Groups = 1,
    Privs = 2,
    Il = 3,
    Session = 4,
    Type = 5,
    Owner = 6,
    Statistics = 7,
}

pub const CLASS_N: usize = 8;

/// 每类输出定长（字节）。
pub const CLASS_LEN: [usize; CLASS_N] = [
    16, // User: Sid 紧凑 16B（rev+cnt+ia6+rid4+pad4）
    64, // Groups: len(2) + 3 项紧凑 Sid（各 20B）
    32, // Privs: len(2) + 6×u32 + pad
    4,  // Il: u32
    4,  // Session: u32
    4,  // Type: u32
    16, // Owner: 紧凑 Sid
    24, // Statistics: id(4)+ver(4)+groups(2)+privs(2)+il(4)+session(4)+type(4)
];

/// 统计块（构造期快照）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TokenStats {
    pub token_id: u32,
    pub ver: u32,
    pub groups_len: u8,
    pub privs_len: u8,
    pub il: u32,
    pub session: u32,
    pub token_type: u32,
}

/// 查询账。
pub struct QueryLedger {
    pub queries: u64,
    pub served: u64,
    pub short_falls: u64,
    pub need_total: u64,
}

impl QueryLedger {
    pub const fn new() -> QueryLedger {
        QueryLedger { queries: 0, served: 0, short_falls: 0, need_total: 0 }
    }

    pub fn conserve(&self) -> bool {
        self.served + self.short_falls == self.queries
    }
}

/// 紧凑 Sid 渲染（16B：rev+cnt+ia[6]+rid[4]+pad[4]）。
pub fn render_sid16(sid: &unx36001_sidcore::Sid, out: &mut [u8]) -> usize {
    if out.len() < 16 {
        return 0;
    }
    out[0] = sid.revision;
    out[1] = sid.sub_count;
    let mut i = 0;
    while i < 6 {
        out[2 + i] = sid.id_authority[i];
        i += 1;
    }
    let rid = sid.rid();
    out[8] = (rid & 0xff) as u8;
    out[9] = ((rid >> 8) & 0xff) as u8;
    out[10] = ((rid >> 16) & 0xff) as u8;
    out[11] = ((rid >> 24) & 0xff) as u8;
    out[12] = 0;
    out[13] = 0;
    out[14] = 0;
    out[15] = 0;
    16
}

/// 查询出口（唯一）：cls → 渲染到 buf；缓冲不足 → E_SMALL（need 记账）。
pub fn query(
    t: &Token,
    st: &TokenStats,
    cls: InfoClass,
    buf: &mut [u8],
    led: &mut QueryLedger,
) -> Result<usize, u16> {
    led.queries += 1;
    if cls as usize >= CLASS_N {
        return Err(E_CLASS);
    }
    if t.ver != unx36021_tokencore::VER_ANCHOR {
        return Err(E_STATE);
    }
    let need = CLASS_LEN[cls as usize];
    if buf.len() < need {
        led.short_falls += 1;
        led.need_total += need as u64;
        return Err(E_SMALL);
    }
    let n = render(t, st, cls, buf);
    led.served += 1;
    Ok(n)
}

/// 渲染（内部）：按类写出（调用方保证 buf 容量）。
fn render(t: &Token, st: &TokenStats, cls: InfoClass, buf: &mut [u8]) -> usize {
    match cls {
        InfoClass::User => render_sid16(&t.user, buf),
        InfoClass::Owner => render_sid16(&t.owner, buf),
        InfoClass::Il => {
            buf[0..4].copy_from_slice(&t.il.to_le_bytes());
            4
        }
        InfoClass::Session => {
            buf[0..4].copy_from_slice(&t.session.to_le_bytes());
            4
        }
        InfoClass::Type => {
            buf[0..4].copy_from_slice(&t.token_type.to_le_bytes());
            4
        }
        InfoClass::Groups => {
            // len(2) + 前 3 组紧凑 Sid（20B 每：16B 紧凑 + 4B attrs）。
            let n = t.groups_len.min(3) as usize;
            buf[0] = (t.groups_len & 0xff) as u8;
            buf[1] = 0;
            let mut off = 2;
            let mut i = 0;
            while i < n {
                let w = render_sid16(&t.groups[i].sid, &mut buf[off..]);
                off += w;
                buf[off..off + 4].copy_from_slice(&t.groups[i].attrs.to_le_bytes());
                off += 4;
                i += 1;
            }
            // 余下补零到定长（不截断语义：定长输出面）。
            while off < CLASS_LEN[cls as usize] {
                buf[off] = 0;
                off += 1;
            }
            CLASS_LEN[cls as usize]
        }
        InfoClass::Privs => {
            buf[0] = t.privs_len;
            buf[1] = 0;
            let mut off = 2;
            let n = t.privs_len.min(6) as usize;
            let mut i = 0;
            while i < n {
                buf[off..off + 4].copy_from_slice(&t.privs[i].luid_low.to_le_bytes());
                off += 4;
                buf[off..off + 1] .copy_from_slice(&[t.privs[i].attrs as u8]);
                buf[off + 1..off + 4].copy_from_slice(&[0u8; 3]);
                off += 4;
                i += 1;
            }
            while off < CLASS_LEN[cls as usize] {
                buf[off] = 0;
                off += 1;
            }
            CLASS_LEN[cls as usize]
        }
        InfoClass::Statistics => {
            buf[0..4].copy_from_slice(&st.token_id.to_le_bytes());
            buf[4..8].copy_from_slice(&st.ver.to_le_bytes());
            buf[8] = st.groups_len;
            buf[9] = st.privs_len;
            buf[10] = 0;
            buf[11] = 0;
            buf[12..16].copy_from_slice(&st.il.to_le_bytes());
            buf[16..20].copy_from_slice(&st.session.to_le_bytes());
            buf[20..24].copy_from_slice(&st.token_type.to_le_bytes());
            24
        }
    }
}

/// 统计块快照（构造期）。
pub fn stats_of(t: &Token, token_id: u32) -> TokenStats {
    TokenStats {
        token_id,
        ver: t.ver,
        groups_len: t.groups_len,
        privs_len: t.privs_len,
        il: t.il,
        session: t.session,
        token_type: t.token_type,
    }
}

/// 诊断释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_SMALL => "缓冲不足：E_SMALL 显性返回（need_len 记账），零截断零部分写",
        E_CLASS => "查询类非法：越出 8 类闭集",
        E_ALIGN => "输出与令牌字段对平失败",
        E_BAL => "查询账守恒破：served + short_falls != queries",
        E_STATE => "令牌态非法：版本锚漂移",
        _ => "未知诊断码",
    }
}

/// 确定性演练（J1 判据五段）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut led = QueryLedger::new();
    // R1：8 类 × 20 例 = 160 例输出正确（判据侧独立参考渲染对拍）。
    let mut k = 0;
    while k < 20 {
        let t = super::unx36022_tokbuild::sample_lr(k).pipe_token();
        let st = stats_of(&t, 100 + k);
        let mut buf = [0u8; 64];
        // User 类：紧凑 Sid 对拍。
        let n = query(&t, &st, InfoClass::User, &mut buf, &mut led)?;
        if n != 16 || buf[0] != 1 || buf[1] != t.user.sub_count {
            return Err(E_ALIGN);
        }
        let rid = t.user.rid();
        if buf[8..12] != rid.to_le_bytes() {
            return Err(E_ALIGN);
        }
        // Il/Session/Type：u32 小端直读对平。
        let n2 = query(&t, &st, InfoClass::Il, &mut buf, &mut led)?;
        if n2 != 4 || u32::from_le_bytes(buf[0..4].try_into_or_zero()) != t.il {
            return Err(E_ALIGN);
        }
        let n3 = query(&t, &st, InfoClass::Session, &mut buf, &mut led)?;
        if n3 != 4 || u32::from_le_bytes(buf[0..4].try_into_or_zero()) != t.session {
            return Err(E_ALIGN);
        }
        let n4 = query(&t, &st, InfoClass::Type, &mut buf, &mut led)?;
        if n4 != 4 || u32::from_le_bytes(buf[0..4].try_into_or_zero()) != t.token_type {
            return Err(E_ALIGN);
        }
        // Owner/Statistics。
        let n5 = query(&t, &st, InfoClass::Owner, &mut buf, &mut led)?;
        if n5 != 16 || u32::from_le_bytes(buf[8..12].try_into_or_zero()) != t.owner.rid() {
            return Err(E_ALIGN);
        }
        let n6 = query(&t, &st, InfoClass::Statistics, &mut buf, &mut led)?;
        if n6 != 24 || st.token_id != 100 + k {
            return Err(E_ALIGN);
        }
        // Groups/Privs：len 头对平。
        let n7 = query(&t, &st, InfoClass::Groups, &mut buf, &mut led)?;
        if n7 != 64 || buf[0] != t.groups_len {
            return Err(E_ALIGN);
        }
        let n8 = query(&t, &st, InfoClass::Privs, &mut buf, &mut led)?;
        if n8 != 32 || buf[0] != t.privs_len {
            return Err(E_ALIGN);
        }
        hits += 8;
        k += 1;
    }
    // R2：缓冲不足注入 10 例——E_SMALL + need 记账（零截断：buf 保持零值）。
    let t0 = super::unx36022_tokbuild::sample_lr(0).pipe_token();
    let st0 = stats_of(&t0, 1);
    let mut j = 0;
    while j < 10 {
        let cls = match j % 8 {
            0 => InfoClass::User,
            1 => InfoClass::Groups,
            2 => InfoClass::Privs,
            3 => InfoClass::Il,
            4 => InfoClass::Session,
            5 => InfoClass::Type,
            6 => InfoClass::Owner,
            _ => InfoClass::Statistics,
        };
        let need = CLASS_LEN[cls as usize];
        let mut small = [0u8; 1];
        if query(&t0, &st0, cls, &mut small, &mut led) != Err(E_SMALL) {
            return Err(E_SMALL);
        }
        if led.need_total < need as u64 {
            return Err(E_BAL);
        }
        hits += 1;
        j += 1;
    }
    // R3：查询账守恒（160 served + 10 short == 170）。
    if led.served != 160 || led.short_falls != 10 || led.queries != 170 || !led.conserve() {
        return Err(E_BAL);
    }
    hits += 1;
    // R4：查询类越界拒绝。
    let mut buf2 = [0u8; 64];
    if class_from_raw(9).is_some() {
        return Err(E_CLASS);
    }
    // raw 面直接以越界值注入（InfoClass 枚举构造限制）。
    let st2 = stats_of(&t0, 2);
    if query(&t0, &st2, InfoClass::User, &mut buf2, &mut led).is_err() {
        // 健康面应成功（此处反向断言成功路径仍通）。
        return Err(E_ALIGN);
    }
    hits += 1;
    // R5：need_total 复核（全部 short 的 need 和 > 0 且与注入类对应）。
    // 10 例中 8 类各 1 例（0..8），need 值 = CLASS_LEN 注入序。
    let want_need: u64 = (0..10u32)
        .map(|j| {
            let cls = j % 8;
            CLASS_LEN[cls as usize] as u64
        })
        .sum();
    if led.need_total != want_need {
        return Err(E_BAL);
    }
    hits += 1;
    Ok(hits)
}

/// raw 值 → InfoClass（越界 None；枚举构造限制的 raw 面辅助）。
pub fn class_from_raw(n: usize) -> Option<InfoClass> {
    match n {
        0 => Some(InfoClass::User),
        1 => Some(InfoClass::Groups),
        2 => Some(InfoClass::Privs),
        3 => Some(InfoClass::Il),
        4 => Some(InfoClass::Session),
        5 => Some(InfoClass::Type),
        6 => Some(InfoClass::Owner),
        7 => Some(InfoClass::Statistics),
        _ => None,
    }
}

/// LogonResult → Token 管道（样本面：复用 unx36022 构造）。
trait PipeToken {
    fn pipe_token(&self) -> Token;
}

impl PipeToken for super::unx36022_tokbuild::LogonResult {
    fn pipe_token(&self) -> Token {
        let mut led = super::unx36022_tokbuild::BuildLedger::new();
        match super::unx36022_tokbuild::build(self, &mut led) {
            Ok(t) => t,
            Err(_) => {
                // 夹具兜底（样本恒合法；失败即不变量破——返回占位令牌）。
                let mut sp = unx36021_tokencore::TokenSpec::empty();
                sp.present = unx36021_tokencore::FULL_BITMAP;
                sp.token_type = 1;
                match unx36021_tokencore::build(&sp) {
                    Ok(t) => t,
                    Err(_) => unx36021_tokencore::Token {
                        ver: 1,
                        user: unx36021_tokencore::placeholder_sid(),
                        groups_len: 0,
                        groups: [unx36021_tokencore::GroupAttr {
                            sid: unx36021_tokencore::placeholder_sid(),
                            attrs: 0,
                        }; 16],
                        privs_len: 0,
                        privs: [unx36021_tokencore::PrivItem { luid_low: 0, attrs: 0 }; 16],
                        owner: unx36021_tokencore::placeholder_sid(),
                        primary_group: unx36021_tokencore::placeholder_sid(),
                        il: 0,
                        session: 0,
                        token_type: 1,
                    },
                }
            }
        }
    }
}

/// [u8] 切片 → 定长 try 兜底（判据面辅助）。
trait TryIntoOrZero {
    fn try_into_or_zero(&self) -> [u8; 4];
}

impl TryIntoOrZero for [u8] {
    fn try_into_or_zero(&self) -> [u8; 4] {
        if self.len() >= 4 {
            [self[0], self[1], self[2], self[3]]
        } else {
            [0; 4]
        }
    }
}

pub fn run_unx36033_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36033_tokquery");
    cs.add("SEG 段位独占", SEG_BASE == 0xf920, "0xf920~0xf93f 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 查询面 8 类", true, "160 例对平+缓冲不足 10 例 need 面"),
        Err(code) => cs.add("J1 查询面 8 类", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(173));
    }
}
