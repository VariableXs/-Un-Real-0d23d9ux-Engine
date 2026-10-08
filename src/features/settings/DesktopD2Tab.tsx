/**
 * C 桌面体验域·后段 · AI-D2 — 设置中心「桌面体验」标签页（F093-F110 全量面板）。
 *
 * 面板纪律（与 MouseJ1Tab 同源）：
 * - 名称 + 一句话说明 + 调节控件三件套齐全（F474 同源）；
 * - 默认档 = 主册判据默认（悬停 800ms、2GB 缓存、贪睡 5 分钟、30% 可读下限……）；
 * - 全部改动即时生效（d2Store 订阅制广播）+ 可回退（undoSection 栈深 3）；
 * - 高密度纵深排布：六个分组区，十六项各就各位；F101/F104 冻结候删不在面板内
 *   （结构性守护，与 d2store 分节清单互斥）。
 */

import { useEffect, useState } from "react";
import { d2Store, D2_DEFAULTS, assertFrozenGuard, type D2Section } from "../desktopxp/d2store";
import { FONT_SIZE_STEPS } from "../desktopxp/termcore";
import { openVwmApp } from "../../system/windows/vwm";
import { pushToast } from "../../state/uiStore";
import { askConfirm } from "../../components/Modal";
import { SectionCard, MediaTryPanel, PhraseLibPanel, PalettePanel, ThumbCachePanel, XLogPanel } from "./DesktopD2Panels";
import "../../styles/desktop-d2.css";

type Cfg = Record<string, unknown>;

function useSection<T extends Cfg>(section: D2Section): [T, (patch: Partial<T>) => void] {
  const [, tick] = useState(0);
  useEffect(() => d2Store.subscribe(() => tick((v) => v + 1)), []);
  const value = { ...(D2_DEFAULTS[section] as T), ...(d2Store.get(section) as unknown as T) };
  const set = (patch: Partial<T>): void => {
    try {
      d2Store.set(section, patch as Cfg);
    } catch (e) {
      pushToast("error", "配置落盘失败", String(e));
    }
  };
  return [value, set];
}

/** 分组容器（纵深层级：组标题 + F 号锚 + 说明 + 内容）。 */
function Group(props: { title: string; f: string; desc: string; children: React.ReactNode }): React.ReactElement {
  return (
    <section className="d2-group">
      <header className="d2-group-head">
        <h4>{props.title}</h4>
        <span className="d2-fnum">{props.f}</span>
        <p>{props.desc}</p>
      </header>
      <div className="d2-group-body">{props.children}</div>
    </section>
  );
}

/** 三件套行：名称 / 一句话说明 / 调节控件。 */
function Row(props: { label: string; hint: string; children: React.ReactNode }): React.ReactElement {
  return (
    <div className="d2-row">
      <div className="d2-row-text">
        <span className="d2-row-label">{props.label}</span>
        <span className="d2-row-hint">{props.hint}</span>
      </div>
      <div className="d2-row-ctl">{props.children}</div>
    </div>
  );
}

function Toggle(props: { on: boolean; onChange: (v: boolean) => void; label: string }): React.ReactElement {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={props.on}
      aria-label={props.label}
      className={props.on ? "d2-switch d2-switch--on" : "d2-switch"}
      onClick={() => props.onChange(!props.on)}
    >
      <span className="d2-switch-knob" />
    </button>
  );
}

export function DesktopD2Tab(): React.ReactElement {
  const [thumb, setThumb] = useSection<{ enabled: boolean; cacheCapMb: number; progressive: boolean }>("thumbeng");
  const [media, setMedia] = useSection<{ enabled: boolean; hoverDelayMs: number }>("mediainfo");
  const [term, setTerm] = useSection<{ fontSizeStep: number; cursorStyle: string; cursorBlink: boolean }>("term2");
  const [palette, setPalette] = useSection<{ enabled: boolean }>("termpalette");
  const [notepad, setNotepad] = useSection<{ autoSave: boolean; bigFileReadOnlyGate: boolean }>("notepad");
  const [snap, setSnap] = useSection<{ defaultMode: string; includeCursor: boolean }>("snipshot");
  const [calc, setCalc] = useSection<{ historyCap: number; angle: string }>("calcx");
  const [clock, setClock] = useSection<{ countdownFullChain: boolean; alarmSnoozeMin: number }>("clocksuite");
  const [note, setNote] = useSection<{ cap: number; fontSize: number; colorIndex: number }>("sticknote");
  const [sketch, setSketch] = useSection<{ brush: string; autoDraftSec: number }>("sketchpad");
  const [photo, setPhoto] = useSection<{ slideIntervalSec: number; crossfadeMs: number; zoomStep: number }>("photolib");
  const [hud, setHud] = useSection<{ enabled: boolean; cornerMode: boolean; position: string }>("keyhud");
  const [ime, setIme] = useSection<{ mode: string; hideOnPassword: boolean; compact: boolean }>("imefloat");
  const [phrase, setPhrase] = useSection<{ phrasePriority: boolean }>("phrasebk");
  const [clip, setClip] = useSection<{ confirmPasteBack: boolean }>("cliphist");
  const [osk, setOsk] = useSection<{ full: boolean; opacity: number; learning: boolean; clickThrough: boolean; alwaysOnTop: boolean; visible: boolean }>("osk");

  const undo = (s: D2Section, name: string): void => {
    if (d2Store.undoSection(s)) pushToast("success", `已还原「${name}」上一态`);
    else pushToast("info", "没有可回退的历史");
  };

  const resetAll = (): void => {
    void (async () => {
      const ok = await askConfirm({
        title: "恢复桌面体验域默认",
        body: "将把 C 域后段（F093-F110）全部 16 项恢复为系统默认档。此操作可撤销（每节保留最近 3 次快照）。继续吗？",
      });
      if (!ok) return;
      for (const s of Object.keys(D2_DEFAULTS) as D2Section[]) d2Store.set(s, { ...(D2_DEFAULTS[s] as Cfg) });
      pushToast("success", "桌面体验域已恢复默认");
    })();
  };

  const frozenOk = assertFrozenGuard();

  return (
    <div className="d2-tab">
      <p className="d2-desc">
        桌面体验·后段（C 域二分队 · F093-F110）：缩略图与媒体、终端与命令、编辑截图、计算时间、便签画图相册、浮窗与键盘。全部改动即时生效，每节可一键还原上一态；天气件/录音件（F101/F104）在主册冻结候删名单，不在本页。
      </p>

      <div className="d2-toolbar">
        <button type="button" onClick={resetAll}>恢复域默认</button>
        <span className="d2-toolbar-note">
          冻结守护 {frozenOk ? <span className="d2-ok">✓</span> : <span className="d2-bad">✗ 结构缺陷</span>} · 分节 {Object.keys(D2_DEFAULTS).length}/16
        </span>
      </div>

      <Group title="图片缩略图与媒体信息" f="F093 / F094" desc="缩略图三级队列（可视>邻近>背景）+ 2GB LRU 缓存；媒体文件悬停 800ms 出信息卡，缓存命中即时（<100ms 判线）。">
        <Row label="缩略图引擎" hint="关闭后列表直读原图（慢且耗流量——不建议）。">
          <Toggle on={thumb.enabled} onChange={(v) => setThumb({ enabled: v })} label="缩略图引擎" />
        </Row>
        <Row label="缓存上限" hint="判据线 2GB；超出从最冷逐出（LRU）。">
          <input
            type="range" min={256} max={8192} step={256} value={thumb.cacheCapMb}
            onChange={(e) => setThumb({ cacheCapMb: Number(e.target.value) })}
            aria-label="缩略图缓存上限"
          />
          <span className="d2-num">{(thumb.cacheCapMb / 1024).toFixed(2)} GB</span>
        </Row>
        <Row label="渐进占位" hint="背景请求低清先行、精修跟随——长列表先有影再变清。">
          <Toggle on={thumb.progressive} onChange={(v) => setThumb({ progressive: v })} label="渐进占位" />
        </Row>
        <Row label="媒体信息悬浮" hint={`悬停 ${media.hoverDelayMs}ms 出卡；缓存命中即时显示（判线 <100ms）。`}>
          <Toggle on={media.enabled} onChange={(v) => setMedia({ enabled: v })} label="媒体信息悬浮" />
          <button type="button" onClick={() => undo("mediainfo", "媒体悬浮")}>还原</button>
        </Row>
        <SectionCard title="媒体信息试析台（五容器嗅探）" f="F094">
          <MediaTryPanel />
        </SectionCard>
        <SectionCard title="缩略图缓存对账（队列/命中率）" f="F093">
          <ThumbCachePanel />
        </SectionCard>
      </Group>

      <Group title="终端与命令面板" f="F095 / F096" desc="终端 2.0：CJK 混排不错位、10 万行回看、分屏拖拽重排；命令面板：内置 12 条全可达、收藏 frecency、Ctrl+Shift+P 呼出。">
        <Row label="字号档" hint={`Ctrl+滚轮 8 档（当前 ${FONT_SIZE_STEPS[term.fontSizeStep]}px）。`}>
          <input
            type="range" min={0} max={7} value={term.fontSizeStep}
            onChange={(e) => setTerm({ fontSizeStep: Number(e.target.value) })}
            aria-label="终端字号档"
          />
          <span className="d2-num">{FONT_SIZE_STEPS[term.fontSizeStep]}px</span>
        </Row>
        <Row label="光标样式" hint="块 / 下划线 / 竖线——闪烁跟随下一行开关。">
          <select value={term.cursorStyle} onChange={(e) => setTerm({ cursorStyle: e.target.value })} aria-label="光标样式">
            <option value="block">块</option>
            <option value="underline">下划线</option>
            <option value="bar">竖线</option>
          </select>
          <Toggle on={term.cursorBlink} onChange={(v) => setTerm({ cursorBlink: v })} label="光标闪烁" />
        </Row>
        <Row label="命令面板" hint="收藏-搜索-执行全链；危险字符原样传递不代管。">
          <Toggle on={palette.enabled} onChange={(v) => setPalette({ enabled: v })} label="命令面板" />
          <button type="button" onClick={() => { openVwmApp("term2"); pushToast("info", "终端 2.0 已打开", "Ctrl+Shift+P 呼出命令面板"); }}>打开终端</button>
        </Row>
        <SectionCard title="命令面板试搜（双路模糊）" f="F096">
          <PalettePanel />
        </SectionCard>
      </Group>

      <Group title="编辑与截图" f="F097 / F098" desc="记事本：改动即冲刷（草稿防抖 500ms）+ 大文件只读门；截图：四模式 + 标注 + 取色对拍 + 马赛克。">
        <Row label="自动保存" hint="改动即冲刷；断电后草稿恢复（B-1801 原子写同源）。">
          <Toggle on={notepad.autoSave} onChange={(v) => setNotepad({ autoSave: v })} label="自动保存" />
        </Row>
        <Row label="大文件只读门" hint="超阈值文件先只读打开、显式启用编辑——防误改大日志。">
          <Toggle on={notepad.bigFileReadOnlyGate} onChange={(v) => setNotepad({ bigFileReadOnlyGate: v })} label="大文件只读门" />
          <button type="button" onClick={() => { openVwmApp("notepad"); }}>打开记事本</button>
          <button type="button" onClick={() => undo("notepad", "记事本")}>还原</button>
        </Row>
        <Row label="截图默认模式" hint="PrintScreen 与截图窗口共用此默认。">
          <select value={snap.defaultMode} onChange={(e) => setSnap({ defaultMode: e.target.value })} aria-label="截图默认模式">
            <option value="region">区域</option>
            <option value="full">全屏</option>
            <option value="window">窗口</option>
            <option value="delay">延时 3s</option>
          </select>
        </Row>
        <Row label="截图含光标" hint="默认不含（干净优先）；录屏有独立开关（F592 对端）。">
          <Toggle on={snap.includeCursor} onChange={(v) => setSnap({ includeCursor: v })} label="截图含光标" />
          <button type="button" onClick={() => { openVwmApp("snapshot"); }}>打开截图</button>
        </Row>
      </Group>

      <Group title="计算与时钟" f="F099 / F100" desc="计算器：128 位定点引擎（0.1+0.2=0.3 精确）+ 历史回填 20 轮零错位；时钟：倒计时到点全链（音效+通知+全屏）+ 贪睡 5 分钟。">
        <Row label="历史轮数" hint="回填点击即回——20 轮零错位（判据线）。">
          <select value={String(calc.historyCap)} onChange={(e) => setCalc({ historyCap: Number(e.target.value) })} aria-label="历史轮数">
            {[10, 20, 50].map((n) => (
              <option key={n} value={String(n)}>{n} 轮</option>
            ))}
          </select>
        </Row>
        <Row label="角度制" hint="科学模式三角函数的角单位（deg/rad）。">
          <select value={calc.angle} onChange={(e) => setCalc({ angle: e.target.value })} aria-label="角度制">
            <option value="deg">角度（°）</option>
            <option value="rad">弧度（rad）</option>
          </select>
          <button type="button" onClick={() => { openVwmApp("calc"); }}>打开计算器</button>
        </Row>
        <Row label="倒计时到点全链" hint="音效 + 通知 + 全屏提示三件齐发——静默到点视为缺陷。">
          <Toggle on={clock.countdownFullChain} onChange={(v) => setClock({ countdownFullChain: v })} label="倒计时到点全链" />
        </Row>
        <Row label="闹钟贪睡" hint="分钟数；跨时区进位对拍 F100 判据。">
          <select value={String(clock.alarmSnoozeMin)} onChange={(e) => setClock({ alarmSnoozeMin: Number(e.target.value) })} aria-label="贪睡分钟">
            {[1, 5, 10].map((n) => (
              <option key={n} value={String(n)}>{n} 分钟</option>
            ))}
          </select>
          <button type="button" onClick={() => { openVwmApp("clockhub"); }}>打开时钟中心</button>
        </Row>
      </Group>

      <Group title="便签 · 画图 · 相册" f="F102 / F103 / F105" desc="便签 20 张上限诚实拒绝、置顶跨窗口恒前；画图 50 步撤销 + 45° 吸附 + 速度笔压；相册月分组 + 放映 5s/交叉 250ms。">
        <Row label="便签上限" hint="超出诚实拒绝（不静默挤掉最旧）——20 张为判据档。">
          <select value={String(note.cap)} onChange={(e) => setNote({ cap: Number(e.target.value) })} aria-label="便签上限">
            {[10, 20, 50].map((n) => (
              <option key={n} value={String(n)}>{n} 张</option>
            ))}
          </select>
          <button type="button" onClick={() => { openVwmApp("notes"); }}>打开便签</button>
        </Row>
        <Row label="便签字号" hint="13/14/16px 三档，改动即存。">
          <select value={String(note.fontSize)} onChange={(e) => setNote({ fontSize: Number(e.target.value) })} aria-label="便签字号">
            {[13, 14, 16].map((n) => (
              <option key={n} value={String(n)}>{n}px</option>
            ))}
          </select>
        </Row>
        <Row label="画图默认笔刷" hint="钢笔（速度笔压）/ 马克笔（恒宽）/ 铅笔（轻纹理）。">
          <select value={sketch.brush} onChange={(e) => setSketch({ brush: e.target.value })} aria-label="画图默认笔刷">
            <option value="pen">钢笔</option>
            <option value="marker">马克笔</option>
            <option value="pencil">铅笔</option>
          </select>
          <button type="button" onClick={() => { openVwmApp("paint"); }}>打开画图</button>
        </Row>
        <Row label="自动草稿" hint="30s 一存；断电恢复草稿（F103 判据）。">
          <select value={String(sketch.autoDraftSec)} onChange={(e) => setSketch({ autoDraftSec: Number(e.target.value) })} aria-label="自动草稿间隔">
            {[0, 15, 30, 60].map((n) => (
              <option key={n} value={String(n)}>{n === 0 ? "关" : `${n}s`}</option>
            ))}
          </select>
          <button type="button" onClick={() => undo("sketchpad", "画图")}>还原</button>
        </Row>
        <Row label="放映节奏" hint="自动翻页秒数与交叉淡入时长。">
          <select value={String(photo.slideIntervalSec)} onChange={(e) => setPhoto({ slideIntervalSec: Number(e.target.value) })} aria-label="放映间隔">
            {[3, 5, 8].map((n) => (
              <option key={n} value={String(n)}>{n}s</option>
            ))}
          </select>
          <select value={String(photo.crossfadeMs)} onChange={(e) => setPhoto({ crossfadeMs: Number(e.target.value) })} aria-label="交叉淡入">
            {[0, 250, 400].map((n) => (
              <option key={n} value={String(n)}>{n === 0 ? "直切" : `${n}ms`}</option>
            ))}
          </select>
          <button type="button" onClick={() => { openVwmApp("album"); }}>打开相册</button>
        </Row>
      </Group>

      <Group title="浮窗与键盘" f="F106 / F107 / F108 / F109 / F110" desc="锁定键 HUD（1s 淡出判线）、输入法三态浮窗（16ms 节流跟随）、短语库（<100ms 全链）、剪贴板回贴确认、屏幕键盘（学习模式 + 半透明可读）。">
        <Row label="键盘提示 HUD" hint="Caps/Num/Scroll 切换即出、1s 后 300ms 淡出；全屏自动转右下角标。">
          <Toggle on={hud.enabled} onChange={(v) => setHud({ enabled: v })} label="键盘提示 HUD" />
        </Row>
        <Row label="HUD 角标模式" hint="放映/游戏前景区强制角标（也可手动常驻角标）。">
          <Toggle on={hud.cornerMode} onChange={(v) => setHud({ cornerMode: v })} label="HUD 角标模式" />
          <button type="button" onClick={() => undo("keyhud", "键盘 HUD")}>还原</button>
        </Row>
        <Row label="HUD 位置" hint="底部中（任务栏上方）/ 顶部中——避开你常用的区域。">
          <select value={hud.position} onChange={(e) => setHud({ position: e.target.value })} aria-label="HUD 位置">
            <option value="bottom">底部中</option>
            <option value="top">顶部中</option>
          </select>
        </Row>
        <Row label="输入法浮窗" hint="跟随光标 / 收起为小标 / 隐藏；点击态位即切换（浮窗即开关）。">
          <select value={ime.mode} onChange={(e) => setIme({ mode: e.target.value })} aria-label="输入法浮窗模式">
            <option value="follow">跟随光标</option>
            <option value="taskbarChip">任务栏小标</option>
            <option value="hidden">隐藏</option>
          </select>
        </Row>
        <Row label="仅显中英态" hint="紧凑档只出语言位（全半角/标点收进完整浮窗）。">
          <Toggle on={ime.compact} onChange={(v) => setIme({ compact: v })} label="仅显中英态" />
          <button type="button" onClick={() => undo("imefloat", "输入法浮窗")}>还原</button>
        </Row>
        <Row label="密码框自动隐藏" hint="焦点进密码框浮窗即隐（隐私纪律，默认开）。">
          <Toggle on={ime.hideOnPassword} onChange={(v) => setIme({ hideOnPassword: v })} label="密码框自动隐藏" />
          <button type="button" onClick={() => undo("imefloat", "输入法浮窗")}>还原</button>
        </Row>
        <Row label="短语优先" hint="与词库同词重时短语排前（可关）。">
          <Toggle on={phrase.phrasePriority} onChange={(v) => setPhrase({ phrasePriority: v })} label="短语优先" />
        </Row>
        <SectionCard title="短语库管理台（导入导出 + 候选试炼）" f="F108">
          <PhraseLibPanel />
        </SectionCard>
        <Row label="剪贴板回贴确认" hint="回贴敏感条目前二次确认（录制/落盘策略在 AI-07 后端——一处一事实）。">
          <Toggle on={clip.confirmPasteBack} onChange={(v) => setClip({ confirmPasteBack: v })} label="回贴确认" />
          <button type="button" onClick={() => { openVwmApp("clipboard"); }}>打开剪贴板历史</button>
        </Row>
        <Row label="屏幕键盘" hint="Ctrl+Shift+O 呼出（F169 注册表已登记）；透明度 30% 为可读下限。">
          <Toggle
            on={osk.visible}
            onChange={(v) => setOsk({ visible: v })}
            label="屏幕键盘"
          />
          <button type="button" onClick={() => { setOsk({ visible: true }); pushToast("info", "屏幕键盘已呼出", "拖标题栏移动，松手自动屏边吸附"); }}>立即呼出</button>
        </Row>
        <Row label="键盘透明度" hint={`当前 ${osk.opacity}%（下限 30%——低于即拒绝，可读红线）。`}>
          <input
            type="range" min={30} max={100} value={osk.opacity}
            onChange={(e) => setOsk({ opacity: Number(e.target.value) })}
            aria-label="屏幕键盘透明度"
          />
          <span className="d2-num">{osk.opacity}%</span>
        </Row>
        <Row label="学习模式" hint="物理键按下时对应虚拟键同步高亮（触屏应急双用途）。">
          <Toggle on={osk.learning} onChange={(v) => setOsk({ learning: v })} label="学习模式" />
        </Row>
        <Row label="半透明观察模式" hint="开启后键盘穿透点击（只看不按——下层内容操作不受阻）。">
          <Toggle on={osk.clickThrough} onChange={(v) => setOsk({ clickThrough: v })} label="点击穿透" />
          <button type="button" onClick={() => undo("osk", "屏幕键盘")}>还原</button>
        </Row>
      </Group>

      <Group title="体验日志与证据" f="十三 / MD3 附B" desc="体验日志还原每一次操作（挫败信号自动标记，隐私红线：不记内容只记行为）；域配置整包导出 + 冻结候删结构守护自检。">
        <SectionCard title="体验日志（挫败信号）" f="十三">
          <XLogPanel />
        </SectionCard>
        <div className="d2-rowflex">
          <button
            type="button"
            onClick={() => {
              const pack = { format: "desktop-d2-config", version: 1, frozenGuard: assertFrozenGuard(), data: d2Store.exportAll() };
              void navigator.clipboard?.writeText(JSON.stringify(pack, null, 2)).then(
                () => pushToast("success", "证据包已复制", "JSON 开放格式——配置整包 + 冻结守护位"),
                (e) => pushToast("error", "复制失败", String(e)),
              );
            }}
          >
            复制整包证据（JSON）
          </button>
          <span className="d2-muted">冻结候删 F101/F104 与 16 实现项互斥：{frozenOk ? <span className="d2-ok">✓ 在册守护</span> : <span className="d2-bad">✗</span>}</span>
        </div>
      </Group>
    </div>
  );
}
