import { assert, describe, it } from "vitest";

import { hello } from "@root";

describe("Basics", () => {
    it("hello", () => {
        assert.equal(hello(), "Hello, World!");
    });
});
