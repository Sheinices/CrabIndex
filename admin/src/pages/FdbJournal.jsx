import { useEffect, useState } from "react";
import { Trash2 } from "lucide-react";
import { clearFdbLog, getFdbLog, setFdbLog } from "../lib/api.js";
import { useConfirm } from "../components/Confirm.jsx";
import { useToast } from "../components/Toast.jsx";
import { Spinner, StatusDot, Toggle } from "../components/ui.jsx";
import { formatBytes } from "../lib/format.js";

/** FileDB change journal (logFdb): on/off, limits and disk usage. */
export function FdbJournalCard({ onChanged }) {
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
        toast.error("Не сохранено", e?.message || String(e));
    } finally {
      setBusy("");
    }
  };

  const toggle = async (on) => {
    if (on) {
      const ok = await confirm({
        title: "Включить журнал изменений базы?",
        message:
          "На каждое добавление и изменение раздачи в Data/log/fdb.*.log будет записываться строка «было / стало». При активном парсинге это много записи на диск. Включайте для разбора проблем и не забудьте ограничение размера.",
        confirmLabel: "Включить",
      });
      if (!ok) return;
    }
    await save(
      "toggle",
      { enabled: on },
      on ? "Журнал изменений включён" : "Журнал изменений выключен",
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
      toast.error("Проверьте значения", "Нужны целые числа от 0");
      return;
    }
    save("limits", { retentionDays, maxSizeMb }, "Ограничения сохранены");
  };

  const clear = async () => {
    const ok = await confirm({
      title: "Удалить журналы изменений?",
      message: `Будут удалены все файлы Data/log/fdb.*.log (${formatBytes(st?.totalBytes || 0)}). База не затрагивается.`,
      confirmLabel: "Удалить",
      danger: true,
    });
    if (!ok) return;
    setBusy("clear");
    try {
      const r = await clearFdbLog();
      toast.success(
        `Удалено файлов: ${r.files}, освобождено ${formatBytes(r.bytes)}`,
      );
      apply(await getFdbLog());
      onChanged?.();
    } catch (e) {
      if (e?.status !== 401) toast.error("Не удалено", e?.message || String(e));
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
            Журнал изменений базы (logFdb)
          </h2>
          <p className="mt-1 max-w-2xl text-sm text-muted">
            Строка «было / стало» на каждое изменение раздачи. Нужен только для
            отладки: при активном парсинге пишет на диск гигабайты. По умолчанию
            выключен.
          </p>
        </div>
        <Toggle
          id="fdb-journal-on"
          checked={!!st.enabled}
          onChange={toggle}
          disabled={!!busy}
          label={st.enabled ? "Включён" : "Выключен"}
        />
      </div>
      <div className="mt-4 flex flex-wrap items-center gap-x-6 gap-y-2 text-sm">
        <StatusDot
          tone={over ? "warn" : st.totalBytes ? "brand" : "muted"}
          label={`На диске: ${formatBytes(st.totalBytes)} · файлов: ${st.files}`}
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
          Удалить журналы
        </button>
      </div>
      <form
        className="mt-4 flex flex-wrap items-end gap-3"
        onSubmit={saveLimits}
      >
        <div>
          <label className="label" htmlFor="fdb-days">
            Хранить, дней (0 - не удалять)
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
            Не больше, МБ (0 - без лимита)
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
          {busy === "limits" ? <Spinner /> : null} Сохранить ограничения
        </button>
      </form>
      <p className="mt-3 text-xs text-muted">
        При превышении срока или размера самые старые файлы удаляются
        автоматически (проверка раз в минуту). Настройки сохраняются в конфиг,
        как в разделе «Настройки».
      </p>
    </section>
  );
}
