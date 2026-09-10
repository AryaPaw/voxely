export function applyTheme(theme: string): void {
  const root = document.documentElement;
  const dark = theme === "dark" || (theme === "system" && prefersDark());
  root.classList.toggle("dark", dark);
  root.dataset.theme = dark ? "dark" : "light";
}

export function resolvedTheme(theme: string): "light" | "dark" {
  return theme === "dark" || (theme === "system" && prefersDark()) ? "dark" : "light";
}

export function watchSystemTheme(theme: string): () => void {
  if (theme !== "system" || typeof window.matchMedia !== "function") {
    return () => undefined;
  }
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  const onChange = () => applyTheme("system");
  media.addEventListener("change", onChange);
  return () => media.removeEventListener("change", onChange);
}

function prefersDark(): boolean {
  return typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-color-scheme: dark)").matches
    : false;
}
