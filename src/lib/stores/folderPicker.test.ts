import { beforeEach, describe, expect, it, vi } from "vitest";

// 24z3 step 3, f1: a shown server's "+" picks a folder on that server through Qanuvi's own picker; every read names
// the machine, a typed path must exist there, and switching machines closes it.
const tree: Record<string, { parent: string | null; dirs: string[] }> = {
  "/root": { parent: "/", dirs: ["sites"] },
  "/": { parent: null, dirs: ["root", "srv"] },
  "/root/sites": { parent: "/root", dirs: [] },
};
const asked: { machine: string | null; path: unknown }[] = [];
vi.mock("$lib/remote", () => ({
  machineName: (id: string) => `name of ${id}`,
  callOn: vi.fn(async (machine: string | null, cmd: string, args: { path?: string }) => {
    asked.push({ machine, path: args.path });
    if (cmd !== "dirs_list") throw new Error("unexpected");
    const path = args.path || "/root";
    const found = tree[path];
    if (!found) throw new Error(`The folder ${path} cannot be found.`);
    const join = (d: string) => (path === "/" ? `/${d}` : `${path}/${d}`);
    return { path, parent: found.parent, dirs: found.dirs.map((d) => ({ name: d, path: join(d) })) };
  }),
}));

const { folderPicker } = await import("./folderPicker.svelte");
const settle = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  asked.length = 0;
  folderPicker.cancel();
});

describe("a server's folder picker", () => {
  it("starts at the machine's home, walks up and down, and answers the folder chosen", async () => {
    const picked = folderPicker.pick("vps");
    await settle();
    expect(folderPicker.current?.name).toBe("name of vps");
    expect(folderPicker.folder?.path).toBe("/root");
    folderPicker.up();
    await settle();
    expect(folderPicker.folder?.dirs.map((d) => d.name)).toEqual(["root", "srv"]);
    await folderPicker.go("/root/sites");
    await folderPicker.choose();
    expect(await picked).toBe("/root/sites");
    expect(folderPicker.current).toBeNull();
    expect(new Set(asked.map((a) => a.machine)), "every read names the machine").toEqual(new Set(["vps"]));
  });

  it("checks a typed path on the machine before choosing it", async () => {
    const picked = folderPicker.pick("vps");
    await settle();
    folderPicker.typed = "/nowhere";
    await folderPicker.choose();
    expect(folderPicker.current, "still open").not.toBeNull();
    expect(folderPicker.error).toContain("cannot be found");
    expect(folderPicker.folder?.path, "the folder before stays listed").toBe("/root");
    folderPicker.typed = "/root/sites";
    await folderPicker.choose();
    expect(await picked).toBe("/root/sites");
  });

  it("closes with nothing chosen when another machine is shown, or on Cancel", async () => {
    const first = folderPicker.pick("vps");
    await settle();
    folderPicker.switched("vps");
    expect(folderPicker.current, "the same machine keeps it open").not.toBeNull();
    folderPicker.switched(null);
    expect(await first).toBeNull();
    const second = folderPicker.pick("vps");
    folderPicker.cancel();
    expect(await second).toBeNull();
  });
});
