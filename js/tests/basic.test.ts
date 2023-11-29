import { assert, describe, it } from "vitest";

import { hello } from "@root";
import { add } from "@root/utils";

describe("Basics", () => {
    it("hello", () => {
        assert.equal(hello(), "Hello, World!");
    });

    it("add", () => {
        assert.equal(add(1, 2), 3);
    });
});
