import { useEffect, useState } from "react";
import { Trash2 } from "lucide-react";
import { clearFdbLog, getFdbLog, setFdbLog } from "../lib/api.js";
import { useConfirm } from "../components/Confirm.jsx";
import { useToast } from "../components/Toast.jsx";
import { Spinner, StatusDot, Toggle } from "../components/ui.jsx";
import { formatBytes } from "../lib/format.js";
import { useT } from "../lang/index.jsx";

/** FileDB change journal (logFdb): on/off, limits and disk usage. */
export function FdbJournalCard({ onChanged }) {
  const t = useT();
  const [st, setSt] = useState(null);
  const [form, setForm] = useState({ retentionDays: "", maxSizeMb: "" });
  const [busy, setBusy] = useState("");
  const confirm = useConfirm();
  const toast = useToast();

  const apply = (v) => {
    setSt(v);
    setForm({
      retentionDays: String(v.retentionDays ?? ""),
      maxSizeMb: String(v.maxSizeMb ?? ""),
    });
  };

  useEffect(() => {
    getFdbLog()
      .then(apply)
      .catch(() => {});
  }, []);

  const save = async (key, patch, done) => {
    setBusy(key);
    try {
      const r = await setFdbLog(patch);
      apply(r.state);
      toast.success(done);
      onChanged?.();
    } catch (e) {
      if (e?.status !== 401)
        toast.error(t("fdb_not_saved"), e?.message || String(e));
    } finally {
      setBusy("");
    }
  };

  const toggle = async (on) => {
    if (on) {
      const ok = await confirm({
        title: t("fdb_enable_title"),
        message: t("fdb_enable_msg"),
        confirmLabel: t("enable"),
      });
      if (!ok) return;
    }
    await save(
      "toggle",
      { enabled: on },
      on ? t("fdb_on_toast") : t("fdb_off_toast"),
    );
  };

  const saveLimits = (e) => {
    e.preventDefault();
    const retentionDays = Number(form.retentionDays);
    const maxSizeMb = Number(form.maxSizeMb);
    if (
      !Number.isInteger(retentionDays) ||
      retentionDays < 0 ||
      !Number.isInteger(maxSizeMb) ||
      maxSizeMb < 0
    ) {
      toast.error(t("fdb_check_values"), t("fdb_need_ints"));
      return;
    }
    save("limits", { retentionDays, maxSizeMb }, t("fdb_limits_saved"));
  };

  const clear = async () => {
    const ok = await confirm({
      title: t("fdb_clear_title"),
      message: t("fdb_clear_msg", { size: formatBytes(st?.totalBytes || 0) }),
      confirmLabel: t("delete"),
      danger: true,
    });
    if (!ok) return;
    setBusy("clear");
    try {
      const r = await clearFdbLog();
      toast.success(t("fdb_cleared", { files: r.files, bytes: formatBytes(r.bytes) }));
      apply(await getFdbLog());
      onChanged?.();
    } catch (e) {
      if (e?.status !== 401) toast.error(t("fdb_not_deleted"), e?.message || String(e));
    } finally {
      setBusy("");
    }
  };

  if (!st) return null;
  const over = st.maxSizeMb > 0 && st.totalBytes > st.maxSizeMb * 1024 * 1024;

  return (
    <section className="card mb-6 p-5" aria-labelledby="fdb-journal">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="min-w-0">
          <h2 id="fdb-journal" className="font-semibold">
            {t("fdb_title")}
          </h2>
          <p className="mt-1 max-w-2xl text-sm text-muted">{t("fdb_desc")}</p>
        </div>
        <Toggle
          id="fdb-journal-on"
          checked={!!st.enabled}
          onChange={toggle}
          disabled={!!busy}
          label={st.enabled ? t("fdb_enabled") : t("fdb_disabled")}
        />
      </div>
      <div className="mt-4 flex flex-wrap items-center gap-x-6 gap-y-2 text-sm">
        <StatusDot
          tone={over ? "warn" : st.totalBytes ? "brand" : "muted"}
          label={t("fdb_on_disk", { size: formatBytes(st.totalBytes), files: st.files })}
        />
        {st.oldest ? (
          <span className="text-xs text-muted">
            {st.oldest === st.newest
              ? st.oldest
              : `${st.oldest} - ${st.newest}`}
          </span>
        ) : null}
        <button
          type="button"
          className="btn btn-sm"
          onClick={clear}
          disabled={!!busy || !st.files}
        >
          {busy === "clear" ? (
            <Spinner />
          ) : (
            <Trash2 className="size-4" aria-hidden="true" />
          )}{" "}
          {t("fdb_clear_btn")}
        </button>
      </div>
      <form
        className="mt-4 flex flex-wrap items-end gap-3"
        onSubmit={saveLimits}
      >
        <div>
          <label className="label" htmlFor="fdb-days">
            {t("fdb_keep_days")}
          </label>
          <input
            id="fdb-days"
            type="number"
            min="0"
            className="input w-40"
            value={form.retentionDays}
            onChange={(e) =>
              setForm({ ...form, retentionDays: e.target.value })
            }
          />
        </div>
        <div>
          <label className="label" htmlFor="fdb-size">
            {t("fdb_max_mb")}
          </label>
          <input
            id="fdb-size"
            type="number"
            min="0"
            className="input w-40"
            value={form.maxSizeMb}
            onChange={(e) => setForm({ ...form, maxSizeMb: e.target.value })}
          />
        </div>
        <button type="submit" className="btn btn-sm" disabled={!!busy}>
          {busy === "limits" ? <Spinner /> : null} {t("fdb_save_limits")}
        </button>
      </form>
      <p className="mt-3 text-xs text-muted">{t("fdb_note")}</p>
    </section>
  );
}
