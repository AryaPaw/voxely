async function capture(page) {
  // Use the app's default inner size, not a separate documentation-only size.
  await page.goto("http://127.0.0.1:1425/");
  const viewport = await page.evaluate(async () => {
    const response = await fetch("/src-tauri/tauri.conf.json");
    if (!response.ok) throw new Error("Unable to read the main window configuration");
    const config = await response.json();
    const main = config.app.windows.find((window) => window.label === "main");
    if (
      !main ||
      !Number.isInteger(main.width) ||
      !Number.isInteger(main.height) ||
      main.width <= 0 ||
      main.height <= 0
    ) {
      throw new Error("Main window configuration must define a positive integer size");
    }
    return { width: main.width, height: main.height };
  });
  await page.setViewportSize(viewport);
  await page.emulateMedia({ reducedMotion: "reduce" });
  const sections = [
    ["history", "History"],
    ["statistics", "Usage statistics"],
    ["filters", "Filters"],
    ["compare", "Compare"],
    ["general", "General"],
    ["appearance", "Appearance"],
    ["transcription", "Transcription"],
    ["about", "About"],
  ];

  for (const [section, heading] of sections) {
    await page.goto(`http://127.0.0.1:1425/?section=${section}`);
    await page.getByRole("heading", { name: heading, exact: true }).waitFor();
    if (section === "history") {
      await page.getByRole("article").last().waitFor();
    } else if (section === "statistics") {
      await page.getByText("openai/whisper-large-v3", { exact: true }).waitFor();
    } else if (section === "compare") {
      await page.getByRole("button", { name: "Record clip", exact: true }).click();
      await page.getByRole("button", { name: "Stop", exact: true }).click();
      await page.getByRole("button", { name: "Compare", exact: true }).last().click();
      await page.getByText("Let's move the design review", { exact: false }).first().waitFor();
      await page.waitForFunction(() => {
        const audio = document.querySelector("audio");
        return audio && audio.readyState >= 1 && Number.isFinite(audio.duration);
      });
    }
    await page.evaluate(() => document.fonts.ready);
    await page.mouse.move(viewport.width - 10, viewport.height - 10);
    await page.screenshot({
      path: `docs/images/shot-${section}.png`,
      fullPage: false,
      scale: "css",
      animations: "disabled",
    });
  }
}
