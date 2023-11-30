import { assert, describe, it } from "vitest";

import { add, genPath } from ".";

describe("Utils", () => {
    it("add", () => {
        assert.equal(add(1, 2), 3);
    });
    describe("genPath", () => {
        it("Defaults", () => {
            // Dir looking path slashes everywhere:
            assert.equal(genPath("test"), "/test/");
            // File looking path not at end:
            assert.equal(genPath("test.txt"), "/test.txt");
            // Url looking path no where:
            assert.equal(genPath("http://test.com"), "http://test.com");
            // Relative path should add at end but not beginning:
            assert.equal(genPath("./test"), "./test/");
            // Double for good measure:
            assert.equal(genPath("../test/"), "../test/");
        });
        it("Overrides", () => {
            assert.equal(genPath("/test/", { eSlash: false, sShlash: false }), "test");
        });
        it("Extra", () => {
            assert.equal(genPath("test", { extra: ["foo", "bar"] }), "/test/foo/bar/");
            assert.equal(
                genPath("test/", { eSlash: false, extra: ["foo", "bar"] }),
                "/test/foo/bar",
            );
        });
    });
});
