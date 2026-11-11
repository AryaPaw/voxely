import data from "./accent-presets.json";
import {
  DEFAULT_ACCENT,
  makeAccentTokens,
  type AccentColor,
  type ColorTheme,
} from "./accent-color";
export { ACCENT_FAMILIES, ACCENT_SHADES, DEFAULT_ACCENT, type AccentColor } from "./accent-color";
type Preset = { color: string; light: Record<string, string>; dark: Record<string, string> };
const presets: Record<string, Preset> = data.presets;
export function isAccentColor(value: unknown): value is AccentColor {
  if (!value || typeof value !== "object") return false;
  const color = value as AccentColor;
  return (
    typeof color.family === "string" &&
    typeof color.shade === "number" &&
    Object.hasOwn(presets, `${color.family}-${color.shade}`)
  );
}
export function accentPreset(value: unknown = DEFAULT_ACCENT): Preset {
  const color = isAccentColor(value) ? value : DEFAULT_ACCENT;
  return presets[`${color.family}-${color.shade}`];
}
export function applyAccent(theme: ColorTheme, value: unknown = DEFAULT_ACCENT): void {
  const tokens = accentPreset(value)[theme];
  for (const [name, color] of Object.entries(tokens))
    document.documentElement.style.setProperty(name, color);
}
export function applyPreviewAccent(theme: ColorTheme, color: string): void {
  if (!/^#[\da-f]{6}$/i.test(color)) return;
  for (const [name, value] of Object.entries(makeAccentTokens(color, theme, data.surfaces[theme])))
    document.documentElement.style.setProperty(name, value);
}
