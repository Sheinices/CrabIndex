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

export function JobList({ jobs, empty = "Нет активных задач", hint = true }) {
  if (!jobs?.length) {
    return (
      <div className="text-sm text-muted">
        <p>{empty}</p>
        {hint ? (
          <p className="mt-2 text-xs">
            Это нормально. Здесь видны только долгие задачи, которые идут прямо
            сейчас: полный обход трекера (ParseAll), составление карт задач
            (UpdateTasksParse), догрузка старых раздач. Они запускаются по
            расписанию, в основном ночью и утром по UTC, поэтому после установки
            или перезапуска данные появляются не сразу. Обычный парсинг каждые
            15 минут занимает секунды и сюда не попадает.
          </p>
        ) : null}
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
  const { data, error, loading } = usePolling(
    () => getWafOverview("60m"),
    10_000,
  );
  const t = data?.totals || {};
  const pct =
    t.requests > 0 ? Math.round((t.blocked / t.requests) * 1000) / 10 : 0;
  return (
    <section className="card p-5" aria-labelledby="ov-waf">
      <div className="mb-3 flex items-center justify-between">
        <h2 id="ov-waf" className="flex items-center gap-2 font-semibold">
          <Shield className="size-4 text-muted" aria-hidden="true" /> WAF
        </h2>
        <Link to="/waf" className="text-sm text-accent hover:underline">
          Подробнее
        </Link>
      </div>
      {loading && !data ? (
        <Spinner label="Загрузка…" />
      ) : error && !data ? (
        <p className="text-sm text-muted">
          Статистика недоступна: {error.message}
        </p>
      ) : data?.enabled === false ? (
        <p className="text-sm">
          <StatusDot tone="muted" label="WAF выключен" />{" "}
          <Link
            to="/settings?group=waf"
            className="text-accent hover:underline"
          >
            Включить
          </Link>
        </p>
      ) : (
        <dl className="grid grid-cols-2 gap-3">
          <div>
            <dt className="text-xs text-muted">Запросов за 60 мин</dt>
            <dd className="text-xl font-semibold tabular-nums">
              {formatNumber(t.requests)}
            </dd>
          </div>
          <div>
            <dt className="text-xs text-muted">Заблокировано</dt>
            <dd
              className={`text-xl font-semibold tabular-nums ${t.blocked ? "text-danger" : ""}`}
            >
              {formatNumber(t.blocked)}{" "}
              <span className="text-xs font-normal text-muted">{pct}%</span>
            </dd>
          </div>
        </dl>
      )}
    </section>
  );
}

export function OverviewPage() {
  const { data, error, loading, reload } = usePolling(
    () => getOverview(),
    10_000,
  );
  const o = data || {};
  const enabled = (o.trackers || []).filter((t) => t.enabled !== false).length;
  const sync = o.sync || {};

  return (
    <>
      <PageHeader
        title="Обзор"
        description="Обновляется каждые 10 секунд"
        actions={
          <button type="button" className="btn btn-sm" onClick={reload}>
            <RefreshCw className="size-4" aria-hidden="true" /> Обновить
          </button>
        }
      />
      <ErrorBox error={error} onRetry={reload} />
      {loading && !data ? (
        <Spinner className="size-5" label="Загрузка…" />
      ) : (
        <div className="space-y-6">
          <section
            aria-label="Показатели"
            className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-4"
          >
            <Stat
              icon={GitCommit}
              label="Версия"
              value={o.version || "-"}
              hint={[o.gitSha, o.buildDate && formatDate(o.buildDate)]
                .filter(Boolean)
                .join(" · ")}
            />
            <Stat
              icon={Clock}
              label="Аптайм"
              value={formatDuration(o.uptimeSeconds)}
              hint={o.listen ? `Слушает ${o.listen}` : undefined}
            />
            <Stat
              icon={Database}
              label="Раздачи"
              value={formatNumber(o.torrents)}
              hint={`${formatNumber(o.masterDbKeys)} ключей · fast ${formatNumber(o.fastDbKeys)}`}
            />
            <Stat
              icon={HardDrive}
              label="Обновление БД"
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
                  Фоновые задачи
                </h2>
                <Link
                  to="/jobs"
                  className="text-sm text-accent hover:underline"
                >
                  Все задачи
                </Link>
              </div>
              <JobList jobs={o.activeJobs} />
            </section>

            <div className="space-y-6">
              <WafCard />
              <section className="card p-5" aria-labelledby="ov-sync">
                <h2 id="ov-sync" className="mb-3 font-semibold">
                  Синхронизация
                </h2>
                <dl className="space-y-2 text-sm">
                  <div className="flex justify-between gap-3">
                    <dt className="text-muted">Статус</dt>
                    <dd>
                      <StatusDot
                        tone={sync.enabled ? "ok" : "muted"}
                        label={sync.enabled ? "включена" : "выключена"}
                      />
                    </dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="text-muted">Источник</dt>
                    <dd className="truncate font-mono text-xs">
                      {sync.syncapi || "-"}
                    </dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="text-muted">Последняя</dt>
                    <dd>{formatRelative(sync.lastsync)}</dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="text-muted">Трекеры</dt>
                    <dd>
                      {enabled} / {(o.trackers || []).length} включены
                    </dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="text-muted">Конфиг</dt>
                    <dd className="font-mono text-xs">
                      {o.config?.path || "-"}
                      {o.config?.format ? ` (${o.config.format})` : ""}
                    </dd>
                  </div>
                </dl>
              </section>

              <section className="card p-5" aria-labelledby="ov-links">
                <h2 id="ov-links" className="mb-3 font-semibold">
                  Быстрые ссылки
                </h2>
                <ul className="space-y-1 text-sm">
                  {[
                    { href: "/", label: "Открыть сайт", icon: Server },
                    { href: "/docs/", label: "Документация", icon: BookOpen },
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
