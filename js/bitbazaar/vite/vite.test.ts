import { createConfig, TopViteConfig } from ".";
import { describe, expect, it } from "bun:test";
import { tmpdir } from "os";

import fs from "fs/promises";

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
            // Convert to expect:
            expect(() => {
                createConfig("unknown", defaultConf);
            }).toThrow();
        });
        it("closeBundle callback", async () => {
            const tmpDir = tmpdir();
            const staticPath = `${tmpDir}/bb_vite_static`;
            const assetsPath = `${tmpDir}/bb_vite_static/dist`;
            const sameDomStaticPath = `${tmpDir}/bb_vite_same_dom_static`;
            if (await fs.exists(assetsPath)) {
                await fs.rm(assetsPath, { recursive: true });
            }
            if (await fs.exists(sameDomStaticPath)) {
                await fs.rm(sameDomStaticPath, { recursive: true });
            }
            await fs.mkdir(assetsPath, { recursive: true });
            await fs.mkdir(sameDomStaticPath, { recursive: true });

            await fs.writeFile(`${assetsPath}/manifest.webmanifest`, "{}");
            await fs.writeFile(`${assetsPath}/index.html`, "<html></html>");

            const conf = createConfig("test", {
                ...defaultConf,
                staticPath,
                sameDomStaticPath,
            });
            if (!conf.plugins) {
                throw new Error("Plugins not found");
            }
            const plugin = conf.plugins.find(
                (p) => p && typeof p === "object" && "name" in p && p.name === "move-webmanifest",
            );
            if (!plugin) {
                throw new Error("Plugin not found");
            }
            // @ts-expect-error - Testing specific fn
            // eslint-disable-next-line @typescript-eslint/no-unsafe-call
            await plugin.closeBundle();

            // Check files gone from assetsPath:
            expect(fs.readdir(`${assetsPath}`)).resolves.toEqual([]);
            // Check files in sameDomStaticPath:
            expect(fs.readdir(sameDomStaticPath)).resolves.toEqual(["site_index.html", "sworker"]);
            // Confirm sworker contains webmanifest:
            expect(
                fs.readFile(`${sameDomStaticPath}/sworker/manifest.webmanifest`, "utf-8"),
            ).resolves.toContain("{}");
        });
    });

    describe("genProxy", () => {
        it("Intended usage", () => {
            const inside = ["/api/", "/api/foo/bar/index.html"];
            const outside = ["", "/", "/api2", "/api2/", "/api2/foo/bar/index.html"];

            let out = genBackendProxies({ matches: ["/api/"], target: "test", negate: false });
            let matcher = new RegExp(Object.keys(out)[0]);
            for (const path of outside) {
                expect(path.match(matcher)).toBeFalsy();
            }
            for (const path of inside) {
                expect(path.match(matcher)).toBeTruthy();
            }

            // Negate should be the opposite:
            out = genBackendProxies({ matches: ["/api/"], target: "test", negate: true });
            matcher = new RegExp(Object.keys(out)[0]);
            for (const path of inside) {
                expect(path.match(matcher)).toBeFalsy();
            }
            for (const path of outside) {
                expect(path.match(matcher)).toBeTruthy();
            }
        });
    });
});
