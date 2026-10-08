import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Activity,
  AudioLines,
  ChartNoAxesCombined,
  Coins,
  LoaderCircle,
  RotateCcw,
} from "lucide-react";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";
import { PageHeader } from "../../components/settings/PageHeader";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardHeader } from "../../components/ui/card";
import { ChartContainer, ChartTooltip, ChartTooltipContent } from "../../components/ui/chart";
import { Input } from "../../components/ui/input";
import { api, type UsageStatistics } from "../../lib/api";
import { formatInvokeError, type Messages } from "../../lib/i18n";
import { HISTORY_CHANGED } from "../../lib/history-sync";

type Period = "7" | "30" | "90" | "custom";
type Metric = "dictations" | "requests" | "cost";
type StatisticsRange = {
  firstDay: string;
  lastDay: string;
  start: string;
  end: string;
};
type StatisticsSnapshot = {
  data: UsageStatistics;
  range: StatisticsRange;
};

function localDay(date: Date) {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function startOfLocalDay(value: string) {
  const [year, month, day] = value.split("-").map(Number);
  return new Date(year, month - 1, day);
}

function shiftDay(value: string, days: number) {
  const date = startOfLocalDay(value);
  date.setDate(date.getDate() + days);
  return localDay(date);
}

function inclusiveDaysBetween(from: string, to: string) {
  const [fromYear, fromMonth, fromDate] = from.split("-").map(Number);
  const [toYear, toMonth, toDate] = to.split("-").map(Number);
  const start = Date.UTC(fromYear, fromMonth - 1, fromDate);
  const end = Date.UTC(toYear, toMonth - 1, toDate);
  return Math.round((end - start) / 86_400_000) + 1;
}

function formatDay(value: string, locale: string, withYear = false) {
  const options: Intl.DateTimeFormatOptions = withYear
    ? { day: "numeric", month: "short", year: "numeric" }
    : { day: "numeric", month: "short" };
  return new Intl.DateTimeFormat(locale, options).format(startOfLocalDay(value));
}

function formatNumber(value: number, locale: string, fractionDigits?: number) {
  return new Intl.NumberFormat(locale, {
    minimumFractionDigits: fractionDigits,
    maximumFractionDigits: fractionDigits,
  }).format(value);
}

function formatCost(value: number, locale: string) {
  return new Intl.NumberFormat(locale, {
    style: "currency",
    currency: "USD",
    currencyDisplay: "code",
    minimumFractionDigits: 6,
    maximumFractionDigits: 6,
  }).format(value);
}

function formatDuration(milliseconds: number, copy: Messages, locale: string) {
  const number = (value: number) => formatNumber(value, locale);
  const seconds = Math.round(milliseconds / 1_000);
  if (seconds < 60) return `${number(seconds)} ${copy.durationSeconds}`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${number(minutes)} ${copy.durationMinutes}`;
  return `${number(Math.floor(minutes / 60))} ${copy.statisticsHours} ${number(minutes % 60)} ${copy.durationMinutes}`;
}

function metricLabel(metric: Metric, copy: Messages) {
  if (metric === "dictations") return copy.statisticsMetricDictations;
  if (metric === "requests") return copy.statisticsMetricRequests;
  return copy.statisticsMetricCost;
}

function metricUnit(metric: Metric, copy: Messages) {
  if (metric === "dictations") return copy.statisticsChartUnitDictations;
  if (metric === "requests") return copy.statisticsChartUnitRequests;
  return copy.statisticsChartUnitCost;
}

function chartValue(metric: Metric, point: UsageStatistics["daily"][number]) {
  if (metric === "dictations") return point.dictations;
  if (metric === "requests") return point.apiRequests;
  return point.reportedCostUsd;
}

function formatMetricValue(value: number, metric: Metric, locale: string) {
  return metric === "cost" ? formatCost(value, locale) : formatNumber(value, locale);
}

export function StatisticsPane({ copy }: { copy: Messages }) {
  const today = localDay(new Date());
  const [period, setPeriod] = useState<Period>("30");
  const [from, setFrom] = useState(shiftDay(today, -29));
  const [to, setTo] = useState(today);
  const [metric, setMetric] = useState<Metric>("dictations");
  const [snapshot, setSnapshot] = useState<StatisticsSnapshot | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dayCount = period === "custom" ? inclusiveDaysBetween(from, to) : Number(period);
  const customRangeValid = dayCount > 0 && dayCount <= 90 && from <= to && to <= today;

  const range = useMemo(() => {
    const days = period === "custom" ? 0 : Number(period);
    const firstDay = period === "custom" ? from : shiftDay(today, -(days - 1));
    const lastDay = period === "custom" ? to : today;
    return {
      firstDay,
      lastDay,
      start: startOfLocalDay(firstDay).toISOString(),
      end: startOfLocalDay(shiftDay(lastDay, 1)).toISOString(),
    };
  }, [period, from, to, today]);

  useEffect(() => {
    if (!customRangeValid) {
      setSnapshot(null);
      setIsLoading(false);
      setError(null);
      return;
    }

    let active = true;
    let latestRequest = 0;
    let unlisten: (() => void) | undefined;
    const refresh = () => {
      const request = ++latestRequest;
      setError(null);
      setIsLoading(true);
      void api
        .usageStatistics(range.start, range.end)
        .then((result) => {
          if (active && request === latestRequest) {
            setSnapshot({ data: result, range });
            setIsLoading(false);
          }
        })
        .catch((reason: unknown) => {
          if (active && request === latestRequest) {
            setError(formatInvokeError(reason, copy));
            setIsLoading(false);
          }
        });
    };

    refresh();
    void listen(HISTORY_CHANGED, refresh)
      .then((stop) => {
        if (active) unlisten = stop;
        else stop();
      })
      .catch(() => undefined);
    return () => {
      active = false;
      unlisten?.();
    };
  }, [range, copy, customRangeValid]);

  const data = snapshot?.data ?? null;
  const points = data?.daily ?? [];
  const activeMetricLabel = metricLabel(metric, copy);
  const requestedRangeDescription = customRangeValid
    ? `${formatDay(range.firstDay, copy.dateLocale)} – ${formatDay(range.lastDay, copy.dateLocale, true)}`
    : copy.statisticsChooseValidRange;
  const rangeDescription = !customRangeValid
    ? copy.statisticsChooseValidRange
    : snapshot
      ? `${formatDay(snapshot.range.firstDay, copy.dateLocale)} – ${formatDay(snapshot.range.lastDay, copy.dateLocale, true)}`
      : requestedRangeDescription;
  const isRefreshing = isLoading && data !== null;
  const chartHasValues = points.some((point) => chartValue(metric, point) > 0);
  const maxDailyCost = Math.max(0, ...points.map((point) => point.reportedCostUsd));
  const costAxisDigits = maxDailyCost >= 0.01 ? 2 : maxDailyCost >= 0.0001 ? 4 : 6;
  const chartConfig = {
    value: { label: activeMetricLabel, color: "var(--primary)" },
  };
  const tickInterval = Math.max(0, Math.ceil(points.length / 7) - 1);

  return (
    <section
      className="mx-auto w-full max-w-6xl space-y-5"
      aria-label={copy.statisticsTitle}
      aria-busy={isLoading}
    >
      <PageHeader
        icon={ChartNoAxesCombined}
        title={copy.statisticsTitle}
        description={
          <span className="flex min-h-5 flex-wrap items-center gap-x-2">
            <span>{rangeDescription}</span>
            <span className="inline-flex min-h-5 min-w-36 items-center gap-1.5 text-xs">
              {isRefreshing ? (
                <>
                  <LoaderCircle className="size-3.5 animate-spin" aria-hidden="true" />
                  <span role="status">{copy.statisticsLoading}</span>
                </>
              ) : null}
            </span>
          </span>
        }
        actions={
          <div
            className="flex rounded-xl border border-border bg-muted/60 p-1"
            role="group"
            aria-label={copy.statisticsPeriod}
          >
            {(["7", "30", "90", "custom"] as Period[]).map((item) => {
              const label =
                item === "7"
                  ? copy.statisticsDays7
                  : item === "30"
                    ? copy.statisticsDays30
                    : item === "90"
                      ? copy.statisticsDays90
                      : copy.statisticsCustom;
              return (
                <Button
                  key={item}
                  type="button"
                  size="sm"
                  variant={period === item ? "default" : "ghost"}
                  aria-pressed={period === item}
                  onClick={() => setPeriod(item)}
                >
                  {label}
                </Button>
              );
            })}
          </div>
        }
      />

      {period === "custom" ? (
        <div className="flex flex-wrap items-end gap-3 rounded-xl border border-border bg-card p-4">
          <label className="grid min-w-40 gap-1.5 text-xs font-medium text-muted-foreground">
            {copy.dateFrom}
            <Input
              type="date"
              value={from}
              min={shiftDay(today, -89)}
              max={to}
              aria-label={copy.dateFrom}
              onChange={(event) => {
                if (event.target.value) setFrom(event.target.value);
              }}
            />
          </label>
          <label className="grid min-w-40 gap-1.5 text-xs font-medium text-muted-foreground">
            {copy.dateTo}
            <Input
              type="date"
              value={to}
              min={from}
              max={today}
              aria-label={copy.dateTo}
              onChange={(event) => {
                if (event.target.value) setTo(event.target.value);
              }}
            />
          </label>
          <p className="pb-1 text-xs text-muted-foreground">{copy.statisticsMaxRange}</p>
        </div>
      ) : null}

      {error ? (
        <div
          className="rounded-xl border border-destructive/30 bg-destructive/5 p-4 text-sm"
          role="alert"
        >
          {error}
        </div>
      ) : null}

      {!customRangeValid ? (
        <div className="rounded-xl border border-border bg-card p-6 text-sm text-muted-foreground">
          {copy.statisticsChooseValidRange}
        </div>
      ) : !data && isLoading ? (
        <div
          className="rounded-xl border border-border bg-card p-6 text-sm text-muted-foreground"
          aria-live="polite"
        >
          {copy.statisticsLoading}
        </div>
      ) : data ? (
        <>
          <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
            <Summary
              icon={Activity}
              label={copy.statisticsDictations}
              value={formatNumber(data.dictations, copy.dateLocale)}
            />
            <Summary
              icon={RotateCcw}
              label={copy.statisticsRequests}
              hint={copy.statisticsRequestsHint}
              value={formatNumber(data.apiRequests, copy.dateLocale)}
            />
            <Summary
              icon={Coins}
              label={copy.statisticsCost}
              hint={copy.statisticsCostHint}
              value={formatCost(data.reportedCostUsd, copy.dateLocale)}
            />
            <Summary
              icon={AudioLines}
              label={copy.statisticsAudio}
              value={formatDuration(data.audioDurationMs, copy, copy.dateLocale)}
            />
          </div>

          {data.unpricedAttempts > 0 ? (
            <p
              className="rounded-lg border border-amber-500/20 bg-amber-500/5 px-3 py-2 text-sm text-muted-foreground"
              role="status"
            >
              {copy.statisticsIncomplete.replace(
                "{count}",
                formatNumber(data.unpricedAttempts, copy.dateLocale),
              )}
            </p>
          ) : null}

          <Card className="gap-0 overflow-visible py-0" aria-labelledby="usage-chart-title">
            <CardHeader className="flex flex-row flex-wrap items-start justify-between gap-4 border-b border-border px-5 py-4 sm:px-6">
              <div>
                <h2 id="usage-chart-title" className="text-sm font-semibold tracking-tight">
                  {copy.statisticsDailyUsage}
                </h2>
                <p className="mt-1 text-xs text-muted-foreground">
                  {copy.statisticsYAxisLabel.replace("{unit}", metricUnit(metric, copy))}
                </p>
              </div>
              <div
                className="flex flex-wrap gap-1 rounded-xl border border-border bg-muted/60 p-1"
                role="group"
                aria-label={copy.statisticsChartMetric}
              >
                {(["dictations", "requests", "cost"] as Metric[]).map((item) => (
                  <Button
                    key={item}
                    type="button"
                    size="sm"
                    variant={metric === item ? "secondary" : "ghost"}
                    aria-pressed={metric === item}
                    onClick={() => setMetric(item)}
                  >
                    {metricLabel(item, copy)}
                  </Button>
                ))}
              </div>
            </CardHeader>

            {chartHasValues ? (
              <CardContent className="px-3 pb-4 pt-5 sm:px-5">
                <ChartContainer
                  config={chartConfig}
                  className="h-64 w-full aspect-auto sm:h-72"
                  role="img"
                  aria-label={`${activeMetricLabel}, ${metricUnit(metric, copy)}, ${rangeDescription}`}
                >
                  <BarChart data={points} margin={{ top: 8, right: 12, left: 4, bottom: 0 }}>
                    <CartesianGrid vertical={false} strokeDasharray="3 3" />
                    <XAxis
                      dataKey="date"
                      tickLine={false}
                      axisLine={false}
                      tickMargin={10}
                      interval={tickInterval}
                      tickFormatter={(value: string) => formatDay(value, copy.dateLocale)}
                    />
                    <YAxis
                      width={metric === "cost" ? 76 : 56}
                      tickLine={false}
                      axisLine={false}
                      tickMargin={8}
                      allowDecimals={metric === "cost"}
                      domain={[0, "auto"]}
                      tickFormatter={(value: number) =>
                        metric === "cost"
                          ? formatNumber(value, copy.dateLocale, costAxisDigits)
                          : formatNumber(value, copy.dateLocale)
                      }
                    />
                    <ChartTooltip
                      cursor={{ fill: "var(--muted)", opacity: 0.45 }}
                      content={
                        <ChartTooltipContent
                          labelFormatter={(value) =>
                            formatDay(String(value), copy.dateLocale, true)
                          }
                          formatter={(value) => (
                            <span className="font-mono tabular-nums">
                              {formatMetricValue(Number(value), metric, copy.dateLocale)}
                            </span>
                          )}
                        />
                      }
                    />
                    <Bar
                      dataKey={
                        metric === "dictations"
                          ? "dictations"
                          : metric === "requests"
                            ? "apiRequests"
                            : "reportedCostUsd"
                      }
                      fill="var(--color-value)"
                      radius={[5, 5, 0, 0]}
                      maxBarSize={30}
                      isAnimationActive={false}
                    />
                  </BarChart>
                </ChartContainer>
              </CardContent>
            ) : (
              <CardContent className="grid min-h-64 place-items-center px-6 text-center text-sm text-muted-foreground">
                {copy.statisticsMetricEmpty}
              </CardContent>
            )}
            <details className="border-t border-border px-5 py-3 sm:px-6">
              <summary className="cursor-pointer text-sm font-medium text-muted-foreground outline-none focus-visible:ring-3 focus-visible:ring-ring/50">
                {copy.statisticsChartData}
              </summary>
              <div className="mt-3 max-h-72 overflow-auto">
                <table
                  className="w-full border-collapse text-left text-sm"
                  aria-label={`${activeMetricLabel} – ${rangeDescription}`}
                >
                  <thead className="sticky top-0 bg-card text-xs text-muted-foreground">
                    <tr>
                      <th scope="col" className="px-2 py-2 font-medium">
                        {copy.statisticsChartDate}
                      </th>
                      <th scope="col" className="px-2 py-2 text-right font-medium">
                        {copy.statisticsChartValue}
                      </th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border">
                    {points.map((point) => (
                      <tr key={point.date}>
                        <th scope="row" className="px-2 py-2 font-normal">
                          {formatDay(point.date, copy.dateLocale, true)}
                        </th>
                        <td className="px-2 py-2 text-right font-mono tabular-nums">
                          {formatMetricValue(chartValue(metric, point), metric, copy.dateLocale)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </details>
          </Card>

          <Card className="gap-0 py-0" aria-labelledby="usage-models-title">
            <CardHeader className="border-b border-border px-5 py-4 sm:px-6">
              <h2 id="usage-models-title" className="text-sm font-semibold tracking-tight">
                {copy.statisticsModels}
              </h2>
              <p className="mt-1 text-xs text-muted-foreground">{copy.statisticsModelCostHint}</p>
            </CardHeader>
            {data.models.length ? (
              <CardContent className="overflow-x-auto px-0">
                <table className="w-full min-w-[38rem] border-collapse text-left text-sm">
                  <thead className="bg-muted/40 text-xs text-muted-foreground">
                    <tr>
                      <th className="px-5 py-3 font-medium sm:px-6">{copy.statisticsModel}</th>
                      <th className="px-4 py-3 text-right font-medium">
                        {copy.statisticsModelDictations}
                      </th>
                      <th className="px-4 py-3 text-right font-medium">
                        {copy.statisticsModelRequests}
                      </th>
                      <th className="px-5 py-3 text-right font-medium sm:px-6">
                        {copy.statisticsModelCost}
                      </th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border">
                    {data.models.map((item) => (
                      <tr key={item.model} className="transition-colors hover:bg-muted/30">
                        <th
                          scope="row"
                          className="max-w-72 truncate px-5 py-3.5 font-medium sm:px-6"
                          title={item.model}
                        >
                          {item.model}
                        </th>
                        <td className="px-4 py-3.5 text-right tabular-nums">
                          {formatNumber(item.dictations, copy.dateLocale)}
                        </td>
                        <td className="px-4 py-3.5 text-right tabular-nums">
                          {formatNumber(item.apiRequests, copy.dateLocale)}
                        </td>
                        <td className="px-5 py-3.5 text-right font-mono text-xs tabular-nums sm:px-6">
                          <div>{formatCost(item.reportedCostUsd, copy.dateLocale)}</div>
                          {item.unpricedAttempts > 0 ? (
                            <span className="mt-1 block text-xs font-sans text-amber-700 dark:text-amber-300">
                              {copy.statisticsModelPartialCost.replace(
                                "{count}",
                                formatNumber(item.unpricedAttempts, copy.dateLocale),
                              )}
                            </span>
                          ) : null}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </CardContent>
            ) : (
              <CardContent className="px-5 py-6 text-sm text-muted-foreground sm:px-6">
                {copy.statisticsEmpty}
              </CardContent>
            )}
          </Card>
        </>
      ) : null}
    </section>
  );
}

function Summary({
  icon: Icon,
  label,
  value,
  hint,
}: {
  icon: typeof Activity;
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <Card size="sm" className="min-w-0 gap-0 py-3.5">
      <CardContent className="px-4">
        <div className="flex items-center gap-2 text-muted-foreground">
          <Icon className="size-4 shrink-0 text-primary" aria-hidden="true" />
          <p className="truncate text-xs font-medium">{label}</p>
        </div>
        <p className="mt-3 truncate text-xl font-semibold tracking-tight tabular-nums">{value}</p>
        <p className="mt-1 min-h-8 text-xs leading-4 text-muted-foreground">{hint ?? " "}</p>
      </CardContent>
    </Card>
  );
}
