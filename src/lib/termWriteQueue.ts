// One write queue per terminal (finding z3): every hop to a shown machine runs calls side by side, so keystrokes sent
// as separate calls can reach the shell out of order. Here a terminal has at most one write on its way; what is typed
// meanwhile is gathered and sent as one write after it. A lone keystroke goes at once, as before.

type Send = (id: string, data: string) => Promise<void>;

interface Waiting {
  data: string;
  done: Array<{ resolve: () => void; reject: (e: unknown) => void }>;
}

interface Lane {
  busy: boolean;
  next: Waiting | null;
}

export function termWriteQueue(send: Send) {
  const lanes = new Map<string, Lane>();

  async function run(id: string, lane: Lane, first: Waiting) {
    let batch: Waiting | null = first;
    while (batch) {
      try {
        await send(id, batch.data);
        for (const d of batch.done) d.resolve();
      } catch (e) {
        for (const d of batch.done) d.reject(e);
      }
      batch = lane.next;
      lane.next = null;
    }
    lane.busy = false;
    lanes.delete(id);
  }

  return function write(id: string, data: string): Promise<void> {
    return new Promise<void>((resolve, reject) => {
      let lane = lanes.get(id);
      if (!lane) {
        lane = { busy: false, next: null };
        lanes.set(id, lane);
      }
      if (lane.busy) {
        if (!lane.next) lane.next = { data: "", done: [] };
        lane.next.data += data;
        lane.next.done.push({ resolve, reject });
        return;
      }
      lane.busy = true;
      void run(id, lane, { data, done: [{ resolve, reject }] });
    });
  };
}
