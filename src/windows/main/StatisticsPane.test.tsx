import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import process from "node:process";
import { api } from "../../lib/api";
import { messagesFor } from "../../lib/i18n";
import { StatisticsPane } from "./StatisticsPane";

const historyEvents = vi.hoisted(() => ({ listen: vi.fn(), stop: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: historyEvents.listen }));
vi.mock("../../lib/api", () => ({ api: { usageStatistics: vi.fn() } }));
vi.mock("recharts", async () => {
  const { createElement, Fragment } = await import("react");
  const wrapper = (name: string) =>
    function MockChartPrimitive({
      children,
      data,
      dataKey,
    }: {
      children?: ReactNode;
      data?: unknown[];
      dataKey?: string;
    }) {
      return createElement(
        "div",
        {
          "data-chart-component": name,
          "data-chart-data-key": dataKey,
          "data-chart-point-count": data?.length,
        },
        createElement(Fragment, null, children),
      );
    };
  return {
    Bar: wrapper("Bar"),
    BarChart: wrapper("BarChart"),
    CartesianGrid: () => null,
    Legend: () => null,
    ResponsiveContainer: wrapper("ResponsiveContainer"),
    Tooltip: () => null,
    XAxis: wrapper("XAxis"),
    YAxis: wrapper("YAxis"),
  };
});

const sample = {
  dictations: 1_234,
  completed: 1,
  failed: 1,
  interrupted: 99,
  apiRequests: 3,
  reportedCostUsd: 0.125,
  unpricedAttempts: 3,
  audioDurationMs: 125_000,
  daily: [{ date: "2026-10-02", dictations: 1_234, apiRequests: 3, reportedCostUsd: 0.125 }],
  models: [
    {
      model: "vendor/model-a",
      dictations: 2,
      apiRequests: 3,
      reportedCostUsd: 0.125,
      unpricedAttempts: 1,
    },
    {
      model: "vendor/model-b",
      dictations: 1,
      apiRequests: 2,
      reportedCostUsd: 0,
      unpricedAttempts: 2,
    },
  ],
};

function localDateOffset(days: number) {
  const date = new Date();
  date.setHours(0, 0, 0, 0);
  date.setDate(date.getDate() + days);
  return date;
}

function inputDate(date: Date) {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

afterEach(cleanup);
beforeEach(() => {
  vi.mocked(api.usageStatistics).mockReset();
  historyEvents.listen.mockReset().mockResolvedValue(historyEvents.stop);
  historyEvents.stop.mockReset();
});

describe("StatisticsPane", () => {
  it("shows locale-formatted aggregates, partial model costs and accessible daily values", async () => {
    vi.mocked(api.usageStatistics).mockResolvedValue(sample);
    const copy = messagesFor("ru");
    render(<StatisticsPane copy={copy} />);

    expect(await screen.findByText("vendor/model-a")).toBeInTheDocument();
    const formattedDictations = new Intl.NumberFormat(copy.dateLocale).format(1_234);
    const normalizeLocaleNumber = (value: string) => value.replace(/\s/gu, "");
    expect(
      screen.getAllByText(
        (text) => normalizeLocaleNumber(text) === normalizeLocaleNumber(formattedDictations),
      ),
    ).toHaveLength(2);
    const formattedCost = new Intl.NumberFormat(copy.dateLocale, {
      style: "currency",
      currency: "USD",
      currencyDisplay: "code",
      minimumFractionDigits: 6,
      maximumFractionDigits: 6,
    }).format(0.125);
    expect(
      screen.getAllByText(
        (text) => normalizeLocaleNumber(text) === normalizeLocaleNumber(formattedCost),
      ),
    ).toHaveLength(2);
    const requestsSummary = screen
      .getAllByText(copy.statisticsRequests)
      .find((node) => node.tagName === "P")
      ?.closest('[data-slot="card"]') as HTMLElement;
    expect(within(requestsSummary).getByText("3")).toBeInTheDocument();
    const audioSummary = screen
      .getByText(copy.statisticsAudio)
      .closest('[data-slot="card"]') as HTMLElement;
    expect(within(audioSummary).getByText(`2 ${copy.durationMinutes}`)).toBeInTheDocument();
    expect(screen.queryByText("Успешность")).not.toBeInTheDocument();
    expect(screen.queryByText("Success rate")).not.toBeInTheDocument();
    expect(screen.getByText(copy.statisticsIncomplete.replace("{count}", "3"))).toBeInTheDocument();
    expect(screen.getByText("Попыток без подтвержденной стоимости: 1")).toBeInTheDocument();
    expect(screen.getByText("Попыток без подтвержденной стоимости: 2")).toBeInTheDocument();
    expect(screen.getByText("Ось Y: Диктовок в день")).toBeInTheDocument();
    expect(screen.getByRole("img").querySelector('[data-chart-component="Bar"]')).toHaveAttribute(
      "data-chart-data-key",
      "dictations",
    );

    fireEvent.click(screen.getByText(copy.statisticsChartData));
    const dailyTable = screen.getByRole("table", { name: /Диктовки/ });
    expect(
      within(dailyTable).getByRole("columnheader", { name: copy.statisticsChartDate }),
    ).toBeInTheDocument();
    expect(
      within(dailyTable).getByText(
        (text) => normalizeLocaleNumber(text) === normalizeLocaleNumber(formattedDictations),
      ),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: copy.statisticsMetricRequests }));
    expect(screen.getByRole("img", { name: /API-запросы/ })).toBeInTheDocument();
    expect(screen.getByText("Ось Y: API-запросов в день")).toBeInTheDocument();
    expect(screen.getByRole("img").querySelector('[data-chart-component="Bar"]')).toHaveAttribute(
      "data-chart-data-key",
      "apiRequests",
    );
    expect(screen.getByRole("table", { name: /API-запросы/ })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: copy.statisticsMetricCost }));
    expect(screen.getByRole("img", { name: /USD в день/ })).toBeInTheDocument();
    expect(screen.getByText("Ось Y: USD в день")).toBeInTheDocument();
    expect(screen.getByRole("img").querySelector('[data-chart-component="Bar"]')).toHaveAttribute(
      "data-chart-data-key",
      "reportedCostUsd",
    );
    expect(screen.getByRole("table", { name: /Расход/ })).toBeInTheDocument();
  });

  it("sends exact local-day bounds for presets and custom date ranges", async () => {
    vi.mocked(api.usageStatistics).mockResolvedValue(sample);
    const copy = messagesFor("en");
    render(<StatisticsPane copy={copy} />);

    const start30 = localDateOffset(-29).toISOString();
    const tomorrow = localDateOffset(1).toISOString();
    await waitFor(() => expect(api.usageStatistics).toHaveBeenLastCalledWith(start30, tomorrow));

    fireEvent.click(screen.getByRole("button", { name: "7 days" }));
    await waitFor(() =>
      expect(api.usageStatistics).toHaveBeenLastCalledWith(
        localDateOffset(-6).toISOString(),
        tomorrow,
      ),
    );

    fireEvent.click(screen.getByRole("button", { name: "Custom range" }));
    const fromDate = localDateOffset(-8);
    const toDate = localDateOffset(-5);
    fireEvent.change(screen.getByLabelText("From"), { target: { value: inputDate(fromDate) } });
    fireEvent.change(screen.getByLabelText("To"), { target: { value: inputDate(toDate) } });
    await waitFor(() =>
      expect(api.usageStatistics).toHaveBeenLastCalledWith(
        fromDate.toISOString(),
        localDateOffset(-4).toISOString(),
      ),
    );

    const ninetyDaysAgo = localDateOffset(-89);
    const today = localDateOffset(0);
    fireEvent.change(screen.getByLabelText("From"), {
      target: { value: inputDate(ninetyDaysAgo) },
    });
    fireEvent.change(screen.getByLabelText("To"), { target: { value: inputDate(today) } });
    await waitFor(() =>
      expect(api.usageStatistics).toHaveBeenLastCalledWith(
        ninetyDaysAgo.toISOString(),
        localDateOffset(1).toISOString(),
      ),
    );

    const callsAfterMaximumRange = vi.mocked(api.usageStatistics).mock.calls.length;
    fireEvent.change(screen.getByLabelText("From"), {
      target: { value: inputDate(localDateOffset(-90)) },
    });
    await waitFor(() =>
      expect(screen.getAllByText(messagesFor("en").statisticsChooseValidRange)).not.toHaveLength(0),
    );
    expect(api.usageStatistics).toHaveBeenCalledTimes(callsAfterMaximumRange);
    expect(screen.getByText(/Choose a range up to 90 days/)).toBeInTheDocument();
  });

  it("keeps the current dashboard in place while a newly selected timeframe loads", async () => {
    const nextPeriod = deferred<typeof sample>();
    vi.mocked(api.usageStatistics)
      .mockResolvedValueOnce(sample)
      .mockReturnValueOnce(nextPeriod.promise);
    const copy = messagesFor("en");
    render(<StatisticsPane copy={copy} />);

    const formattedDay = (date: Date, withYear = false) =>
      new Intl.DateTimeFormat(copy.dateLocale, {
        day: "numeric",
        month: "short",
        ...(withYear ? { year: "numeric" } : {}),
      }).format(date);
    const initialRange = [
      formattedDay(localDateOffset(-29)),
      " – ",
      formattedDay(localDateOffset(0), true),
    ].join("");
    const nextRange = [
      formattedDay(localDateOffset(-6)),
      " – ",
      formattedDay(localDateOffset(0), true),
    ].join("");
    const existingModel = await screen.findByText("vendor/model-a");
    const dashboard = screen.getByRole("region", { name: copy.statisticsTitle });
    const chart = dashboard.querySelector('[data-slot="chart"]');
    expect(chart).not.toBeNull();
    expect(dashboard).toHaveAttribute("aria-busy", "false");
    expect(screen.getByText(initialRange)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "7 days" }));
    await waitFor(() => expect(api.usageStatistics).toHaveBeenCalledTimes(2));

    expect(dashboard).toHaveAttribute("aria-busy", "true");
    expect(dashboard.querySelector('[data-slot="chart"]')).toBe(chart);
    expect(screen.getByText("vendor/model-a")).toBe(existingModel);
    expect(screen.getByText(copy.statisticsLoading)).toHaveAttribute("role", "status");
    expect(screen.getByText(initialRange)).toBeInTheDocument();

    await act(async () => {
      nextPeriod.resolve({ ...sample, dictations: 222 });
      await nextPeriod.promise;
    });

    expect(screen.getByText("222", { selector: "p" })).toBeInTheDocument();
    expect(dashboard).toHaveAttribute("aria-busy", "false");
    expect(dashboard.querySelector('[data-slot="chart"]')).toBe(chart);
    expect(screen.queryByText(copy.statisticsLoading)).not.toBeInTheDocument();
    expect(screen.getByText(nextRange)).toBeInTheDocument();
    expect(screen.queryByText(initialRange)).not.toBeInTheDocument();
  });

  it("uses the actual local-day duration across a daylight-saving transition", async () => {
    const previousTimezone = process.env.TZ;
    process.env.TZ = "America/New_York";
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-08T12:00:00-04:00"));
    try {
      vi.mocked(api.usageStatistics).mockResolvedValue(sample);
      render(<StatisticsPane copy={messagesFor("en")} />);
      fireEvent.click(screen.getByRole("button", { name: "Custom range" }));
      fireEvent.change(screen.getByLabelText("From"), { target: { value: "2026-03-08" } });
      fireEvent.change(screen.getByLabelText("To"), { target: { value: "2026-03-08" } });
      await vi.waitFor(() =>
        expect(api.usageStatistics).toHaveBeenLastCalledWith(
          "2026-03-08T05:00:00.000Z",
          "2026-03-09T04:00:00.000Z",
        ),
      );
    } finally {
      cleanup();
      vi.useRealTimers();
      if (previousTimezone === undefined) delete process.env.TZ;
      else process.env.TZ = previousTimezone;
    }
  });

  it("stops the history-change subscription when unmounted", async () => {
    vi.mocked(api.usageStatistics).mockResolvedValue(sample);
    const { unmount } = render(<StatisticsPane copy={messagesFor("en")} />);
    await waitFor(() => expect(historyEvents.listen).toHaveBeenCalledOnce());
    unmount();
    expect(historyEvents.stop).toHaveBeenCalledOnce();
  });

  it("stops a history subscription that resolves after unmount", async () => {
    const subscription = deferred<() => void>();
    historyEvents.listen.mockReturnValueOnce(subscription.promise);
    vi.mocked(api.usageStatistics).mockResolvedValue(sample);
    const { unmount } = render(<StatisticsPane copy={messagesFor("en")} />);
    expect(historyEvents.listen).toHaveBeenCalledOnce();
    unmount();
    subscription.resolve(historyEvents.stop);
    await waitFor(() => expect(historyEvents.stop).toHaveBeenCalledOnce());
  });

  it("refreshes the aggregates after history changes", async () => {
    vi.mocked(api.usageStatistics).mockResolvedValue(sample);
    render(<StatisticsPane copy={messagesFor("en")} />);
    await waitFor(() => expect(historyEvents.listen).toHaveBeenCalledOnce());
    const [, refresh] = historyEvents.listen.mock.calls[0] as [string, () => void];
    vi.mocked(api.usageStatistics).mockClear();

    refresh();

    await waitFor(() => expect(api.usageStatistics).toHaveBeenCalledOnce());
  });

  it("keeps the newest data when overlapping history refreshes resolve out of order", async () => {
    const initial = deferred<typeof sample>();
    const refreshed = deferred<typeof sample>();
    vi.mocked(api.usageStatistics)
      .mockReturnValueOnce(initial.promise)
      .mockReturnValueOnce(refreshed.promise);
    render(<StatisticsPane copy={messagesFor("en")} />);
    await waitFor(() => expect(historyEvents.listen).toHaveBeenCalledOnce());
    const [, refresh] = historyEvents.listen.mock.calls[0] as [string, () => void];
    refresh();
    await waitFor(() => expect(api.usageStatistics).toHaveBeenCalledTimes(2));

    refreshed.resolve({ ...sample, dictations: 222 });
    expect(await screen.findByText("222", { selector: "p" })).toBeInTheDocument();
    await act(async () => {
      initial.resolve(sample);
      await initial.promise;
    });
    expect(screen.getByText("222", { selector: "p" })).toBeInTheDocument();
  });

  it("does not show an error from an older refresh after the latest one succeeded", async () => {
    const initial = deferred<typeof sample>();
    const refreshed = deferred<typeof sample>();
    vi.mocked(api.usageStatistics)
      .mockReturnValueOnce(initial.promise)
      .mockReturnValueOnce(refreshed.promise);
    render(<StatisticsPane copy={messagesFor("en")} />);
    await waitFor(() => expect(historyEvents.listen).toHaveBeenCalledOnce());
    const [, refresh] = historyEvents.listen.mock.calls[0] as [string, () => void];
    refresh();
    await waitFor(() => expect(api.usageStatistics).toHaveBeenCalledTimes(2));

    refreshed.resolve(sample);
    expect(await screen.findByText("vendor/model-a")).toBeInTheDocument();
    await act(async () => {
      initial.reject(new Error("stale statistics failure"));
      await initial.promise.catch(() => undefined);
    });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByText("Success rate")).not.toBeInTheDocument();
  });

  it("shows the metric-specific empty state and handles an API error", async () => {
    vi.mocked(api.usageStatistics).mockResolvedValue({
      ...sample,
      dictations: 0,
      completed: 0,
      failed: 0,
      apiRequests: 0,
      reportedCostUsd: 0,
      unpricedAttempts: 0,
      daily: [{ date: "2026-10-02", dictations: 0, apiRequests: 0, reportedCostUsd: 0 }],
      models: [],
    });
    const copy = messagesFor("en");
    render(<StatisticsPane copy={copy} />);
    expect(
      await screen.findByText("No values for this metric in the selected period."),
    ).toBeInTheDocument();
    expect(screen.queryByText("Success rate")).not.toBeInTheDocument();
    fireEvent.click(screen.getByText("Show daily values"));
    expect(screen.getByRole("table", { name: /Dictations/ })).toBeInTheDocument();

    vi.mocked(api.usageStatistics).mockRejectedValueOnce(new Error("statistics unavailable"));
    fireEvent.click(screen.getByRole("button", { name: "7 days" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.getByText(copy.statisticsMetricEmpty)).toBeInTheDocument();
  });

  it("keeps the previous populated snapshot and range when a timeframe request fails", async () => {
    const failedPeriod = deferred<typeof sample>();
    vi.mocked(api.usageStatistics)
      .mockResolvedValueOnce(sample)
      .mockReturnValueOnce(failedPeriod.promise);
    const copy = messagesFor("en");
    render(<StatisticsPane copy={copy} />);

    const formatDay = (date: Date, withYear = false) =>
      new Intl.DateTimeFormat(copy.dateLocale, {
        day: "numeric",
        month: "short",
        ...(withYear ? { year: "numeric" } : {}),
      }).format(date);
    const initialRange = [
      formatDay(localDateOffset(-29)),
      " – ",
      formatDay(localDateOffset(0), true),
    ].join("");

    expect(await screen.findByText("vendor/model-a")).toBeInTheDocument();
    expect(screen.getByText(initialRange)).toBeInTheDocument();
    const dictationSummary = screen
      .getAllByText(copy.statisticsDictations)
      .find((node) => node.tagName === "P")
      ?.closest('[data-slot="card"]') as HTMLElement;
    expect(
      within(dictationSummary).getByText(
        new Intl.NumberFormat(copy.dateLocale).format(sample.dictations),
      ),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "7 days" }));
    await waitFor(() => expect(api.usageStatistics).toHaveBeenCalledTimes(2));
    await act(async () => {
      failedPeriod.reject(new Error("statistics unavailable"));
      await failedPeriod.promise.catch(() => undefined);
    });

    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.getByText("vendor/model-a")).toBeInTheDocument();
    expect(screen.getByText(initialRange)).toBeInTheDocument();
    expect(
      within(dictationSummary).getByText(
        new Intl.NumberFormat(copy.dateLocale).format(sample.dictations),
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("region", { name: copy.statisticsTitle })).toHaveAttribute(
      "aria-busy",
      "false",
    );
  });
});
