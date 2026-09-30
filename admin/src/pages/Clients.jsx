// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

import { useState } from "react";
import { KeyRound, RefreshCw, Users } from "lucide-react";
import { createSyncKey, deleteSyncKey, enableSyncKey, getSyncKeys, getSyncPeers, revokeSyncKey } from "../lib/api.js";
import { usePolling } from "../hooks/usePolling.js";
import { useConfirm } from "../components/Confirm.jsx";
import { ErrorBox, PageHeader, Spinner, StatusDot } from "../components/ui.jsx";
import { formatDate, formatNumber, formatRelative } from "../lib/format.js";
import { useT } from "../lang/index.jsx";

const DEFAULT_STALE_HOURS = 6;

/// fileTime (100 ns ticks since 1601) → ISO string, null when unknown.
export function fileTimeToIso(ft) {
  if (!ft || ft <= 0) return null;
  const ms = Number(BigInt(ft) / 10000n) - 11644473600000;
  return Number.isFinite(ms) && ms > 0 ? new Date(ms).toISOString() : null;
}

/// Sync keys: a key names a client in the list and can be revoked; with syncRequireKey the
/// host serves /sync/* only to clients that send one. The secret is shown once, at creation.
function KeysSection() {
  const t = useT();
  const confirm = useConfirm();
  const { data, error, reload } = usePolling(() => getSyncKeys(), 60_000);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [created, setCreated] = useState(null);
  const [fail, setFail] = useState(null);
  const keys = data?.keys || [];
  const create = async (e) => {
    e.preventDefault();
    if (!name.trim()) return;
    setBusy(true);
    setFail(null);
    try {
      const r = await createSyncKey(name.trim());
      if (r?.ok) {
        setCreated(r);
        setName("");
      } else setFail(r?.error || "error");
    } catch (err) {
      setFail(String(err?.message || err));
    }
    setBusy(false);
    reload();
  };
  const act = async (fn, k, confirmKey) => {
    if (confirmKey) {
      const ok = await confirm({ title: t(confirmKey, { name: k.name }), message: <p>{t("keys_confirm_body")}</p>, danger: true, confirmLabel: t("execute") });
      if (!ok) return;
    }
    setBusy(true);
    try {
      await fn(k.name);
    } catch {
      /* the list refresh shows the state */
    }
    setBusy(false);
    reload();
  };
  return (
    <section className="card mt-6 p-5" aria-labelledby="clients-keys">
      <h2 id="clients-keys" className="mb-1 flex items-center gap-2 font-semibold">
        <KeyRound className="size-4 text-muted" aria-hidden="true" /> {t("keys_title")}
      </h2>
      <p className="mb-3 text-sm text-muted">{data?.requireKey ? t("keys_desc_required") : t("keys_desc_open")}</p>
      <ErrorBox error={error} onRetry={reload} />
      <form onSubmit={create} className="mb-3 flex flex-wrap items-center gap-2">
        <input className="input w-56" placeholder={t("keys_name_placeholder")} aria-label={t("keys_name")} value={name} onChange={(e) => setName(e.target.value)} maxLength={40} />
        <button type="submit" className="btn" disabled={busy || !name.trim()}>
          {t("keys_create")}
        </button>
        {fail ? <span className="text-sm text-danger">{fail}</span> : null}
      </form>
      {created ? (
        <div className="mb-3 rounded-xl border border-warn/40 bg-warn/10 p-3 text-sm">
          <p className="font-medium">{t("keys_created", { name: created.name })}</p>
          <code className="mt-1 block select-all break-all font-mono text-xs">{created.key}</code>
          <p className="mt-1 text-xs text-muted">{t("keys_created_hint")}</p>
        </div>
      ) : null}
      {keys.length === 0 ? (
        <p className="text-sm text-muted">{t("keys_empty")}</p>
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th scope="col">{t("keys_name")}</th>
                <th scope="col">{t("keys_key")}</th>
                <th scope="col">{t("keys_last_used")}</th>
                <th scope="col">IP</th>
                <th scope="col">{t("keys_created_at")}</th>
                <th scope="col" className="text-right">{t("actions")}</th>
              </tr>
            </thead>
            <tbody>
              {keys.map((k) => (
                <tr key={k.name} className={k.disabled ? "opacity-60" : ""}>
                  <td className="text-sm">
                    <StatusDot tone={k.disabled ? "muted" : "ok"} label={k.name} />
                    {k.disabled ? <span className="ml-1 text-xs text-muted">({t("keys_revoked")})</span> : null}
                  </td>
                  <td className="font-mono text-xs">{k.key}</td>
                  <td className="text-xs whitespace-nowrap" title={k.lastUsedAt ? formatDate(k.lastUsedAt) : ""}>{k.lastUsedAt ? formatRelative(k.lastUsedAt) : t("keys_never")}</td>
                  <td className="font-mono text-xs">{k.lastIp || "-"}</td>
                  <td className="text-xs whitespace-nowrap text-muted" title={formatDate(k.createdAt)}>{formatRelative(k.createdAt)}</td>
                  <td className="text-right whitespace-nowrap">
                    {k.disabled ? (
                      <button type="button" className="btn btn-sm" onClick={() => act(enableSyncKey, k)} disabled={busy}>
                        {t("keys_enable")}
                      </button>
                    ) : (
                      <button type="button" className="btn btn-sm" onClick={() => act(revokeSyncKey, k, "keys_confirm_revoke")} disabled={busy}>
                        {t("keys_revoke")}
                      </button>
                    )}{" "}
                    <button type="button" className="btn btn-sm btn-ghost" onClick={() => act(deleteSyncKey, k, "keys_confirm_delete")} disabled={busy}>
                      {t("delete")}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <p className="mt-3 text-xs text-muted">{t("keys_hint")}</p>
    </section>
  );
}

export function ClientsPage() {
  const t = useT();
  // `now` is stamped when the data arrives (render must stay pure).
  const { data, error, loading, reload } = usePolling(() => getSyncPeers().then((d) => ({ ...d, now: Date.now() })), 30_000);
  const peers = data?.peers || [];
  const now = data?.now || 0;
  const staleMs = (Number(data?.staleHours) > 0 ? Number(data.staleHours) : DEFAULT_STALE_HOURS) * 3_600_000;

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
                <th scope="col">{t("keys_name")}</th>
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
                const stale = !Number.isFinite(seenMs) || now - seenMs > staleMs;
                const cursor = fileTimeToIso(p.lastCursor);
                return (
                  <tr key={p.ip}>
                    <td className="font-mono text-xs">
                      <span className="flex items-center gap-2">
                        <StatusDot tone={stale ? "muted" : "ok"} label="" />
                        {p.ip}
                      </span>
                    </td>
                    <td className="text-xs">{p.name ? p.name : <span className="text-muted">-</span>}</td>
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
      <KeysSection />
    </>
  );
}
