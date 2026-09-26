import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ToastProvider } from "../../components/Toast.jsx";
import { ConfirmProvider } from "../../components/Confirm.jsx";
import { setBaseForTests } from "../../lib/base.js";
import { FdbJournalCard } from "../FdbJournal.jsx";

const STATE = {
  enabled: true,
  retentionDays: 7,
  maxSizeMb: 1024,
  maxFiles: 0,
  files: 3,
  totalBytes: 5 * 1024 * 1024,
  oldest: "2026-09-24",
  newest: "2026-09-26",
};

function stub() {
  const fn = vi.fn(async (url, init) => {
    const u = String(url);
    let body = STATE;
    if (u.endsWith("/api/logs/fdb") && init?.method === "POST")
      body = {
        ok: true,
        state: {
          ...STATE,
          ...JSON.parse(init.body),
          enabled: JSON.parse(init.body).enabled ?? STATE.enabled,
        },
      };
    return new Response(JSON.stringify(body), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  });
  vi.stubGlobal("fetch", fn);
  return fn;
}

function renderCard() {
  setBaseForTests("/admin");
  return render(
    <ToastProvider>
      <ConfirmProvider>
        <FdbJournalCard />
      </ConfirmProvider>
    </ToastProvider>,
  );
}

describe("FdbJournalCard", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("shows usage and turns the journal off without confirmation", async () => {
    const fetch = stub();
    renderCard();
    expect(await screen.findByText(/На диске: 5/)).toBeInTheDocument();
    await userEvent.click(screen.getByLabelText("Включён"));
    await waitFor(() => {
      const call = fetch.mock.calls.find(
        ([u, i]) => String(u).endsWith("/api/logs/fdb") && i?.method === "POST",
      );
      expect(JSON.parse(call[1].body)).toEqual({ enabled: false });
      expect(call[1].headers["X-Crab-Admin"]).toBe("1");
    });
  });

  it("saves limits", async () => {
    const fetch = stub();
    renderCard();
    const size = await screen.findByLabelText(/Не больше, МБ/);
    await userEvent.clear(size);
    await userEvent.type(size, "500");
    await userEvent.click(
      screen.getByRole("button", { name: /Сохранить ограничения/ }),
    );
    await waitFor(() => {
      const call = fetch.mock.calls.find(
        ([u, i]) => String(u).endsWith("/api/logs/fdb") && i?.method === "POST",
      );
      expect(JSON.parse(call[1].body)).toEqual({
        retentionDays: 7,
        maxSizeMb: 500,
      });
    });
  });
});
