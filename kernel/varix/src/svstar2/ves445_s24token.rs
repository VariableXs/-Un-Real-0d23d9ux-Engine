//! ves445_s24token — VE-F4211 · 一致性与视觉令牌
//! 视觉令牌系统入约落地：色彩/间距/圆角/阴影/动效曲线五族令牌→规则转译；
//! 令牌漂移检测（实际渲染值 vs 令牌声明值像素级比对）；主题×语种双维矩阵令牌一致性；
//! 漂移超阈→定位+修复单；主题缺令牌→补位+登记；RTL 镜像令牌失配→T02 联动修复；
//! 令牌含无障碍色标族（对比度阈值内建为令牌约束——域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1480;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TkCode {
    Ok = 0,
    DriftOver = SEG_BASE + 0x01,   // 0x1481
    ThemeGap = SEG_BASE + 0x02,    // 0x1482
    MirrorMismatch = SEG_BASE + 0x03, // 0x1483
}

/// 五族令牌 + 无障碍色标族（第六族——域本色）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenFamily { Color = 0, Spacing = 1, Radius = 2, Shadow = 3, Motion = 4, A11yColor = 5 }

impl TokenFamily {
    pub const ALL: [TokenFamily; 6] = [
        TokenFamily::Color, TokenFamily::Spacing, TokenFamily::Radius,
        TokenFamily::Shadow, TokenFamily::Motion, TokenFamily::A11yColor];
}

/// 令牌声明（声明值 + 阈值 bp）。
pub struct TokenDecl { pub family: TokenFamily, pub name: &'static str, pub declared: u32, pub drift_limit_bp: u32 }

/// 令牌册（双维矩阵：主题 × 语种/方向）。
pub struct TokenBook {
    pub decls: Vec<TokenDecl>,
    pub rendered: Vec<(&'static str, u32)>,      // 实际渲染值（像素级）
    pub themes: Vec<(&'static str, Vec<&'static str>)>, // 主题 → 令牌名集合
    pub mirrors: Vec<(&'static str, u32, u32)>,  // RTL 镜像（令牌, LTR 值, RTL 值）
    pub registered: Vec<&'static str>,           // 补位登记
}

impl TokenBook {
    pub fn new() -> Self {
        TokenBook { decls: Vec::new(), rendered: Vec::new(), themes: Vec::new(), mirrors: Vec::new(), registered: Vec::new() }
    }
    pub fn declare(&mut self, f: TokenFamily, name: &'static str, v: u32, lim: u32) {
        self.decls.retain(|d| d.name != name);
        self.decls.push(TokenDecl { family: f, name, declared: v, drift_limit_bp: lim });
    }
    pub fn observe(&mut self, name: &'static str, v: u32) {
        self.rendered.retain(|(n, _)| *n != name);
        self.rendered.push((name, v));
    }
    /// 漂移检测：|渲染-声明|/声明 > 限 → 超阈定位。
    pub fn drift_check(&self) -> Vec<&'static str> {
        let mut bad = Vec::new();
        for d in &self.decls {
            if let Some((_, rv)) = self.rendered.iter().find(|(n, _)| *n == d.name) {
                let diff = ((*rv) as i64 - d.declared as i64).unsigned_abs();
                if d.declared > 0 && diff * 10_000 / d.declared as u64 > d.drift_limit_bp as u64 {
                    bad.push(d.name);
                }
            }
        }
        bad
    }
    /// 双维矩阵：主题必须覆盖五族+a11y 色标族。
    pub fn theme_gaps(&self, theme: &str) -> Vec<TokenFamily> {
        let names = match self.themes.iter().find(|(t, _)| *t == theme) {
            Some((_, ns)) => ns, None => return TokenFamily::ALL.to_vec(),
        };
        TokenFamily::ALL.iter().filter(|f| !names.iter().any(|n|
            self.decls.iter().any(|d| d.name == *n && d.family == **f))).map(|f| *f).collect()
    }
    pub fn set_theme(&mut self, theme: &'static str, tokens: Vec<&'static str>) {
        self.themes.retain(|(t, _)| *t != theme);
        self.themes.push((theme, tokens));
    }
    pub fn register_gap(&mut self, name: &'static str) { if !self.registered.contains(&name) { self.registered.push(name); } }
    /// RTL 镜像一致性：镜像值应为声明镜像（此处以偶校验代：RTL != LTR 且 > 0）。
    pub fn mirror_check(&self) -> Vec<&'static str> {
        self.mirrors.iter().filter(|(_, l, r)| r == l || *r == 0).map(|(n, _, _)| *n).collect()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves445_checks() -> CheckSet {
    let mut set = CheckSet::new("ves445");
    let mut bk = TokenBook::new();

    // C445-01 六族声明（含 a11y 色标族——域本色）。
    bk.declare(TokenFamily::Color, "bg-primary", 1000, 200);
    bk.declare(TokenFamily::Spacing, "gap-md", 160, 100);
    bk.declare(TokenFamily::Radius, "corner-md", 80, 200);
    bk.declare(TokenFamily::Shadow, "elev-1", 40, 300);
    bk.declare(TokenFamily::Motion, "ease-std", 250, 150);
    bk.declare(TokenFamily::A11yColor, "contrast-min", 450, 0); // 对比度 4.5:1 内建约束
    chk(&mut set, "C445-01", bk.decls.len() == 6, "六族声明");

    // C445-02 像素级比对：渲染值带内通过。
    bk.observe("bg-primary", 1010);
    bk.observe("gap-md", 158);
    bk.observe("corner-md", 80);
    bk.observe("elev-1", 41);
    bk.observe("ease-std", 248);
    bk.observe("contrast-min", 450);
    chk(&mut set, "C445-02", bk.drift_check().is_empty(), "带内零漂移");

    // C445-03 漂移超阈→定位。
    bk.observe("gap-md", 200); // 偏差 25% > 1%
    let bad = bk.drift_check();
    chk(&mut set, "C445-03", bad == vec!["gap-md"], "漂移定位");

    // C445-04 修复单回填复绿。
    bk.observe("gap-md", 160);
    chk(&mut set, "C445-04", bk.drift_check().is_empty(), "修复复绿");

    // C445-05 双维矩阵：主题缺 Motion 族→补位定位。
    bk.set_theme("dark", vec!["bg-primary", "gap-md", "corner-md", "elev-1", "contrast-min"]);
    let gaps = bk.theme_gaps("dark");
    chk(&mut set, "C445-05", gaps == vec![TokenFamily::Motion], "缺族定位");

    // C445-06 补位+登记。
    bk.set_theme("dark", vec!["bg-primary", "gap-md", "corner-md", "elev-1", "ease-std", "contrast-min"]);
    bk.register_gap("ease-std@dark");
    chk(&mut set, "C445-06", bk.theme_gaps("dark").is_empty() && bk.registered.len() == 1, "补位登记");

    // C445-07 RTL 镜像失配检出（镜像值等于原值）。
    bk.mirrors.push(("gap-md", 160, 160));
    chk(&mut set, "C445-07", bk.mirror_check() == vec!["gap-md"], "镜像失配");

    // C445-08 镜像修复（RTL 值不同且非零）→ T02 联动。
    bk.mirrors[0].2 = 200;
    chk(&mut set, "C445-08", bk.mirror_check().is_empty(), "镜像修复");

    // C445-09 a11y 色标族零容差断言（内建约束 drift_limit=0）。
    let a11y = bk.decls.iter().find(|d| d.family == TokenFamily::A11yColor).unwrap();
    chk(&mut set, "C445-09", a11y.drift_limit_bp == 0 && a11y.declared == 450, "色标零容差");

    // C445-10 六族单源断言。
    chk(&mut set, "C445-10", TokenFamily::ALL.len() == 6
        && TokenFamily::ALL[5] == TokenFamily::A11yColor, "六族单源");

    // C445-11 声明覆盖幂等。
    bk.declare(TokenFamily::Color, "bg-primary", 1000, 200);
    chk(&mut set, "C445-11", bk.decls.len() == 6, "声明幂等");

    // C445-12 观测覆盖幂等。
    bk.observe("bg-primary", 1000);
    chk(&mut set, "C445-12", bk.rendered.len() == 6, "观测幂等");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves445_smoke() {
        let set = run_ves445_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves445 red: {:?}", red);
    }
}
