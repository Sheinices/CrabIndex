import { Link } from "react-router";
import {
  BookOpen,
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
import { getOverview, getWafOverview } from "../lib/api.js";
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
