/**
 * AI-17 · U-10 图标语言 2.0 + Z-01 官方图标网格对齐
 *
 * 「图标语义字典」：同一语义全仓唯一图标（如「删除」永远是 trash）。
 * 组件层一律 `resolveIcon("delete")` 取 lucide 组件，禁止散落直引。
 * 同时登记每个语义的 Segoe Fluent Icons 对应物（系统自带字体，零依赖），
 * 规格：16/20/24/32 四档（Z-01 官方网格），keyline 细节见 docs/ICON-SPEC.md。
 */
import type { LucideIcon } from "lucide-react";
import {
  Trash2, X, Search, Settings, Folder, File, Copy, Scissors, ClipboardList,
  RefreshCw, Plus, Minus, ChevronRight, ChevronLeft, ChevronDown, ChevronUp,
  Check, AlertTriangle, Info, Download, Upload, Play, Pause, Power, Home,
  Bell, Volume2, VolumeX, Wifi, WifiOff, Battery, Clock, Calendar, Lock,
  Unlock, Eye, EyeOff, Star, Pin, Heart, Edit3, Save, ExternalLink,
} from "lucide-react";

export type IconSemantic =
  | "delete" | "close" | "search" | "settings" | "folder" | "file"
  | "copy" | "cut" | "paste" | "refresh" | "add" | "remove"
  | "next" | "previous" | "expand" | "collapse"
  | "confirm" | "warning" | "info"
  | "download" | "upload" | "play" | "pause" | "power" | "home"
  | "notification" | "sound-on" | "sound-off" | "network" | "network-off"
  | "battery" | "clock" | "calendar" | "lock" | "unlock"
  | "visible" | "hidden" | "favorite" | "pin" | "like"
  | "edit" | "save" | "open-external";

/** 语义 → lucide 组件（全仓唯一映射；registry 单测守护语义冲突）。 */
export const ICON_SEMANTICS: Record<IconSemantic, LucideIcon> = {
  delete: Trash2,
  close: X,
  search: Search,
  settings: Settings,
  folder: Folder,
  file: File,
  copy: Copy,
  cut: Scissors,
  paste: ClipboardList,
  refresh: RefreshCw,
  add: Plus,
  remove: Minus,
  next: ChevronRight,
  previous: ChevronLeft,
  expand: ChevronDown,
  collapse: ChevronUp,
  confirm: Check,
  warning: AlertTriangle,
  info: Info,
  download: Download,
  upload: Upload,
  play: Play,
  pause: Pause,
  power: Power,
  home: Home,
  notification: Bell,
  "sound-on": Volume2,
  "sound-off": VolumeX,
  network: Wifi,
  "network-off": WifiOff,
  battery: Battery,
  clock: Clock,
  calendar: Calendar,
  lock: Lock,
  unlock: Unlock,
  visible: Eye,
  hidden: EyeOff,
  favorite: Star,
  pin: Pin,
  like: Heart,
  edit: Edit3,
  save: Save,
  "open-external": ExternalLink,
};

/** 图标尺寸三档令牌（Z-01 规格：默认 18 / 设置区 16 / 桌面级入口 24）。 */
export type IconSize = 16 | 18 | 24 | 32;
export const ICON_SIZE_MAP: Record<"default" | "settings" | "desktop" | "large", IconSize> = {
  settings: 16,
  default: 18,
  desktop: 24,
  large: 32,
};

/** Segoe Fluent Icons 对应物映射（Z-01：系统图标首选来源；lucide 仅补缺口）。 */
export const SEGOE_FLUENT_MAP: Partial<Record<IconSemantic, string>> = {
  delete: "\uE74D", // Delete
  close: "\uE711", // ChromeClose
  search: "\uE721", // Search
  settings: "\uE713", // Setting
  folder: "\uE8B7", // Folder
  file: "\uE7C3", // Page
  copy: "\uE8C8", // Copy
  cut: "\uE8C6", // Cut
  paste: "\uE77F", // Paste
  refresh: "\uE72C", // Refresh
  add: "\uE710", // Add
  remove: "\uE738", // Remove
  confirm: "\uE73E", // CheckMark
  warning: "\uE7BA", // Warning
  info: "\uE946", // Info
  download: "\uE896", // Download
  upload: "\uE898", // Upload
  play: "\uE768", // Play
  pause: "\uE769", // Pause
  power: "\uE7E8", // PowerButton
  home: "\uE80F", // Home
  notification: "\uEA91", // Ringer
  "sound-on": "\uE767", // Volume
  "sound-off": "\uE74F", // Mute
  network: "\uE701", // Wifi
  "network-off": "\uE702", // Offline
  battery: "\uE850", // BatteryCharging10 家族
  clock: "\uE823", // Recent
  calendar: "\uE787", // Calendar
  lock: "\uE72E", // Lock
  unlock: "\uE785", // Unlock
  visible: "\uE7B3", // RedEye
  hidden: "\uED1A", // Hide
  favorite: "\uE734", // FavoriteStar
  pin: "\uE718", // Pin
  edit: "\uE70F", // Edit
  save: "\uE74E", // Save
  "open-external": "\uE8A7", // OpenInNewWindow
};

/** 取语义对应的 lucide 组件。未知语义在 dev 下抛错（防漂移）。 */
export function resolveIcon(semantic: IconSemantic): LucideIcon {
  const icon = ICON_SEMANTICS[semantic];
  if (!icon) throw new Error(`iconRegistry: unknown semantic "${String(semantic)}"`);
  return icon;
}

/** 取语义对应的 Segoe Fluent 码位（无登记返回 null → 用 lucide 补缺口）。 */
export function resolveSegoe(semantic: IconSemantic): string | null {
  return SEGOE_FLUENT_MAP[semantic] ?? null;
}

export const ALL_ICON_SEMANTICS = Object.keys(ICON_SEMANTICS) as IconSemantic[];
