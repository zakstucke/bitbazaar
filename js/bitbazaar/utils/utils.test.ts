import { add, genPath } from ".";
import { describe, expect, it } from "bun:test";

describe("Utils", () => {
    it("add", () => {
        expect(add(1, 2)).toBe(3);
    });
    describe("genPath", () => {
        it("Defaults", () => {
            // Dir looking path slashes everywhere:
            expect(genPath("test")).toBe("/test/");
            // File looking path not at end:
            expect(genPath("test.txt")).toBe("/test.txt");
            // Url looking path no where:
            expect(genPath("http://test.com")).toBe("http://test.com");
            // Relative path should add at end but not beginning:
            expect(genPath("./test")).toBe("./test/");
            // Double for good measure:
            expect(genPath("../test/")).toBe("../test/");
        });
        it("Overrides", () => {
            expect(genPath("/test/", { eSlash: false, sShlash: false })).toBe("test");
        });
        it("Extra", () => {
            expect(genPath("test", { extra: ["foo", "bar"] })).toBe("/test/foo/bar/");
            expect(genPath("test/", { eSlash: false, extra: ["foo", "bar"] })).toBe(
                "/test/foo/bar",
            );
        });
    });
});
