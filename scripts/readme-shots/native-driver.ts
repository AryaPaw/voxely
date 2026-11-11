import scenes from "./scenes.json";
import { emit as nativeEmit } from "../../node_modules/@tauri-apps/api/event";

// A fixed local day makes demo history and date filters repeatable. No application
// source, persisted data, window chrome or layout styles are replaced.
const RealDate = Date;
const epoch = new RealDate("2026-11-10T15:40:00Z").getTime();
globalThis.Date = new Proxy(RealDate, {
  construct: (target, args) => Reflect.construct(target, args.length ? args : [epoch]),
  apply: () => new RealDate(epoch).toString(),
  get: (target, key) => (key === "now" ? () => epoch : Reflect.get(target, key)),
});

const errors: string[] = [];
window.addEventListener("error", (event) => errors.push(event.message));
window.addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));

const parameters = new URLSearchParams(location.search);
const section = parameters.get("shot") ?? parameters.get("section") ?? "history";
const heading = scenes.find((scene) => scene.section === section)?.heading;

async function until(check: () => boolean, message: string) {
  const deadline = performance.now() + 15_000;
  while (!check()) {
    if (errors.length) throw new Error(errors.join("\n"));
    if (performance.now() > deadline) throw new Error(message);
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}

async function prepare() {
  history.scrollRestoration = "manual";
  window.scrollTo(0, 0);
  await import("../../src/main");
  await until(
    () =>
      section === "hud"
        ? document.querySelector(".overlay-status")?.textContent === heading
        : [...document.querySelectorAll("h1,h2")].some((el) => el.textContent === heading),
    `Heading not rendered: ${section}`,
  );
  if (section === "history") {
    await until(() => document.querySelectorAll("article").length === 4, "History not loaded");
  }
  if (section.startsWith("statistics")) {
    await until(
      () => document.body.textContent?.includes("openai/whisper-large-v3") === true,
      "Statistics not loaded",
    );
    if (section === "statistics-chart") {
      const title = document.getElementById("usage-chart-title");
      if (!title) throw new Error("Statistics chart heading not found");
      const card = title.closest<HTMLElement>('[data-slot="card"]')!;
      const viewport = document.querySelector<HTMLElement>(".app-content > .overflow-auto")!;
      viewport.scrollTop += card.getBoundingClientRect().top - viewport.getBoundingClientRect().top;
    }
  }
  if (section === "compare") {
    const click = async (label: string) => {
      const button = () =>
        [...document.querySelectorAll<HTMLButtonElement>("button")]
          .filter((el) => el.textContent?.trim() === label && !el.disabled)
          .at(-1);
      await until(() => Boolean(button()), `Missing button: ${label}`);
      button()!.click();
    };
    await click("Record clip");
    await click("Stop");
    await click("Compare");
    await until(
      () => document.body.textContent?.includes("Let's move the design review") === true,
      "Compare results not loaded",
    );
  }
  await document.fonts.ready;
  // Capture completed rendering, with production CSS and actual enabled controls.
  await new Promise((resolve) => setTimeout(resolve, 800));
  for (const animation of document.getAnimations()) {
    if (animation.effect?.getComputedTiming().iterations === Infinity) animation.pause();
    else animation.finish();
  }
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
  const alerts = [...document.querySelectorAll('[role="alert"], [data-sonner-toast]')]
    .map((el) => el.textContent)
    .filter(Boolean);
  if (errors.length || alerts.length) throw new Error([...errors, ...alerts].join("\n"));
  window.scrollTo(0, 0);
  const bounds = (selector: string) => {
    const rect = document.querySelector(selector)?.getBoundingClientRect();
    return rect ? { x: rect.x, y: rect.y, width: rect.width, height: rect.height } : null;
  };
  await nativeEmit("readme://ready", {
    section,
    report: {
      title: document.title,
      width: innerWidth,
      height: innerHeight,
      dpr: devicePixelRatio,
      errors,
      header: bounds(".window-titlebar"),
      logo: bounds(".window-drag-region img"),
      version: bounds(".window-version"),
      scrollY,
    },
  });
}

void prepare().catch((error: unknown) => {
  void nativeEmit("readme://ready", { section, report: { error: String(error) } });
});
