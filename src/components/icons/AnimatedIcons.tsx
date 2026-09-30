/**
 * AI-17 · U-10 动效图标八件套（Animated Icons）
 * 全部 SVG + CSS 动画，transform/opacity-only，reduce-motion 一键停用
 * （interactions.css [data-reduce-motion] 全局覆盖）。
 * 八件套：加载弧线、同步循环、告警呼吸、成功勾绘、网络波动、
 *         音量级联、电池充电呼吸、时钟指针步进。
 */
import React from "react";

export type AnimatedIconName =
  | "loading" | "sync" | "alert" | "success"
  | "network" | "volume" | "battery" | "clock";

const S = 18; // 默认 18px（U-10 尺寸令牌档）

const stroke = "currentColor";

export function AnimatedIcon({ name, size = S, className }: {
  name: AnimatedIconName;
  size?: number;
  className?: string;
}): React.ReactElement {
  const cls = `ai17-anim ai17-anim-${name}${className ? ` ${className}` : ""}`;
  const common = { width: size, height: size, viewBox: "0 0 24 24", className: cls, "aria-hidden": true as const };
  switch (name) {
    case "loading":
      return (
        <svg {...common} fill="none">
          <circle cx="12" cy="12" r="9" stroke={stroke} strokeWidth="2" opacity="0.2" />
          <path d="M21 12a9 9 0 0 0-9-9" stroke={stroke} strokeWidth="2" strokeLinecap="round" />
        </svg>
      );
    case "sync":
      return (
        <svg {...common} fill="none">
          <path d="M20 12a8 8 0 1 1-2.34-5.66" stroke={stroke} strokeWidth="2" strokeLinecap="round" />
          <path d="M20 3v4h-4" stroke={stroke} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      );
    case "alert":
      return (
        <svg {...common} fill="none">
          <path d="M12 3 2.5 20h19L12 3Z" stroke={stroke} strokeWidth="2" strokeLinejoin="round" />
          <path d="M12 10v4" stroke={stroke} strokeWidth="2" strokeLinecap="round" />
        </svg>
      );
    case "success":
      return (
        <svg {...common} fill="none">
          <circle cx="12" cy="12" r="9" stroke={stroke} strokeWidth="2" />
          <path className="ai17-anim-draw" d="M7.5 12.5 10.5 15.5 16.5 8.5" stroke={stroke} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      );
    case "network":
      return (
        <svg {...common} fill="none">
          <path className="ai17-anim-wave-1" d="M4 14v-4" stroke={stroke} strokeWidth="2" strokeLinecap="round" />
          <path className="ai17-anim-wave-2" d="M9 17V7" stroke={stroke} strokeWidth="2" strokeLinecap="round" />
          <path className="ai17-anim-wave-3" d="M14 15V9" stroke={stroke} strokeWidth="2" strokeLinecap="round" />
          <path className="ai17-anim-wave-4" d="M19 18V6" stroke={stroke} strokeWidth="2" strokeLinecap="round" />
        </svg>
      );
    case "volume":
      return (
        <svg {...common} fill="none">
          <path d="M4 9v6h4l5 4V5L8 9H4Z" stroke={stroke} strokeWidth="2" strokeLinejoin="round" />
          <path className="ai17-anim-wave-1" d="M16 9.5a4 4 0 0 1 0 5" stroke={stroke} strokeWidth="2" strokeLinecap="round" />
          <path className="ai17-anim-wave-2" d="M18.5 7a8 8 0 0 1 0 10" stroke={stroke} strokeWidth="2" strokeLinecap="round" />
        </svg>
      );
    case "battery":
      return (
        <svg {...common} fill="none">
          <rect x="2" y="7" width="17" height="10" rx="2" stroke={stroke} strokeWidth="2" />
          <path d="M21.5 10.5v3" stroke={stroke} strokeWidth="2" strokeLinecap="round" />
          <path className="ai17-anim-charge" d="M10.5 9 7.5 12.5h3l-1 3 4-4.5h-3l1-2.5Z" fill={stroke} />
        </svg>
      );
    case "clock":
      return (
        <svg {...common} fill="none">
          <circle cx="12" cy="12" r="9" stroke={stroke} strokeWidth="2" />
          <path className="ai17-anim-hand" d="M12 7v5l3.5 2" stroke={stroke} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      );
  }
}
