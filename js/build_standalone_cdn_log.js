import { build } from "esbuild";

build({
    entryPoints: ["bitbazaar/log/index.ts"],
    minify: true,
    bundle: true,
    platform: "browser",
    format: "esm",
    outfile: "dist/standalone_cdn_log.js",
    treeShaking: true,
});
