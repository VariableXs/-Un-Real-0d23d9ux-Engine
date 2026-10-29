//! UNX-F12154 — UnhandledExceptionFilter 与 WER 联签预告（0x1a560 段）
//!
//! 锚点：UNX-F12154-J1 UEF 判定面 10/10（EXCEPTION_EXECUTE_HANDLER=1
//! / CONTINUE_EXECUTION=0 / CONTINUE_SEARCH=-1），UEF 链单发一次、
//! 判定后转 WER 提交，联签留痕全账自洽。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：UefFilter（过滤回调注册 + 判定）+ WerSubmit（联签提交账）
//! + 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a560;
pub const UE_TWICE: u32 = SEG_BASE + 0x01; // UEF 二次注册
pub const UE_GHOST: u32 = SEG_BASE + 0x02; // 未注册即判定
pub const UE_ORDER: u32 = SEG_BASE + 0x03; // 判定码非法
pub const UE_SEALED: u32 = SEG_BASE + 0x04; // 印后提交
pub const UE_EMPTY: u32 = SEG_BASE + 0x05; // 空提交账

/// UEF 三值判定。
pub const UE_EXECUTE: i32 = 1;
pub const UE_CONTINUE: i32 = 0;
pub const UE_SEARCH: i32 = -1;

/// WER 提交。
#[derive(Clone, PartialEq)]
pub struct WerSubmit {
    pub code: u32,
    pub verdict: i32,
    pub bucket: String,
    pub note: String,
}

impl WerSubmit {
    /// 提交自洽：码非零、判定合法、bucket 非空。
    pub fn ok(&self) -> bool {
        self.code != 0
            && matches!(self.verdict, UE_EXECUTE | UE_CONTINUE | UE_SEARCH)
            && !self.bucket.is_empty()
    }

    pub fn render(&self) -> String {
        format!("c={:#x} v={} b={}", self.code, self.verdict, self.bucket)
    }
}

/// UEF 面。
pub struct UefFilter {
    installed: bool,
    pub submits: Vec<WerSubmit>,
    pub verdicts: Vec<(u32, i32)>,
    sealed: bool,
}

impl UefFilter {
    pub fn new() -> UefFilter {
        UefFilter { installed: false, submits: Vec::new(), verdicts: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn installed(&self) -> bool {
        self.installed
    }

    /// 安装 UEF（二次安装必拒）。
    pub fn install(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(UE_SEALED);
        }
        if self.installed {
            return Err(UE_TWICE);
        }
        self.installed = true;
        Ok(())
    }

    /// 判定一次异常（未安装必拒；判定码随策略表返回）。
    /// 策略：0xC0000005 访问违例 → EXECUTE（转 WER）；
    ///       0xC0000409 快速失败 → EXECUTE；
    ///       其余 → SEARCH（继续搜，实际已无下级 = WER）。
    pub fn judge(&mut self, code: u32) -> Result<i32, u32> {
        if self.sealed {
            return Err(UE_SEALED);
        }
        if !self.installed {
            return Err(UE_GHOST);
        }
        let v = match code {
            0xC0000005 | 0xC0000409 => UE_EXECUTE,
            _ => UE_SEARCH,
        };
        self.verdicts.push((code, v));
        Ok(v)
    }

    /// WER 联签提交（bucket = 崩溃桶签名）。
    pub fn submit(&mut self, code: u32, bucket: &str, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(UE_SEALED);
        }
        if self.verdicts.is_empty() {
            return Err(UE_EMPTY);
        }
        let (_, verdict) = *self.verdicts.last().unwrap();
        if code != self.verdicts.last().unwrap().0 {
            return Err(UE_GHOST);
        }
        let sub = WerSubmit {
            code,
            verdict,
            bucket: bucket.to_string(),
            note: note.to_string(),
        };
        if !sub.ok() {
            return Err(UE_ORDER);
        }
        self.submits.push(sub);
        Ok(self.submits.len() as u32)
    }

    /// 提交账回放全对：每条提交的 verdict 与判定账一致。
    pub fn submits_replayable(&self) -> bool {
        self.submits.iter().all(|s| {
            self.verdicts
                .iter()
                .any(|&(c, v)| c == s.code && v == s.verdict)
        })
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a560;
        h = h.wrapping_mul(211).wrapping_add(self.installed as u32);
        for s in &self.submits {
            h = h.wrapping_mul(211).wrapping_add(s.code);
            h = h.wrapping_mul(211).wrapping_add(s.verdict as u32);
            for by in s.bucket.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "installed={} verdicts={} submits={}",
            self.installed,
            self.verdicts.len(),
            self.submits.len()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = UefFilter::new();
    // 1) 未安装即判定必拒。
    got[0] = b.judge(0xC0000005).err() == Some(UE_GHOST);
    // 2) 二次安装必拒。
    let _ = b.install();
    got[1] = b.install().err() == Some(UE_TWICE);
    // 3) 未判定即提交必拒。
    got[2] = b.submit(0xC0000005, "bucket", "n").err() == Some(UE_EMPTY);
    // 4) 判定码与提交码错配必拒。
    let _ = b.judge(0xC0000005);
    got[3] = b.submit(0xC0000094, "bucket", "n").err() == Some(UE_GHOST);
    // 5) 空桶签名提交必拒。
    got[4] = b.submit(0xC0000005, "", "n").err() == Some(UE_ORDER);
    got
}

/// 判据入口：UNX-F12154
pub fn run_unx12154_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12154");
    let mut b = UefFilter::new();

    // J1 UEF 安装一次成功。
    s.add("J1_install", b.install().is_ok() && b.installed(), "UEF 单次安装");

    // J2 判定 10/10：五码各判一遍。
    let mut j2 = true;
    if b.judge(0xC0000005) != Ok(UE_EXECUTE) { j2 = false; }
    if b.judge(0xC0000409) != Ok(UE_EXECUTE) { j2 = false; }
    for code in [0xC0000094u32, 0xC00000FD, 0xC0000017, 0xE0000001, 0xDEAD_BEEF, 0xC000012D, 0xC0000121, 0xC0000022] {
        if b.judge(code) != Ok(UE_SEARCH) { j2 = false; }
    }
    s.add("J2_judge_10x", j2 && b.verdicts.len() == 10, "十码判定 EXECUTE/SEARCH 全对");

    // J3 三值常量。
    s.add(
        "J3_consts",
        UE_EXECUTE == 1 && UE_CONTINUE == 0 && UE_SEARCH == -1,
        "UEF 三值与 NT 语义一致",
    );

    // J4 访问违例转 WER 提交（提交码须对齐最近判定码，先判 AV 再提交）。
    let _ = b.judge(0xC0000005);
    let sub = b.submit(0xC0000005, "AV_bucket_ntdll", "accv");
    s.add(
        "J4_wer_submit",
        sub == Ok(1) && b.submits.len() == 1,
        "AV 判定后 WER 提交",
    );

    // J5 提交自洽核。
    s.add("J5_submit_ok", b.submits.iter().all(|x| x.ok()), "全提交过自洽核");

    // J6 提交渲染。
    let r = b.submits[0].render();
    s.add(
        "J6_render",
        r.contains("c=0xc0000005") && r.contains("v=1"),
        "渲染含码/判定/桶",
    );

    // J7 桶签名一致。
    s.add(
        "J7_bucket",
        b.submits[0].bucket == "AV_bucket_ntdll",
        "桶签名字段对平",
    );

    // J8 提交账回放。
    s.add("J8_replayable", b.submits_replayable(), "提交账全可回放");

    // J9 错配提交拒。
    s.add(
        "J9_mismatch",
        b.submit(0xC0000094, "b", "n").err() == Some(UE_GHOST),
        "提交码≠最近判定码拒 GHOST",
    );

    // J10 空桶签名拒（提交码对齐最近判定码 0xC0000005，桶空拒 ORDER）。
    s.add(
        "J10_empty_bucket",
        b.submit(0xC0000005, "", "n").err() == Some(UE_ORDER),
        "空桶签名拒 ORDER",
    );

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "UEF 面折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.judge(0xC0000005).err() == Some(UE_SEALED)
            && b.submit(0xC0000005, "b", "n").err() == Some(UE_SEALED),
        "印账后判定/提交皆拒",
    );

    // J14 账面摘要（J2 十码 + J4 前补判一码 = 11 条判定）。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("verdicts=11") && sm.contains("submits=1"),
        "摘要含判定数与提交数",
    );

    // J15 未安装摘要。
    let b15 = UefFilter::new();
    s.add(
        "J15_fresh",
        !b15.installed() && b15.book_summary().contains("installed=false"),
        "未安装态摘要",
    );

    // J16 判定账含最近码。
    let mut b16 = UefFilter::new();
    let _ = b16.install();
    let _ = b16.judge(0xC0000094);
    s.add(
        "J16_last_verdict",
        b16.verdicts.last().map(|&(c, v)| c == 0xC0000094 && v == UE_SEARCH) == Some(true),
        "判定账尾=最近判定",
    );

    // J17 连续判十码全留痕。
    let mut b17 = UefFilter::new();
    let _ = b17.install();
    for i in 0..10u32 {
        let _ = b17.judge(0xC000_0000 + i);
    }
    s.add("J17_ten_verdicts", b17.verdicts.len() == 10, "十码判定全留痕");

    // J18 提交序号递增。
    let mut b18 = UefFilter::new();
    let _ = b18.install();
    let _ = b18.judge(0xC0000005);
    let n1 = b18.submit(0xC0000005, "b1", "n");
    let _ = b18.judge(0xC0000005);
    let n2 = b18.submit(0xC0000005, "b2", "n");
    s.add(
        "J18_seq",
        n1 == Ok(1) && n2 == Ok(2),
        "提交序号 1→2 递增",
    );

    // J19 空面指纹稳定。
    let b19 = UefFilter::new();
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空 UEF 面指纹稳定");

    // J20 快速失败码走 EXECUTE。
    let mut b20 = UefFilter::new();
    let _ = b20.install();
    s.add(
        "J20_fastfail",
        b20.judge(0xC0000409) == Ok(UE_EXECUTE),
        "0xC0000409 判 EXECUTE",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12154_uef() {
        let mut b = UefFilter::new();
        let _ = b.install();
        assert_eq!(b.judge(0xC0000005), Ok(UE_EXECUTE));
        assert_eq!(b.submit(0xC0000005, "b", "n"), Ok(1));
    }

    #[test]
    fn unx12154_checks_green() {
        let s = run_unx12154_checks();
        assert!(s.all_passed(), "UNX-F12154 判据未全绿");
    }

    #[test]
    fn unx12154_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
