// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

import { RefreshCw, Users } from "lucide-react";
import { getSyncPeers } from "../lib/api.js";
import { usePolling } from "../hooks/usePolling.js";
import { ErrorBox, PageHeader, Spinner, StatusDot } from "../components/ui.jsx";
import { formatDate, formatNumber, formatRelative } from "../lib/format.js";
import { useT } from "../lang/index.jsx";

const STALE_MS = 6 * 60 * 60 * 1000;

/// fileTime (100 ns ticks since 1601) → ISO string, null when unknown.
export function fileTimeToIso(ft) {
  if (!ft || ft <= 0) return null;
  const ms = Number(BigInt(ft) / 10000n) - 11644473600000;
  return Number.isFinite(ms) && ms > 0 ? new Date(ms).toISOString() : null;
}

export function ClientsPage() {
  const t = useT();
  // `now` is stamped when the data arrives (render must stay pure).
  const { data, error, loading, reload } = usePolling(() => getSyncPeers().then((d) => ({ ...d, now: Date.now() })), 30_000);
  const peers = data?.peers || [];
  const now = data?.now || 0;

  return (
    <>
      <PageHeader
        title={t("nav_clients")}
        description={t("clients_desc")}
        actions={
          <button type="button" className="btn btn-sm" onClick={reload}>
            <RefreshCw className="size-4" aria-hidden="true" /> {t("refresh")}
          </button>
        }
      />
      <ErrorBox error={error} onRetry={reload} />
      {data && data.opensync === false ? (
        <p className="card mb-4 p-4 text-sm text-warn">{t("clients_opensync_off")}</p>
      ) : null}
      {loading && !data ? (
        <Spinner className="size-5" label={t("loading")} />
      ) : peers.length === 0 ? (
        <div className="card flex items-center gap-3 p-5 text-sm text-muted">
          <Users className="size-5" aria-hidden="true" /> {t("clients_empty")}
        </div>
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th scope="col">IP</th>
                <th scope="col">{t("clients_version")}</th>
                <th scope="col">{t("clients_last_seen")}</th>
                <th scope="col">{t("clients_cursor")}</th>
                <th scope="col">{t("clients_spidr")}</th>
                <th scope="col">{t("clients_check")}</th>
                <th scope="col" className="text-right">{t("clients_buckets")}</th>
                <th scope="col">{t("clients_health")}</th>
                <th scope="col" className="text-right">{t("clients_requests")}</th>
                <th scope="col">{t("clients_first_seen")}</th>
              </tr>
            </thead>
            <tbody>
              {peers.map((p) => {
                const seenMs = Date.parse(p.lastSeen);
                const stale = !Number.isFinite(seenMs) || now - seenMs > STALE_MS;
                const cursor = fileTimeToIso(p.lastCursor);
                return (
                  <tr key={p.ip}>
                    <td className="font-mono text-xs">
                      <span className="flex items-center gap-2">
                        <StatusDot tone={stale ? "muted" : "ok"} label="" />
                        {p.ip}
                      </span>
                    </td>
                    <td className="text-xs">{p.version || <span className="text-muted">{t("clients_version_unknown")}</span>}</td>
                    <td className="text-xs whitespace-nowrap" title={formatDate(p.lastSeen)}>{formatRelative(p.lastSeen)}</td>
                    <td className="text-xs whitespace-nowrap" title={cursor ? formatDate(cursor) : ""}>{cursor ? formatRelative(cursor) : "-"}</td>
                    <td className="text-xs whitespace-nowrap" title={p.lastSpidr ? formatDate(p.lastSpidr) : ""}>{p.lastSpidr ? formatRelative(p.lastSpidr) : "-"}</td>
                    <td className="text-xs whitespace-nowrap" title={p.lastCheck ? formatDate(p.lastCheck) : ""}>
                      {p.lastCheck ? formatRelative(p.lastCheck) : "-"}
                      {p.status?.check && Number(p.status.check.remaining) > 0 ? <span className="ml-1 text-warn">{t("clients_check_remaining", { n: formatNumber(p.status.check.remaining) })}</span> : null}
                    </td>
                    <td className="text-right text-xs tabular-nums">{p.status && Number.isFinite(Number(p.status.buckets)) ? formatNumber(p.status.buckets) : "-"}</td>
                    <td className="text-xs whitespace-nowrap">
                      {p.status ? (
                        <span title={p.statusAt ? formatDate(p.statusAt) : ""}>
                          <StatusDot
                            tone={Number(p.status.errors) > 0 ? "danger" : Number(p.status.issues) > 0 ? "warn" : "ok"}
                            label={Number(p.status.issues) > 0 ? t("clients_health_issues", { n: p.status.issues, errors: p.status.errors || 0 }) : t("clients_health_ok")}
                          />
                        </span>
                      ) : (
                        <span className="text-muted">{t("clients_health_unknown")}</span>
                      )}
                    </td>
                    <td className="text-right text-xs tabular-nums">{formatNumber(p.requests)}</td>
                    <td className="text-xs whitespace-nowrap text-muted" title={formatDate(p.firstSeen)}>{formatRelative(p.firstSeen)}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      <p className="mt-3 text-xs text-muted">{t("clients_hint")}</p>
    </>
  );
}
