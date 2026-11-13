<script lang="ts">
  /**
   * Settings → Notifications (24u part 4, session B): new Inbox items as desktop notifications. A click brings Qanuvi
   * to the front with the Inbox open at the item. The Inbox keeps every item, whatever is shown here.
   */
  import { settings, type NotifySettings } from "$lib/stores/settings.svelte";
  import { testNotification } from "$lib/stores/notify.svelte";

  let n = $derived(settings.value.notifications);
  let tested = $state("");

  function set(patch: Partial<NotifySettings>) {
    settings.set("notifications", { ...n, ...patch });
  }

  const hhmm = (m: number) => `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
  function minutes(v: string, fallback: number): number {
    const m = /^(\d{1,2}):(\d{2})$/.exec(v);
    return m ? Math.min(23, Number(m[1])) * 60 + Math.min(59, Number(m[2])) : fallback;
  }

  async function test() {
    tested = "";
    try {
      await testNotification();
      tested = "Sent. If nothing showed, check the system's notification settings (Do Not Disturb, Focus).";
    } catch (e) {
      tested = `It could not be shown: ${e}`;
    }
  }
</script>

<h2>Notifications</h2>
<p class="intro">
  New Inbox items can show as desktop notifications, so a run that waits for you is hard to miss. A click brings Qanuvi
  to the front with the Inbox open at the item. Whatever is shown, the Inbox keeps every item; Do Not Disturb or Focus
  on your system still applies.
</p>

<label class="auto">
  <input type="checkbox" checked={n.desktop} onchange={(e) => set({ desktop: e.currentTarget.checked })} />
  <span>
    <strong>Desktop notifications</strong>
    <span class="muted">Off: new items only show in the Inbox.</span>
  </span>
</label>

<div class="card" class:off={!n.desktop}>
  <div class="row">
    <span>
      <strong>What to show</strong>
      <span class="muted">Questions are the steps a run waits on.</span>
    </span>
    <select
      value={n.what}
      disabled={!n.desktop}
      onchange={(e) => set({ what: e.currentTarget.value as NotifySettings["what"] })}
    >
      <option value="questions">Questions only</option>
      <option value="problems">Questions and problems</option>
      <option value="all">Everything (also finished runs)</option>
    </select>
  </div>
  <label class="row check">
    <span>
      <strong>Several at once as one</strong>
      <span class="muted">Items coming within a few seconds show as one notification ("3 new in the Inbox").</span>
    </span>
    <input
      type="checkbox"
      checked={n.bundle}
      disabled={!n.desktop}
      onchange={(e) => set({ bundle: e.currentTarget.checked })}
    />
  </label>
  <label class="row check">
    <span>
      <strong>Quiet hours</strong>
      <span class="muted">Nothing shows in this time (this computer's clock); the Inbox still gets everything.</span>
    </span>
    <input
      type="checkbox"
      checked={n.quiet}
      disabled={!n.desktop}
      onchange={(e) => set({ quiet: e.currentTarget.checked })}
    />
  </label>
  {#if n.quiet}
    <div class="row">
      <span class="muted">From</span>
      <span class="times">
        <input
          type="time"
          value={hhmm(n.quietFrom)}
          disabled={!n.desktop}
          onchange={(e) => set({ quietFrom: minutes(e.currentTarget.value, n.quietFrom) })}
        />
        <span class="muted">to</span>
        <input
          type="time"
          value={hhmm(n.quietTo)}
          disabled={!n.desktop}
          onchange={(e) => set({ quietTo: minutes(e.currentTarget.value, n.quietTo) })}
        />
      </span>
    </div>
  {/if}
  <div class="actions">
    <button type="button" class="btn" onclick={test}>Send a test notification</button>
  </div>
  {#if tested}<p class="status">{tested}</p>{/if}
</div>

<style>
  h2 {
    margin: 18px 20px 4px;
    font-size: 16px;
  }

  .intro {
    margin: 0 20px 14px;
    color: var(--qx-text-secondary);
    font-size: 12.5px;
  }

  .auto {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    margin: 12px 20px;
    cursor: pointer;
    font-size: 12.5px;
  }

  .auto > span,
  .row > span:first-child {
    display: grid;
    gap: 2px;
  }

  .card {
    display: grid;
    gap: 12px;
    margin: 0 20px;
    padding: 12px 14px;
    border: 1px solid var(--qx-border);
    border-radius: 8px;
    background: var(--qx-bg-raised);
    font-size: 12.5px;
  }

  .card.off .row {
    opacity: 0.55;
  }

  .row {
    display: flex;
    gap: 12px;
    align-items: center;
    justify-content: space-between;
  }

  .check {
    cursor: pointer;
  }

  .times {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .muted {
    color: var(--qx-text-secondary);
  }

  .status {
    margin: 0;
    color: var(--qx-text-secondary);
  }
</style>
