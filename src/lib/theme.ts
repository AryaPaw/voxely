export function applyTheme(theme: string): void {
  const root = document.documentElement;
  const preferDark =
    typeof window.matchMedia === "function"
      ? window.matchMedia("(prefers-color-scheme: dark)").matches
      : false;
  const dark = theme === "dark" || (theme === "system" && preferDark);
  root.classList.toggle("dark", dark);
  root.dataset.theme = dark ? "dark" : "light";
}
