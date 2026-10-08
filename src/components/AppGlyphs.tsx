/** 四款官方软件的风格化图标（批次E-14）：
 * 每款一个独立视觉语言，同一 viewBox 网格（0 0 48 48），矢量无损缩放 ——
 * - Write  钢笔尖+墨迹（深蓝墨水，文学感）
 * - Mind   节点星图（青色发光节点连线）
 * - Code   像素 </> + 扫描线（深紫，极客感）
 * - Fate   星轨星盘（金色环+六芒星，神秘学）
 * 仅替换字形：底座/网格/尺寸仍由 .desktop-icon-tile / .tb-app-icon 控制。
 * 接口与 Lucide 组件兼容（size/className），可直接替换 desktopIconDefs 里的 icon。
 */

export interface GlyphProps {
  size?: number;
  className?: string;
}

function base(size?: number, className?: string): { width: number; height: number; className: string } {
  return { width: size ?? 24, height: size ?? 24, className: className ?? "" };
}

/** Variable Write — 钢笔尖 + 一笔墨迹（墨水蓝渐变） */
export function WriteGlyph(props: GlyphProps): React.ReactElement {
  const { width, height, className } = base(props.size, props.className);
  return (
    <svg width={width} height={height} viewBox="0 0 48 48" className={className} aria-hidden>
      <defs>
        <linearGradient id="vw-ink" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#bfe0ff" />
          <stop offset="1" stopColor="#5b8fd6" />
        </linearGradient>
      </defs>
      {/* 一笔墨迹（弧线，笔触渐细由两段路径模拟） */}
      <path d="M8 40 C 16 34, 26 30, 40 12" stroke="url(#vw-ink)" strokeWidth="2.6" strokeLinecap="round" fill="none" opacity="0.85" />
      {/* 钢笔尖 */}
      <path
        d="M28 14 L38 6 C40.5 8.5 42 12 40 16 L32 22 Z"
        fill="#0e2a4d"
        stroke="url(#vw-ink)"
        strokeWidth="2"
        strokeLinejoin="round"
      />
      <circle cx="33.4" cy="13.4" r="1.8" fill="url(#vw-ink)" />
      {/* 笔尖缝线 */}
      <path d="M33.4 13.4 L28 22 L24.5 27.5" stroke="url(#vw-ink)" strokeWidth="1.6" strokeLinecap="round" fill="none" />
      {/* 墨滴 */}
      <circle cx="21.5" cy="31.5" r="2.2" fill="url(#vw-ink)" opacity="0.9" />
    </svg>
  );
}

/** Variable Mind — 节点星图（青色发光节点） */
export function MindGlyph(props: GlyphProps): React.ReactElement {
  const { width, height, className } = base(props.size, props.className);
  return (
    <svg width={width} height={height} viewBox="0 0 48 48" className={className} aria-hidden>
      <defs>
        <radialGradient id="vm-glow">
          <stop offset="0" stopColor="#aef4ff" />
          <stop offset="1" stopColor="#39d6f2" stopOpacity="0.15" />
        </radialGradient>
      </defs>
      {/* 连线 */}
      <g stroke="#54d8ee" strokeWidth="1.7" strokeLinecap="round" opacity="0.85">
        <path d="M24 24 L11 13" />
        <path d="M24 24 L38 10" />
        <path d="M24 24 L40 32" />
        <path d="M24 24 L14 37" />
      </g>
      {/* 中心光晕 + 核心节点 */}
      <circle cx="24" cy="24" r="9.5" fill="url(#vm-glow)" />
      <circle cx="24" cy="24" r="4.6" fill="#0b2f38" stroke="#7fe7f7" strokeWidth="2" />
      {/* 外围节点 */}
      <g fill="#0b2f38" stroke="#54d8ee" strokeWidth="1.9">
        <circle cx="11" cy="13" r="3.1" />
        <circle cx="38" cy="10" r="3.1" />
        <circle cx="40" cy="32" r="3.1" />
        <circle cx="14" cy="37" r="3.1" />
      </g>
      {/* 星点 */}
      <circle cx="31" cy="20" r="1" fill="#bff2fb" />
      <circle cx="20" cy="31" r="0.9" fill="#bff2fb" opacity="0.8" />
    </svg>
  );
}

/** Variable Code — 像素 </> + 扫描线（深紫，极客感） */
export function CodeGlyph(props: GlyphProps): React.ReactElement {
  const { width, height, className } = base(props.size, props.className);
  // 像素化 "<" 与 ">"（单元 3px 网格）
  const px = (x: number, y: number, w = 1, h = 1): string =>
    `M${10 + x * 3} ${12 + y * 3} h${w * 3} v${h * 3} h${-w * 3} Z`;
  const left = [
    px(2, 0), px(1, 1), px(0, 2), px(0, 3), px(0, 4), px(0, 5), px(1, 6), px(2, 7),
  ].join(" ");
  const right = [
    px(9, 0), px(10, 1), px(11, 2), px(11, 3), px(11, 4), px(11, 5), px(10, 6), px(9, 7),
  ].join(" ");
  const slash = [px(6, 1), px(5, 3), px(4, 5), px(6, 1), px(5, 3)].join(" ");
  return (
    <svg width={width} height={height} viewBox="0 0 48 48" className={className} aria-hidden>
      <defs>
        <linearGradient id="vc-px" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#d6c6ff" />
          <stop offset="1" stopColor="#9a7bff" />
        </linearGradient>
      </defs>
      <g fill="url(#vc-px)">
        <path d={left} />
        <path d={right} />
        <path d={slash} opacity="0.9" />
      </g>
      {/* 扫描线质感 */}
      <g stroke="#c9b8ff" strokeWidth="0.7" opacity="0.28">
        <path d="M9 20 h30" />
        <path d="M9 27 h30" />
        <path d="M9 34 h30" />
      </g>
      {/* 角标光标 */}
      <rect x="33" y="35" width="5" height="3" fill="#8f76ff" opacity="0.95" />
    </svg>
  );
}

/** Variable Fate — 星轨星盘 + 六芒星（金线，神秘学） */
export function FateGlyph(props: GlyphProps): React.ReactElement {
  const { width, height, className } = base(props.size, props.className);
  return (
    <svg width={width} height={height} viewBox="0 0 48 48" className={className} aria-hidden>
      <defs>
        <linearGradient id="vf-gold" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#ffe9a8" />
          <stop offset="1" stopColor="#d8a83c" />
        </linearGradient>
      </defs>
      {/* 外环 + 内环 + 刻度 */}
      <g stroke="url(#vf-gold)" fill="none">
        <circle cx="24" cy="24" r="16" strokeWidth="1.8" />
        <circle cx="24" cy="24" r="11.5" strokeWidth="1" opacity="0.65" />
        <g strokeWidth="1.2" opacity="0.8">
          <path d="M24 6.2 v3" />
          <path d="M24 38.8 v3" />
          <path d="M6.2 24 h3" />
          <path d="M38.8 24 h3" />
        </g>
        {/* 六芒星（两个三角） */}
        <path d="M24 14.5 L32.4 29 L15.6 29 Z" strokeWidth="1.6" strokeLinejoin="round" />
        <path d="M24 33.5 L15.6 19 L32.4 19 Z" strokeWidth="1.6" strokeLinejoin="round" opacity="0.85" />
        {/* 轨道弧 */}
        <path d="M13 33 A 15 15 0 0 0 35 33" strokeWidth="0.9" opacity="0.5" />
      </g>
      <circle cx="24" cy="24" r="2" fill="url(#vf-gold)" />
      <circle cx="35.2" cy="14.6" r="1.3" fill="#ffe9a8" />
    </svg>
  );
}

/** 每款软件的风格色（VWM 标题栏圆点等点缀跟随）。 */
export function appAccent(app: string): string {
  switch (app) {
    case "write": return "#7fb2e8"; // 墨水蓝
    case "mindmap": return "#4fd8ee"; // 青色星图
    case "project": return "#a88bff"; // 深紫像素
    case "fate": return "#e8c15c"; // 星盘金
    default: return "#7fa8d6";
  }
}

/** 软件图标组件统一接口（Lucide 或风格化 SVG 字形，用法兼容 size/className/strokeWidth）。 */
export type AppGlyph = React.ElementType;
