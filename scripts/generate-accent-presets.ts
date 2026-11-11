import { readFileSync, writeFileSync } from "node:fs";
import {
  ACCENT_FAMILIES,
  ACCENT_SHADES,
  DEFAULT_ACCENT,
  makeAccentTokens,
  oklchHex,
  type ColorTheme,
} from "../src/lib/accent-color";
const css = readFileSync(new URL("../node_modules/tailwindcss/theme.css", import.meta.url), "utf8");
const variables = Object.fromEntries(
  [...css.matchAll(/--color-([\w-]+):\s*([^;]+);/g)].map((match) => [match[1], match[2]]),
);
const styles = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
function surface(theme: ColorTheme) {
  const block = styles.split(theme === "dark" ? ".dark {" : ":root {")[1].split("}")[0];
  const tokens = Object.fromEntries(
    [...block.matchAll(/--([\w-]+):\s*([^;]+);/g)].map((match) => [match[1], match[2]]),
  );
  return {
    background: oklchHex(tokens.background),
    overlay: oklchHex(tokens["overlay-bg"]),
    card: oklchHex(tokens.card),
  };
}
const surfaces = { light: surface("light"), dark: surface("dark") };
const presets = Object.fromEntries(
  ACCENT_FAMILIES.flatMap((family) =>
    ACCENT_SHADES.map((shade) => {
      const color = oklchHex(variables[`${family}-${shade}`]);
      return [
        `${family}-${shade}`,
        {
          color,
          light: makeAccentTokens(color, "light", surfaces.light),
          dark: makeAccentTokens(color, "dark", surfaces.dark),
        },
      ];
    }),
  ),
);
const content =
  JSON.stringify(
    {
      default: DEFAULT_ACCENT,
      families: ACCENT_FAMILIES,
      shades: ACCENT_SHADES,
      surfaces,
      presets,
    },
    null,
    2,
  ) + "\n";
const output = new URL("../src/lib/accent-presets.json", import.meta.url);
if (process.argv.includes("--check")) {
  if (readFileSync(output, "utf8") !== content)
    throw Error("Accent presets are stale; run bun scripts/generate-accent-presets.ts");
} else writeFileSync(output, content);
console.log("Accent presets match the installed Tailwind palette.");
