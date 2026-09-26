//! F251 媒体会话仲裁 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：双应用并发仲裁用例（播放/暂停/恢复六态）；OSD 显示
//! 应用归属；浏览器会话接入验证；键响应 <50ms（F064 链）。
//!
//! **设计要点（主册）**：全局媒体键作用于「最近活跃的媒体会话」；仲裁
//! 规则写死——正在播放的优先于暂停的、同等状态取最近操作者；键按下时
//! OSD 显示当前控制的是哪个应用（图标+曲名）；浏览器与本地播放器同一
//! 仲裁体系。
//!
//! 实装：会话注册制（浏览器/本地同口注册），仲裁器纯函数化——
//! `arbitrate` 对会话表算出唯一获胜者；媒体键四键（播放/暂停/上一首/
//! 下一首）经仲裁器路由；每次键击回填 OSD 快照（应用+曲名）并记录
//! 响应耗时进环形账（<50ms 硬线判定）。时间全注入，六态用例宿主可复现。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 键响应判线（ms）——主册「键响应 <50ms」。
pub const KEY_RESPONSE_LIMIT_MS: u64 = 50;

/// 媒体会话状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayState {
    Playing,
    Paused,
}

/// 一个媒体会话（应用注册后获得稳定句柄 id）。
#[derive(Clone, Debug)]
pub struct MediaSession {
    pub id: u32,
    /// 应用名（OSD 归属显示，如「音乐」「Edge」）。
    pub app: String,
    /// 曲名（OSD 第二行）。
    pub title: String,
    pub state: PlayState,
    /// 最近一次用户操作（播放/暂停/切曲）时刻，ms 注入。
    pub last_op_ms: u64,
    /// 浏览器会话标记（判据「浏览器会话接入验证」——同一体系显式留痕）。
    pub from_browser: bool,
}

/// 仲裁结果：获胜会话 + 动作去向。
#[derive(Debug)]
pub struct Arbited<'a> {
    pub session: &'a MediaSession,
}

/// 仲裁器：正在播放的优先于暂停的；同等状态取最近操作者。
/// 全表为空返回 None（OSD 显「无可控制的媒体」——诚实降级）。
pub fn arbitrate<'a>(sessions: &'a [MediaSession]) -> Option<Arbited<'a>> {
    let mut best: Option<&MediaSession> = None;
    for s in sessions {
        let take = match best {
            None => true,
            Some(b) => match (s.state, b.state) {
                (PlayState::Playing, PlayState::Paused) => true,
                (PlayState::Paused, PlayState::Playing) => false,
                _ => s.last_op_ms > b.last_op_ms,
            },
        };
        if take {
            best = Some(s);
        }
    }
    best.map(|session| Arbited { session })
}

/// OSD 快照：键按下时显示的应用归属。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsdSnapshot {
    pub app: String,
    pub title: String,
    /// 本次键响应耗时（ms）。
    pub latency_ms: u64,
}

/// OSD 环形账容量（最近 32 次）。
pub const OSD_RING_CAP: usize = 32;

/// 全局媒体键。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKey {
    PlayPause,
    Prev,
    Next,
    Stop,
}

/// 仲裁服务：注册表 + OSD 账本。
pub struct MediaArbiter {
    sessions: Vec<MediaSession>,
    next_id: u32,
    /// 最近 32 次 OSD 快照（域内定容环形账——满则丢最旧，不无限增长）。
    osd_log: Vec<OsdSnapshot>,
    /// 键响应超 50ms 的次数（判据红线计数，宿主自检断言为 0）。
    pub over_limit: u32,
}

impl MediaArbiter {
    pub fn new() -> MediaArbiter {
        MediaArbiter { sessions: Vec::new(), next_id: 1, osd_log: Vec::new(), over_limit: 0 }
    }

    /// 注册会话（浏览器/本地同一口——`from_browser` 只留痕不区分权级）。
    pub fn register(&mut self, app: &str, title: &str, from_browser: bool, now_ms: u64) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.sessions.push(MediaSession {
            id,
            app: String::from(app),
            title: String::from(title),
            state: PlayState::Paused,
            last_op_ms: now_ms,
            from_browser,
        });
        id
    }

    pub fn session(&self, id: u32) -> Option<&MediaSession> {
        self.sessions.iter().find(|s| s.id == id)
    }

    /// 会话退出（应用关闭/页面关掉）——注销，不留幽灵会话。
    pub fn unregister(&mut self, id: u32) -> bool {
        let before = self.sessions.len();
        self.sessions.retain(|s| s.id != id);
        self.sessions.len() != before
    }

    /// 应用侧状态更新（自动播放等非用户操作也走这里刷 last_op）。
    pub fn set_state(&mut self, id: u32, state: PlayState, title: &str, now_ms: u64) -> bool {
        match self.sessions.iter_mut().find(|s| s.id == id) {
            Some(s) => {
                s.state = state;
                s.last_op_ms = now_ms;
                if !title.is_empty() {
                    s.title = String::from(title);
                }
                true
            }
            None => false,
        }
    }

    /// 媒体键路由：仲裁获胜者执行动作，回 OSD 快照。
    /// `latency_ms` 由调用方注入（F064 低延迟链打点）。
    pub fn press_key(
        &mut self,
        key: MediaKey,
        latency_ms: u64,
        now_ms: u64,
    ) -> Option<OsdSnapshot> {
        if latency_ms > KEY_RESPONSE_LIMIT_MS {
            self.over_limit += 1;
        }
        let (id, cur_state, app, title) = {
            let winner = &arbitrate(&self.sessions)?.session;
            (winner.id, winner.state, winner.app.clone(), winner.title.clone())
        };
        let snap = match key {
            MediaKey::PlayPause => {
                let flipped = match cur_state {
                    PlayState::Playing => PlayState::Paused,
                    PlayState::Paused => PlayState::Playing,
                };
                self.set_state(id, flipped, "", now_ms);
                OsdSnapshot { app, title, latency_ms }
            }
            MediaKey::Prev | MediaKey::Next => {
                // 切曲视为一次活跃操作（仲裁权重刷新）。
                self.set_state(id, cur_state, "", now_ms);
                OsdSnapshot { app, title, latency_ms }
            }
            MediaKey::Stop => {
                self.set_state(id, PlayState::Paused, "", now_ms);
                OsdSnapshot { app, title, latency_ms }
            }
        };
        self.osd_log.push(snap.clone());
        if self.osd_log.len() > OSD_RING_CAP {
            let drop_n = self.osd_log.len() - OSD_RING_CAP;
            self.osd_log.drain(..drop_n);
        }
        Some(snap)
    }

    /// 最近 OSD 快照（新→旧）。
    pub fn recent_osd(&self) -> Vec<OsdSnapshot> {
        self.osd_log.iter().rev().cloned().collect()
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_mediarbit_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F251");
    let mut arb = MediaArbiter::new();
    let a = arb.register("音乐", "曲一", false, 1_000);
    let b = arb.register("Edge", "网页歌", true, 2_000);
    // 六态用例：A 播 / B 暂 → A 胜；B 播（更晚）→ B 胜；A 恢复（更晚）→ A 胜；
    // 双暂 → 最近操作者；A 注销 → B；全空 → None。
    arb.set_state(a, PlayState::Playing, "", 1_100);
    let w = arbitrate(&arb.sessions_ref());
    set.add("F251 playing beats paused", w.unwrap().session.id == a, "A playing wins");
    arb.set_state(b, PlayState::Playing, "", 1_200);
    let w2 = arbitrate(&arb.sessions_ref());
    set.add("F251 recent op wins", w2.unwrap().session.id == b, "B later wins");
    arb.set_state(a, PlayState::Playing, "", 1_300);
    let w3 = arbitrate(&arb.sessions_ref());
    set.add("F251 resume overtakes", w3.unwrap().session.id == a, "A back");
    arb.set_state(a, PlayState::Paused, "", 1_400);
    arb.set_state(b, PlayState::Paused, "", 1_500);
    let w4 = arbitrate(&arb.sessions_ref());
    set.add("F251 tie takes recent", w4.unwrap().session.id == b, "tie recent");
    arb.unregister(a);
    let w5 = arbitrate(&arb.sessions_ref());
    set.add("F251 ghost-free", w5.unwrap().session.id == b, "after exit");
    arb.unregister(b);
    set.add("F251 empty honest", arbitrate(&arb.sessions_ref()).is_none(), "none");
    // 浏览器会话接入：同一注册口，from_browser 留痕。
    set.add("F251 browser same lane", arb.register("Edge", "x", true, 9_000) > 0, "one lane");
    // OSD 归属与键响应 <50ms。
    let mut arb2 = MediaArbiter::new();
    let m = arb2.register("音乐", "曲一", false, 0);
    arb2.set_state(m, PlayState::Playing, "", 10);
    let snap = arb2.press_key(MediaKey::PlayPause, 12, 20).unwrap();
    set.add(
        "F251 osd app+latency",
        snap.app == "音乐" && snap.latency_ms == 12,
        "osd owned",
    );
    let _ = arb2.press_key(MediaKey::Next, 49, 30);
    set.add("F251 latency under 50", arb2.over_limit == 0, "50ms hard line");
    let _ = arb2.press_key(MediaKey::PlayPause, 51, 40);
    set.add("F251 over limit counted", arb2.over_limit == 1, "honest count");
    set
}

impl MediaArbiter {
    /// 自检/测试取只读会话表。
    fn sessions_ref(&self) -> &[MediaSession] {
        &self.sessions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f251_six_states_all_green() {
        let set = run_mediarbit_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F251 自检红 {f}/{p}");
    }

    #[test]
    fn arbitration_is_total_order() {
        // 同刻操作取后注册者（> 严格比较 → 先注册者保位，规则确定无歧义）。
        let mut arb = MediaArbiter::new();
        let x = arb.register("X", "a", false, 100);
        let _y = arb.register("Y", "b", false, 100);
        let w = arbitrate(arb.sessions_ref()).unwrap().session.id;
        assert_eq!(w, x);
    }
}
