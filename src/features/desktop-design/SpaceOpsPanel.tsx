/**
 * UNREAL-X-15000 · AI-06 空间管理面板（族0051~0060 · X01251~X01500 · 领域02 · Variable 桌面线）。
 * 落点：src/features/desktop-design/（新建）+ src/features/vision/（挂载）。
 * 红线：默认折叠、纯演示态驱动（虚拟桌面/整理/小地图/降级链），不改窗口真实落位；
 *       样式全部走 --w2/--accent 令牌（space-ops.css），HC 主题自动降级为描边。
 */
import { useMemo, useState } from "react";
import { LayoutGrid, Sparkles, Gauge } from "lucide-react";
import {
  VirtualDesktops, OrganizeAssistant, Minimap, DegradeChain,
  type WinInfo, type Rect,
} from "./spaceOps";
import "./space-ops.css";

const DEMO_WINS: WinInfo[] = [
  { id: "a", app: "编辑器", rect: { x: 0, y: 0, w: 800, h: 600 } },
  { id: "b", app: "编辑器", rect: { x: 60, y: 60, w: 760, h: 580 } },
  { id: "c", app: "浏览器", rect: { x: 1240, y: 0, w: 680, h: 620 } },
];
const AREA: Rect = { x: 0, y: 0, w: 1920, h: 1080 };

function SpaceOpsPanelInner(): React.ReactElement {
  const desks = useMemo(() => {
    const v = new VirtualDesktops();
    v.create("工作");
    v.create("写作");
    v.assignWindow("a", 0);
    v.assignWindow("b", 0);
    v.assignWindow("c", 1);
    return v;
  }, []);
  const [desk, setDesk] = useState(0);
  const [load, setLoad] = useState(0.4);

  const organize = useMemo(() => new OrganizeAssistant(), []);
  const arranged = useMemo(() => organize.autoArrange(DEMO_WINS, AREA), [organize]);
  const minimap = useMemo(() => new Minimap(AREA, 180, 104), []);
  const degrade = useMemo(() => new DegradeChain(), []);
  const tier = degrade.feed(load);

  const onDesk = desks.windowsOn(desk);
  const cluttered = organize.suggest(DEMO_WINS) === "arrange";

  return (
    <div className="so-panel" role="complementary" aria-label="空间管理">
      <header className="so-head">
        <LayoutGrid size={16} aria-hidden="true" />
        <span className="so-title">空间管理</span>
      </header>

      <section className="so-group" aria-label="虚拟桌面">
        <h5>虚拟桌面</h5>
        <div className="so-chips">
          {Array.from({ length: desks.count }, (_, i) => (
            <button
              key={i}
              type="button"
              className={i === desk ? "so-chip so-chip--on" : "so-chip"}
              aria-pressed={i === desk}
              onClick={() => setDesk(i)}
            >
              {desks.name(i)} · {desks.windowsOn(i).length}
            </button>
          ))}
        </div>
        <p className="so-hint">
          {onDesk.length > 0 ? `当前桌面：${onDesk.join("、")}` : "当前桌面暂无窗口"}
        </p>
      </section>

      <section className="so-group" aria-label="整理助手">
        <h5>整理助手</h5>
        <p className="so-hint">{cluttered ? "检测到窗口重叠，建议自动铺排" : "窗口布局良好，无需整理"}</p>
        <div className="so-minimap" role="img" aria-label="整理预览小地图">
          {arranged.map((w) => {
            const r = minimap.scaleRect(w.rect);
            return <span key={w.id} className="so-mm-cell" style={{ left: r.x, top: r.y, width: r.w, height: r.h }} />;
          })}
        </div>
      </section>

      <section className="so-group" aria-label="性能降级">
        <h5>性能降级</h5>
        <label className="so-slider-row">
          <Gauge size={14} aria-hidden="true" />
          <input
            type="range"
            min={0}
            max={100}
            value={Math.round(load * 100)}
            onChange={(e) => setLoad(Number(e.target.value) / 100)}
            aria-label="模拟系统负载"
          />
          <span className="so-tier">{tier}</span>
        </label>
        <p className="so-hint">降级链：full → medium → low（滞回回升）</p>
      </section>

      <footer className="so-foot">
        <Sparkles size={12} aria-hidden="true" />
        <span>UNREAL-X · AI-06 · X01251~X01500</span>
      </footer>
    </div>
  );
}

/** 桌面浮层入口：默认收起，由 VisionRuntime 挂载（零打扰）。 */
export function SpaceOpsPanel(): React.ReactElement {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        className={open ? "so-launcher so-launcher--on" : "so-launcher"}
        aria-expanded={open}
        aria-label="空间管理面板"
        onClick={() => setOpen((v) => !v)}
      >
        <LayoutGrid size={16} aria-hidden="true" />
      </button>
      {open ? <SpaceOpsPanelInner /> : null}
    </>
  );
}
