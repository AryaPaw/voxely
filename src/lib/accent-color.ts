export type ColorTheme = "light" | "dark";
export const ACCENT_FAMILIES = [
  "slate",
  "gray",
  "zinc",
  "neutral",
  "stone",
  "red",
  "orange",
  "amber",
  "yellow",
  "lime",
  "green",
  "emerald",
  "teal",
  "cyan",
  "sky",
  "blue",
  "indigo",
  "violet",
  "purple",
  "fuchsia",
  "pink",
  "rose",
] as const;
export const ACCENT_SHADES = [400, 500, 600] as const;
export interface AccentColor {
  family: string;
  shade: number;
}
export const DEFAULT_ACCENT: AccentColor = { family: "sky", shade: 500 };
export type Hsl = [number, number, number];
export function rgb(hex: string): number[] {
  return [1, 3, 5].map((offset) => parseInt(hex.slice(offset, offset + 2), 16));
}
export function hex(channels: number[]): string {
  return `#${channels
    .map((channel) =>
      Math.round(Math.max(0, Math.min(255, channel)))
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}
export function oklchHex(css: string): string {
  const match = /oklch\(([\d.]+)(%)?\s+([\d.]+)\s+([\d.]+|none)\)/.exec(css);
  if (!match) throw new Error(`Invalid palette color: ${css}`);
  const l = Number(match[1]) / (match[2] ? 100 : 1),
    c = Number(match[3]),
    h = ((Number(match[4]) || 0) * Math.PI) / 180;
  const a = c * Math.cos(h),
    b = c * Math.sin(h),
    ll = (l + 0.3963377774 * a + 0.2158037573 * b) ** 3,
    mm = (l - 0.1055613458 * a - 0.0638541728 * b) ** 3,
    ss = (l - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return hex(
    [
      4.0767416621 * ll - 3.3077115913 * mm + 0.2309699292 * ss,
      -1.2684380046 * ll + 2.6097574011 * mm - 0.3413193965 * ss,
      -0.0041960863 * ll - 0.7034186147 * mm + 1.707614701 * ss,
    ].map(
      (value) => 255 * (value <= 0.0031308 ? 12.92 * value : 1.055 * value ** (1 / 2.4) - 0.055),
    ),
  );
}
export function toHsl(value: string): Hsl {
  const [r, g, b] = rgb(value).map((channel) => channel / 255),
    max = Math.max(r, g, b),
    min = Math.min(r, g, b),
    delta = max - min,
    l = (max + min) / 2;
  const hue =
    delta === 0
      ? 0
      : max === r
        ? ((g - b) / delta) % 6
        : max === g
          ? (b - r) / delta + 2
          : (r - g) / delta + 4;
  return [
    Math.round((hue * 60 + 360) % 360),
    Math.round(delta === 0 ? 0 : (delta / (1 - Math.abs(2 * l - 1))) * 100),
    Math.round(l * 100),
  ];
}
export function fromHsl([h, saturation, lightness]: Hsl): string {
  const s = saturation / 100,
    l = lightness / 100,
    a = s * Math.min(l, 1 - l);
  const channel = (n: number) => {
    const k = (n + h / 30) % 12;
    return 255 * (l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1)));
  };
  return hex([channel(0), channel(8), channel(4)]);
}
export function contrast(a: string, b: string): number {
  const luminance = (value: string) =>
    rgb(value)
      .map((channel) => channel / 255)
      .map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
      .reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i], 0);
  const x = luminance(a),
    y = luminance(b);
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}
export function contrastForeground(background: string): string {
  return contrast(background, "#ffffff") >= contrast(background, "#000000") ? "#ffffff" : "#000000";
}
export function mix(a: string, b: string, amount: number): string {
  const first = rgb(a),
    second = rgb(b);
  return hex(first.map((v, i) => v * (1 - amount) + second[i] * amount));
}
export function readableTone(
  color: string,
  background: string,
  theme: ColorTheme,
  ratio = 4.5,
): string {
  if (contrast(color, background) >= ratio) return color;
  const [h, s, l] = toHsl(color),
    direction = theme === "light" ? -1 : 1;
  for (let delta = 1; delta <= 100; delta++) {
    const next = fromHsl([h, s, Math.max(0, Math.min(100, l + direction * delta))]);
    if (contrast(next, background) >= ratio) return next;
  }
  return theme === "light" ? "#000000" : "#ffffff";
}
export interface AccentSurfaces {
  background: string;
  overlay: string;
  card: string;
}
export function makeAccentTokens(
  base: string,
  theme: ColorTheme,
  surfaces: AccentSurfaces,
): Record<string, string> {
  const dark = theme === "dark",
    { background, overlay, card } = surfaces;
  // Filled controls own their foreground. Inline accents must contrast with the page.
  const primary = base;
  const primaryForeground = contrastForeground(primary);
  const hover = mix(primary, primaryForeground === "#ffffff" ? "#000000" : "#ffffff", 0.12);
  const textOnSurfaces = (color: string) =>
    readableTone(readableTone(color, background, theme), card, theme);
  const primaryText = textOnSurfaces(primary);
  const primaryTextHover = textOnSurfaces(mix(primaryText, dark ? "#ffffff" : "#000000", 0.12));
  const soft = mix(primary, card, dark ? 0.82 : 0.91);
  const softHover = mix(primary, card, dark ? 0.72 : 0.84);
  const softText = readableTone(readableTone(primaryText, soft, theme), softHover, theme);
  const sidebarHover = mix(primary, card, dark ? 0.94 : 0.96);
  const sidebarActive = mix(primary, card, dark ? 0.82 : 0.88);
  const sidebarActiveHover = mix(primary, card, dark ? 0.76 : 0.82);
  const sidebarText = readableTone(
    readableTone(primaryText, sidebarActive, theme),
    sidebarActiveHover,
    theme,
  );
  const overlayAccent = readableTone(primary, overlay, theme);
  return {
    "--primary": primary,
    "--primary-foreground": primaryForeground,
    "--primary-hover": hover,
    "--primary-text": primaryText,
    "--primary-text-hover": primaryTextHover,
    "--ring": primaryText,
    "--accent": soft,
    "--accent-foreground": softText,
    "--accent-hover": softHover,
    "--sidebar-primary": primary,
    "--sidebar-primary-foreground": primaryForeground,
    "--sidebar-ring": primaryText,
    "--sidebar-accent": sidebarActive,
    "--sidebar-accent-foreground": sidebarText,
    "--sidebar-hover": sidebarHover,
    "--sidebar-active-hover": sidebarActiveHover,
    "--chart-1": primaryText,
    "--chart-2": mix(primaryText, dark ? "#ffffff" : "#000000", 0.18),
    "--chart-3": mix(primaryText, dark ? "#ffffff" : "#000000", 0.35),
    "--chart-4": mix(primaryText, dark ? "#ffffff" : "#000000", 0.5),
    "--chart-5": mix(primaryText, dark ? "#ffffff" : "#000000", 0.65),
    "--overlay-recording": overlayAccent,
    "--overlay-status": overlayAccent,
    "--overlay-glow": overlayAccent,
    "--overlay-armed": overlayAccent,
    "--overlay-armed-fg": readableTone(overlayAccent, overlay, theme),
    "--overlay-hover-bg": overlay,
    "--overlay-hover-glow": overlayAccent,
  };
}
