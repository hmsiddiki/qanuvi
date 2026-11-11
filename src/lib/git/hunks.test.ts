import { describe, expect, it } from "vitest";
import { conflicts, giveBack, hunks, lineMarks, resolve, takeHunk } from "./hunks";

const head = "one\ntwo\nthree\nfour\n";
const work = "ONE\ntwo\nthree\nfour\nfive\n";

describe("hunks", () => {
  it("finds each change with its lines", () => {
    const hs = hunks(head, work);
    expect(hs.map((h) => [h.line, h.linesA, h.linesB])).toEqual([
      [1, 1, 1],
      [5, 0, 1],
    ]);
    expect(hunks(head, head)).toEqual([]);
  });

  it("stages one change and leaves the other", () => {
    const [first, second] = hunks(head, work);
    expect(takeHunk(head, work, first)).toBe("ONE\ntwo\nthree\nfour\n");
    expect(takeHunk(head, work, second)).toBe("one\ntwo\nthree\nfour\nfive\n");
  });

  it("undoes one change in the file", () => {
    const [first] = hunks(head, work);
    expect(giveBack(head, work, first)).toBe("one\ntwo\nthree\nfour\nfive\n");
  });

  it("keeps Windows line endings", () => {
    const a = head.replace(/\n/g, "\r\n");
    const b = work.replace(/\n/g, "\r\n");
    const [first, second] = hunks(a, b);
    expect(takeHunk(a, b, first)).toBe("ONE\r\ntwo\r\nthree\r\nfour\r\n");
    expect(giveBack(a, b, second)).toBe("ONE\r\ntwo\r\nthree\r\nfour\r\n");
  });

  it("handles a file without a final line break", () => {
    const [h] = hunks("a\nb", "a\nB");
    expect(takeHunk("a\nb", "a\nB", h)).toBe("a\nB");
    expect(giveBack("a\nb", "a\nB", h)).toBe("a\nb");
  });
});

describe("lineMarks", () => {
  it("marks added, changed and removed lines", () => {
    const marks = lineMarks("a\nb\nc\nd\n", "a\nB\nd\ne\n");
    expect(marks.get(2)).toBe("changed");
    expect(marks.get(4)).toBe("added");
    expect(marks.has(1)).toBe(false);
    expect(lineMarks("a\nb\nc\n", "a\nc\n").get(2)).toBe("removed");
  });
});

describe("conflicts", () => {
  const text = ["start", "<<<<<<< HEAD", "mine", "=======", "theirs", ">>>>>>> feature", "end", ""].join("\n");

  it("finds the blocks with their sides", () => {
    const [c] = conflicts(text);
    expect(c).toMatchObject({
      line: 2,
      current: "mine\n",
      incoming: "theirs\n",
      currentLabel: "HEAD",
      incomingLabel: "feature",
    });
  });

  it("keeps current, incoming or both", () => {
    const [c] = conflicts(text);
    expect(resolve(text, c, "current")).toBe("start\nmine\nend\n");
    expect(resolve(text, c, "incoming")).toBe("start\ntheirs\nend\n");
    expect(resolve(text, c, "both")).toBe("start\nmine\ntheirs\nend\n");
  });

  it("reads three-way blocks (with the common version) too", () => {
    const three = "<<<<<<< ours\nA\n||||||| base\nO\n=======\nB\n>>>>>>> theirs\n";
    expect(conflicts(three)[0]).toMatchObject({ current: "A\n", incoming: "B\n" });
    expect(conflicts("no conflict here\n")).toEqual([]);
  });
});

describe("line diff", () => {
  it("keeps nearby changes apart, as git does", () => {
    const base = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\n";
    const doc = "line 1xyz\nLINE TWO\nline 3\nline 4\nline 6\nline 7\nline 8\nLINE NINE\nline 10\nline 11 added\n";
    const marks = lineMarks(base, doc);
    expect([...marks.entries()].sort((x, y) => x[0] - y[0])).toEqual([
      [1, "changed"],
      [2, "changed"],
      [5, "removed"],
      [8, "changed"],
      [10, "added"],
    ]);
  });

  it("finds the shortest set of changes in shuffled lines", () => {
    const hs = hunks("a\nb\nc\nd\ne\n", "a\nc\nb\nd\nx\ne\n");
    // Two changes besides the moved line: every hunk applied in turn gives the new text.
    let text = "a\nb\nc\nd\ne\n";
    for (const h of [...hs].reverse()) text = takeHunk(text, "a\nc\nb\nd\nx\ne\n", h);
    expect(text).toBe("a\nc\nb\nd\nx\ne\n");
    expect(hs.length).toBeLessThanOrEqual(3);
  });
});

describe("line endings", () => {
  it("treats CRLF and LF lines as the same", () => {
    const staged = "a\r\nb\r\nc\r\n";
    const file = "a\nb\nc\n// changed\n";
    expect([...lineMarks(staged, file).entries()]).toEqual([[4, "added"]]);
  });
});
