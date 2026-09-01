import { describe, expect, it } from "vitest";
import { mdEscape, mdLink } from "./markdown";

describe("mdEscape", () => {
  it("escapes brackets so a title cannot swallow its link", () => {
    expect(mdEscape("Ch. 3 [cont.")).toBe("Ch. 3 \\[cont.");
  });

  it("escapes emphasis markers", () => {
    expect(mdEscape("week_1_notes")).toBe("week\\_1\\_notes");
    expect(mdEscape("*required*")).toBe("\\*required\\*");
  });

  it("escapes backslashes without double-escaping what follows", () => {
    expect(mdEscape("a\\[b")).toBe("a\\\\\\[b");
  });

  it("leaves ordinary titles alone", () => {
    expect(mdEscape("Levenson, 2017TIP")).toBe("Levenson, 2017TIP");
  });
});

describe("mdLink", () => {
  it("escapes the label", () => {
    expect(mdLink("Ch. 3 [cont.", "https://x/y")).toBe("[Ch. 3 \\[cont.](<https://x/y>)");
  });

  it("survives a destination with spaces and parens", () => {
    expect(mdLink("Wiki", "https://x/Social_work_(profession) v2")).toBe(
      "[Wiki](<https://x/Social_work_(profession) v2>)",
    );
  });

  it("percent-encodes angle brackets that would end the destination", () => {
    expect(mdLink("T", "https://x/a<b>c")).toBe("[T](<https://x/a%3Cb%3Ec>)");
  });
});
