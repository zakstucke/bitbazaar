import { assert, describe, it } from "vitest";

import { createConfig, TopViteConfig } from ".";
import { genBackendProxies } from "./genProxy";

const defaultConf: TopViteConfig = {
    port: 3000,
    favicon192PngPath: "test",
    favicon512PngPath: "test",
    proxy: {
        matches: [],
        target: "test",
    },
    sameDomStaticPath: "test",
    sameDomStaticUrl: "test",
    staticPath: "test",
    staticUrl: "test",
    siteName: "test",
    siteDescription: "test",
    inspect: true,
};

describe("Vite", () => {
    describe("createConfig", () => {
        it("Intended usage", () => {
            // Just confirm no errors on any of the normal modes:
            createConfig("test", defaultConf);
            createConfig("development", defaultConf);
            createConfig("production", defaultConf);
        });
        it("Error on unknown mode", () => {
            assert.throws(() => {
                createConfig("unknown", defaultConf);
            });
        });
    });

    describe("genProxy", () => {
        it("Intended usage", () => {
            const inside = ["/api/", "/api/foo/bar/index.html"];
            const outside = ["", "/", "/api2", "/api2/", "/api2/foo/bar/index.html"];

            let out = genBackendProxies({ matches: ["/api/"], target: "test", negate: false });
            let matcher = new RegExp(Object.keys(out)[0]);
            for (const path of outside) {
                assert.ok(!path.match(matcher));
            }
            for (const path of inside) {
                assert.ok(path.match(matcher));
            }

            // Negate should be the opposite:
            out = genBackendProxies({ matches: ["/api/"], target: "test", negate: true });
            matcher = new RegExp(Object.keys(out)[0]);
            for (const path of inside) {
                assert.ok(!path.match(matcher));
            }
            for (const path of outside) {
                assert.ok(path.match(matcher));
            }
        });
    });
});
