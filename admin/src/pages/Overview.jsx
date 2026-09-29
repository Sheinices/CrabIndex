import { useState } from "react";
import { Link } from "react-router";
import {
  AlertTriangle,
  BellRing,
  BookOpen,
  ChevronDown,
  ChevronRight,
  EyeOff,
  Braces,
  Clock,
  Database,
  ExternalLink,
  GitCommit,
  HardDrive,
  RefreshCw,
  Server,
  Shield,
} from "lucide-react";
import { getOverview, getWafOverview, muteIssue, sendTestNotification, startSyncCheck, unmuteIssue } from "../lib/api.js";
import { usePolling } from "../hooks/usePolling.js";
import {
  ErrorBox,
  PageHeader,
  ProgressBar,
  Spinner,
  StatusDot,
} from "../components/ui.jsx";
import {
  formatDate,
  formatDuration,
  formatNumber,
  formatRelative,
  jobPercent,
} from "../lib/format.js";
import { useT } from "../lang/index.jsx";

/// Result of the last integrity check against syncapi plus a "check now" button.
function SyncCheckRow({ sync, onDone }) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const c = sync.check;
  const running = sync.checkRunning || busy;
  const start = async () => {
    setBusy(true);
    try {
      await startSyncCheck();
    } catch {
      /* the status row shows the outcome */
    }
    setBusy(false);
    onDone?.();
  };
  let summary;
  if (running) summary = t("sync_check_running");
  else if (!c) summary = sync.checkMinutes > 0 ? t("sync_check_never") : t("sync_check_off");
  else if (c.ok === false) summary = t("sync_check_failed", { error: c.error || "" });
  else if (c.missing + c.mismatched + c.extra === 0) summary = t("sync_check_clean");
  else
    summary = t("sync_check_summary", {
      fetched: formatNumber(c.fetchedBuckets || 0),
      deleted: formatNumber(c.deletedBuckets || 0),
      remaining: formatNumber(c.remaining || 0),
    });
  return (
    <div>
      <div className="flex justify-between gap-3">
        <dt className="text-muted">{t("sync_check")}</dt>
        <dd className="text-right">
          {c?.at ? <span title={formatDate(c.at)}>{formatRelative(c.at)}</span> : "-"}
        </dd>
      </div>
      <div className="mt-1 flex items-start justify-between gap-3 text-xs">
        <p className={c && c.ok === false ? "text-danger" : "text-muted"}>{summary}</p>
        <button type="button" className="btn btn-sm shrink-0" onClick={start} disabled={running}>
          <RefreshCw className={`size-3.5 ${running ? "animate-spin" : ""}`} aria-hidden="true" /> {t("sync_check_now")}
        </button>
      </div>
    </div>
  );
}

const HEALTH_COLLAPSED_KEY = "crab.health.collapsed";

function readCollapsed() {
  try {
    return localStorage.getItem(HEALTH_COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

/// Health signals (see crabindex::health) with a link to the section that fixes them.
/// A signal can be hidden (kept on the server until it resolves); the list itself folds and the
/// state is remembered per browser.
function HealthCard({ issues, notifyConfigured, onChange }) {
  const t = useT();
  const [testState, setTestState] = useState(null);
  const [collapsed, setCollapsed] = useState(readCollapsed);
  const [showMuted, setShowMuted] = useState(false);
  const [busyUid, setBusyUid] = useState(null);
  const visible = issues.filter((i) => !i.muted);
  const muted = issues.filter((i) => i.muted);
  const toggleCollapsed = () => {
    const next = !collapsed;
    setCollapsed(next);
    try {
      localStorage.setItem(HEALTH_COLLAPSED_KEY, next ? "1" : "0");
    } catch {
      /* per-browser convenience only */
    }
  };
  const setMute = async (i, mute) => {
    const uid = i.key ? `${i.id}:${i.key}` : i.id;
    setBusyUid(uid);
    try {
      await (mute ? muteIssue(uid) : unmuteIssue(uid));
    } catch {
      /* the list refresh shows the actual state */
    }
    setBusyUid(null);
    onChange?.();
  };
  const sendTest = async () => {
    setTestState("busy");
    try {
      const r = await sendTestNotification();
      setTestState(r?.ok ? "ok" : (r?.errors || []).join("; ") || "error");
    } catch (e) {
      setTestState(String(e?.message || e));
    }
  };
  const testLabel =
    testState === "busy" ? t("health_test_sending") : testState === "ok" ? t("health_test_sent") : testState ? testState : null;
  const renderIssue = (i, isMuted) => {
    const error = i.severity === "error";
    const params = { ...(i.params || {}) };
    if (typeof params.minutes === "number") params.minutes = formatNumber(params.minutes);
    if (typeof params.remaining === "number") params.remaining = formatNumber(params.remaining);
    const uid = i.key ? `${i.id}:${i.key}` : i.id;
    return (
      <li
        key={uid}
        className={`flex flex-wrap items-start gap-3 rounded-xl border px-4 py-3 text-sm ${isMuted ? "border-border bg-surface opacity-70" : error ? "border-danger/40 bg-danger/10" : "border-warn/40 bg-warn/10"}`}
      >
        <AlertTriangle className={`mt-0.5 size-4 shrink-0 ${isMuted ? "text-muted" : error ? "text-danger" : "text-warn"}`} aria-hidden="true" />
        <div className="min-w-0 flex-1">
          <p className="font-medium">{t(`issue_${i.id}_title`, params)}</p>
          <p className="text-muted">{t(`issue_${i.id}_text`, params)}</p>
        </div>
        <div className="flex shrink-0 gap-1.5">
          {i.link ? (
            <Link to={i.link} className="btn btn-sm">
              {t("issue_open")}
            </Link>
          ) : null}
          <button type="button" className="btn btn-sm btn-ghost" onClick={() => setMute(i, !isMuted)} disabled={busyUid === uid} title={isMuted ? t("issue_unmute_hint") : t("issue_mute_hint")}>
            <EyeOff className="size-3.5" aria-hidden="true" /> {isMuted ? t("issue_unmute") : t("issue_mute")}
          </button>
        </div>
      </li>
    );
  };
  return (
    <section aria-labelledby="ov-health" className="mb-6">
      <div className="mb-2 flex flex-wrap items-center gap-3">
        <button type="button" className="flex items-center gap-1 font-semibold" onClick={toggleCollapsed} aria-expanded={!collapsed} aria-controls="ov-health-list">
          {collapsed ? <ChevronRight className="size-4" aria-hidden="true" /> : <ChevronDown className="size-4" aria-hidden="true" />}
          <span id="ov-health">{t("health_title")}</span>
        </button>
        {visible.length === 0 ? <StatusDot tone="ok" label={muted.length ? t("health_ok_muted", { muted: muted.length }) : t("health_ok")} /> : <StatusDot tone={visible.some((i) => i.severity === "error") ? "danger" : "warn"} label={t("health_count", { count: visible.length })} />}
        {muted.length ? (
          <button type="button" className="text-xs text-muted hover:text-fg" onClick={() => setShowMuted((v) => !v)}>
            {showMuted ? t("health_hide_muted") : t("health_show_muted", { count: muted.length })}
          </button>
        ) : null}
        {notifyConfigured ? (
          <button type="button" className="btn btn-sm btn-ghost ml-auto" onClick={sendTest} disabled={testState === "busy"}>
            <BellRing className="size-3.5" aria-hidden="true" /> {t("health_test_notification")}
          </button>
        ) : (
          <Link to="/settings" className="ml-auto text-xs text-muted hover:text-fg">
            {t("health_notify_setup")}
          </Link>
        )}
      </div>
      {testLabel ? <p className="mb-2 text-xs text-muted">{testLabel}</p> : null}
      {!collapsed && (visible.length || (showMuted && muted.length)) ? (
        <ul id="ov-health-list" className="space-y-2">
          {visible.map((i) => renderIssue(i, false))}
          {showMuted ? muted.map((i) => renderIssue(i, true)) : null}
        </ul>
      ) : null}
    </section>
  );
}

function Stat({ icon: Icon, label, value, hint }) {
  return (
    <div className="card p-4">
      <div className="flex items-center gap-2 text-xs font-medium text-muted">
        <Icon className="size-4" aria-hidden="true" />
        {label}
      </div>
      <p className="mt-2 text-2xl font-semibold tabular-nums">{value}</p>
      {hint ? <p className="mt-1 truncate text-xs text-muted">{hint}</p> : null}
    </div>
  );
}

export function JobList({ jobs, empty, hint = true }) {
  const t = useT();
  if (!jobs?.length) {
    return (
      <div className="text-sm text-muted">
        <p>{empty ?? t("jobs_empty")}</p>
        {hint ? <p className="mt-2 text-xs">{t("jobs_empty_hint")}</p> : null}
      </div>
    );
  }
  return (
    <ul className="space-y-4">
      {jobs.map((job) => {
        const pct = jobPercent(job);
        return (
          <li key={job.id || `${job.tracker}:${job.job}`}>
            <div className="mb-1.5 flex flex-wrap items-baseline justify-between gap-2 text-sm">
              <span className="font-medium">
                {job.tracker} <span className="text-muted">· {job.job}</span>
              </span>
              <span className="text-xs text-muted tabular-nums">
                {pct != null ? `${pct}% · ` : ""}
                {formatDuration(job.elapsedSeconds)}
              </span>
            </div>
            <ProgressBar value={pct} label={`${job.tracker} ${job.job}`} />
            {job.summary ? (
              <p className="mt-1 text-xs text-muted">{job.summary}</p>
            ) : null}
          </li>
        );
      })}
    </ul>
  );
}

/** WAF summary for the last 60 minutes (`waf/overview?window=60m`). */
export function WafCard() {
  const t = useT();
  const { data, error, loading } = usePolling(
    () => getWafOverview("60m"),
    10_000,
  );
  const totals = data?.totals || {};
  const pct =
    totals.requests > 0
      ? Math.round((totals.blocked / totals.requests) * 1000) / 10
      : 0;
  return (
    <section className="card p-5" aria-labelledby="ov-waf">
      <div className="mb-3 flex items-center justify-between">
        <h2 id="ov-waf" className="flex items-center gap-2 font-semibold">
          <Shield className="size-4 text-muted" aria-hidden="true" /> WAF
        </h2>
        <Link to="/waf" className="text-sm text-accent hover:underline">
          {t("more")}
        </Link>
      </div>
      {loading && !data ? (
        <Spinner label={t("loading")} />
      ) : error && !data ? (
        <p className="text-sm text-muted">
          {t("waf_stats_unavailable", { msg: error.message })}
        </p>
      ) : data?.enabled === false ? (
        <p className="text-sm">
          <StatusDot tone="muted" label={t("waf_disabled")} />{" "}
          <Link
            to="/settings?group=waf"
            className="text-accent hover:underline"
          >
            {t("enable")}
          </Link>
        </p>
      ) : (
        <dl className="grid grid-cols-2 gap-3">
          <div>
            <dt className="text-xs text-muted">{t("waf_requests_60m")}</dt>
            <dd className="text-xl font-semibold tabular-nums">
              {formatNumber(totals.requests)}
            </dd>
          </div>
          <div>
            <dt className="text-xs text-muted">{t("waf_blocked")}</dt>
            <dd
              className={`text-xl font-semibold tabular-nums ${totals.blocked ? "text-danger" : ""}`}
            >
              {formatNumber(totals.blocked)}{" "}
              <span className="text-xs font-normal text-muted">{pct}%</span>
            </dd>
          </div>
        </dl>
      )}
    </section>
  );
}

export function OverviewPage() {
  const t = useT();
  const { data, error, loading, reload } = usePolling(
    () => getOverview(),
    10_000,
  );
  const o = data || {};
  const enabled = (o.trackers || []).filter((t) => t.enabled !== false).length;
  const sync = o.sync || {};
  const syncLocal = sync.torrents;
  const syncRemote = sync.remoteTorrents;
  const syncPct =
    Number.isFinite(syncRemote) && syncRemote > 0 && Number.isFinite(syncLocal)
      ? Math.min(100, Math.round((syncLocal / syncRemote) * 100))
      : null;

  return (
    <>
      <PageHeader
        title={t("nav_overview")}
        description={t("ov_updates_every_10s")}
        actions={
          <button type="button" className="btn btn-sm" onClick={reload}>
            <RefreshCw className="size-4" aria-hidden="true" /> {t("refresh")}
          </button>
        }
      />
      <ErrorBox error={error} onRetry={reload} />
      {data ? <HealthCard issues={o.issues || []} notifyConfigured={!!o.notifyConfigured} onChange={reload} /> : null}
      {(o.hints || []).length ? (
        <div role="status" className="mb-6 space-y-2">
          {o.hints.map((h) => (
            <div
              key={h.id}
              className="flex flex-wrap items-start gap-3 rounded-xl border border-warn/40 bg-warn/10 px-4 py-3 text-sm"
            >
              <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warn" aria-hidden="true" />
              <div className="min-w-0 flex-1">
                <p className="font-medium">{t(`hint_${h.id}_title`, { minutes: h.minutes })}</p>
                <p className="text-muted">{t(`hint_${h.id}_text`, { minutes: h.minutes })}</p>
              </div>
              <Link to="/settings" className="btn btn-sm shrink-0">
                {t("hint_open_settings")}
              </Link>
            </div>
          ))}
        </div>
      ) : null}
      {loading && !data ? (
        <Spinner className="size-5" label={t("loading")} />
      ) : (
        <div className="space-y-6">
          <section
            aria-label={t("ov_metrics")}
            className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-4"
          >
            <Stat
              icon={GitCommit}
              label={t("ov_version")}
              value={o.version || "-"}
              hint={[o.gitSha, o.buildDate && formatDate(o.buildDate)]
                .filter(Boolean)
                .join(" · ")}
            />
            <Stat
              icon={Clock}
              label={t("ov_uptime")}
              value={formatDuration(o.uptimeSeconds)}
              hint={o.listen ? t("ov_listening", { addr: o.listen }) : undefined}
            />
            <Stat
              icon={Database}
              label={t("ov_torrents")}
              value={formatNumber(o.torrents)}
              hint={t("ov_torrents_hint", {
                keys: formatNumber(o.masterDbKeys),
                fast: formatNumber(o.fastDbKeys),
              })}
            />
            <Stat
              icon={HardDrive}
              label={t("ov_db_update")}
              value={formatRelative(o.lastUpdateDb)}
              hint={formatDate(o.lastUpdateDb)}
            />
          </section>

          <div className="grid gap-6 lg:grid-cols-3">
            <section
              className="card p-5 lg:col-span-2"
              aria-labelledby="ov-jobs"
            >
              <div className="mb-4 flex items-center justify-between">
                <h2 id="ov-jobs" className="font-semibold">
                  {t("ov_background_jobs")}
                </h2>
                <Link
                  to="/jobs"
                  className="text-sm text-accent hover:underline"
                >
                  {t("ov_all_jobs")}
                </Link>
              </div>
              <JobList jobs={o.activeJobs} />
            </section>

            <div className="space-y-6">
              <WafCard />
              <section className="card p-5" aria-labelledby="ov-sync">
                <h2 id="ov-sync" className="mb-3 font-semibold">
                  {t("sync_title")}
                </h2>
                <dl className="space-y-2 text-sm">
                  <div className="flex justify-between gap-3">
                    <dt className="text-muted">{t("sync_status")}</dt>
                    <dd>
                      <StatusDot
                        tone={sync.enabled ? "ok" : "muted"}
                        label={sync.enabled ? t("sync_on") : t("sync_off")}
                      />
                    </dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="text-muted">{t("sync_source")}</dt>
                    <dd className="truncate font-mono text-xs">
                      {sync.syncapi || "-"}
                    </dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="text-muted">{t("sync_last")}</dt>
                    <dd>{formatRelative(sync.lastsync)}</dd>
                  </div>
                  {sync.enabled ? (
                    <SyncCheckRow sync={sync} onDone={reload} />
                  ) : null}
                  {syncPct !== null && (
                    <div>
                      <div className="flex justify-between gap-3">
                        <dt className="text-muted">{t("sync_fill")}</dt>
                        <dd>
                          {formatNumber(syncLocal)} / {formatNumber(syncRemote)}{" "}
                          <span className="text-muted">({syncPct}%)</span>
                        </dd>
                      </div>
                      <div className="mt-1 h-1.5 overflow-hidden rounded bg-border">
                        <div
                          className="h-full rounded bg-accent transition-[width]"
                          style={{ width: `${syncPct}%` }}
                        />
                      </div>
                    </div>
                  )}
                  <div className="flex justify-between gap-3">
                    <dt className="text-muted">{t("nav_trackers")}</dt>
                    <dd>
                      {t("sync_trackers_enabled", {
                        enabled,
                        total: (o.trackers || []).length,
                      })}
                    </dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="text-muted">{t("sync_config")}</dt>
                    <dd className="font-mono text-xs">
                      {o.config?.path || "-"}
                      {o.config?.format ? ` (${o.config.format})` : ""}
                    </dd>
                  </div>
                </dl>
              </section>

              <section className="card p-5" aria-labelledby="ov-links">
                <h2 id="ov-links" className="mb-3 font-semibold">
                  {t("ov_quick_links")}
                </h2>
                <ul className="space-y-1 text-sm">
                  {[
                    { href: "/", label: t("open_site"), icon: Server },
                    { href: "/docs/", label: t("docs"), icon: BookOpen },
                    { href: "/swagger/", label: "Swagger", icon: Braces },
                  ].map(({ href, label, icon: Icon }) => (
                    <li key={href}>
                      <a
                        href={href}
                        target="_blank"
                        rel="noopener"
                        className="flex items-center gap-2 rounded-lg px-2 py-1.5 hover:bg-surface-2"
                      >
                        <Icon
                          className="size-4 text-muted"
                          aria-hidden="true"
                        />
                        {label}
                        <ExternalLink
                          className="ml-auto size-3.5 text-muted"
                          aria-hidden="true"
                        />
                      </a>
                    </li>
                  ))}
                </ul>
              </section>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
