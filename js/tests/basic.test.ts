import { describe, expect, it } from "bun:test";

import { hello } from "@root";
import { add } from "@root/utils";

describe("Basics", () => {
    it("hello", () => {
        expect(hello()).toBe("Hello, World!");
    });

    it("add", () => {
        expect(add(1, 2)).toBe(3);
    });
});
