import preact from "@preact/preset-vite";
import { minify } from "html-minifier-terser";
import checker from "vite-plugin-checker";
import Inspect from "vite-plugin-inspect";
import { VitePWA } from "vite-plugin-pwa";
import { defineConfig, UserConfig } from "vitest/config";

import fs from "fs/promises";

import { genPath } from "./genPath";
import { genBackendProxies, ProxyConf } from "./genProxy";

const baseNonFrontendGlobs: string[] = [
    "**/.git/**",
    "**/venv/**",
    "**/.venv/**",
    "**/node_modules/**",
    "**/dist/**",
    "**/cypress/**",
    "**/coverage/**",
    "**/htmlcov/**",
    "**/.{idea,git,cache,output,temp,mypy_cache,pytype}/**",
    "**/{karma,rollup,webpack,vite,vitest,jest,ava,babel,nyc,cypress,tsup,build}.config.*",
];

export interface TopViteConfig {
    siteName: string;
    siteDescription: string;

    /** The absolute url online that `staticPath` files can be retrieved from
     * E.g. https://example.com/static
     */
    staticUrl: string;
    /** The os path to the files that will deployed to `staticUrl`.
     * E.g. /.../static
     */
    staticPath: string;

    /** The absolute url online that `sameDomStaticPath` files can be retrieved from
     * Note if the static files are hosted on the same domain (i.e. not linking to a cdn, these can be the same as main static version)
     * E.g. https://example.com/~sds
     */
    sameDomStaticUrl: string;
    /** The os path to the files that will deployed to `sameDomStaticUrl`.
     * Note if the static files are hosted on the same domain (i.e. not linking to a cdn, these can be the same as main static version)
     * E.g. /.../~sds
     */
    sameDomStaticPath: string;

    favicon192PngPath: string;
    favicon512PngPath: string;

    /** Proxy rules to apply to the dev server (i.e. when it should forward requests to a backend) */
    proxy: ProxyConf;

    /** Extra globs to exclude from traversal, can help with performance: */
    extraNonFrontendGlobs?: string[];

    /** Include the inspect plugin, which allows you to see how vite is transforming code: */
    inspect?: boolean;

    /** Allows vite to handle custom import phrases, note if they're in tsconfig not 100% sure if this is needed.
    *   E.g. import { resolve } from "path";
        alias: {
            "@": resolve("./bitbazaar"),
        }

        Or better yet, just use the tsconfig paths:
        import tsconfig from "./tsconfig.json";
        alias: tsconfig.compilerOptions.paths,
     */
    alias?: Exclude<UserConfig["resolve"], undefined>["alias"];

    // If not using a separate vitest.config.ts, can pass test config here:
    test?: UserConfig["test"];
}

/** An opinionated outer config wrapper for vite (layers upon layers!!). To prevent having unique & complex config setups across multiple similar projects.
 * This handles index minification, css, scss, preact, pwa, service worker etc and the fact they can't come from a separate CDN.
 * Designed to work with a custom index.html file.
 *
 * CSS:
 * - css/scss is handled automatically, postcss.config.cjs is being detected automatically
 * - foo.module.s?css identifies local, everything else is treated as global
 * - (for potential future compatibility still globals as write as foo.global.s?css)
 *
 * HTML ENTRY:
 * - Vite looks for an index.html file at the root, there's currently no way to configure this.
 * - If you need to preprocess in any way, e.g. django or etch. You'll have to have a source you preprocess first before running vite, writing it to root/index.html.
 * - Vite will process it further
 * - The final minified index.html will be added to the assets folder, where it should be the root of a static site, or server manually from a backend server.
 */
export const createConfig = (mode: string, conf: TopViteConfig): UserConfig => {
    const isProd = mode === "production";
    const isTest = mode === "test";
    const isDev = mode === "development";

    if (!isProd && !isTest && !isDev) {
        throw new Error(`Unexpected vite mode: ${mode}`);
    }

    // eslint-disable-next-line no-console
    console.log(`Vite mode: ${mode}, prod=${isProd}, test=${isTest}, dev=${isDev}`);

    const assetsPath = genPath(conf.staticPath, {
        extra: ["dist"],
    });
    const assetsUrl = genPath(conf.staticUrl, {
        extra: ["dist"],
    });

    const nonFrontendGlobs = [...baseNonFrontendGlobs, ...(conf.extraNonFrontendGlobs || [])];
    const plugins: UserConfig["plugins"] = [
        preact({
            devToolsEnabled: !isProd,
            prefreshEnabled: !isProd,
        }),
        // The service worker / pwa stuff:
        VitePWA({
            registerType: "autoUpdate",
            workbox: {
                // Precache everything found in the vite dist folder:
                globDirectory: genPath(assetsPath),
                // Excluding html, we only have one root index.html and want that to always be fresh:
                globPatterns: ["**/*.{js,css,ico,png,svg}"],

                // Don't fallback on document based (e.g. `/some-page`) requests
                // Even though this says `null` by default, I had to set this specifically to `null` to make it work
                navigateFallback: null,

                // Tell the worker it should be finding all the files from the static domain, rather than the backend:
                modifyURLPrefix: {
                    "": genPath(assetsUrl),
                },
            },

            // Needs to come from the backend/same domain otherwise CORS won't work (classic worker problems)
            srcDir: genPath(assetsPath),
            outDir: genPath(conf.sameDomStaticPath, {
                extra: ["sworker"],
            }),

            // Note the mainfest.webmanifest is incorrectly placed in static/dist, but the links go the backend url.
            // Bug in the lib, haven't found a proper way to solve.
            // A secondary plugin below moves the file at the end.
            base: genPath(conf.sameDomStaticUrl, {
                extra: ["sworker"],
            }),
            buildBase: genPath(conf.sameDomStaticUrl, {
                extra: ["sworker"],
            }),

            // Puts the sw importer script directly in the index.html head, rather than a separate file:
            injectRegister: "inline",

            // Allowing the service worker to control the full stie rather than the dir it comes from:
            // this requires the Service-Worker-Allowed: "/" header to be passed from the worker serve directory on backend (which it is)
            scope: "/",

            manifest: {
                name: conf.siteName,
                short_name: conf.siteName,
                description: conf.siteDescription,
                theme_color: "#031033",
                background_color: "#031033",
                display: "standalone",
                start_url: "/",
                icons: [
                    {
                        src: conf.favicon192PngPath,
                        sizes: "192x192",
                        type: "image/png",
                        purpose: "any maskable",
                    },
                    {
                        src: conf.favicon512PngPath,
                        sizes: "512x512",
                        type: "image/png",
                        purpose: "any maskable",
                    },
                ],
            },
        }),
        {
            name: "move-webmanifest", // the name of your custom plugin. Could be anything.
            apply: "build",
            enforce: "post",
            closeBundle: async () => {
                const oldLoc = genPath(assetsPath, {
                    extra: ["manifest.webmanifest"],
                });
                const newLoc = genPath(conf.sameDomStaticPath, {
                    extra: ["sworker", "manifest.webmanifest"],
                });
                // eslint-disable-next-line no-console
                console.log(`Moving webmanifest from ${oldLoc} to ${newLoc}`);
                await fs.rename(oldLoc, newLoc);

                // Also copy and minify the index.html file to the same domain static dir, which is where nginx/backend expects it to be:
                const indexLoc = genPath(assetsPath, {
                    extra: ["index.html"],
                });
                // eslint-disable-next-line no-console
                console.log(`Reading index.html from ${oldLoc} and minifying...`);
                const indexSrc = await fs.readFile(indexLoc, "utf-8");
                const indexMinified = await minify(indexSrc, {
                    collapseBooleanAttributes: true,
                    collapseInlineTagWhitespace: true,
                    collapseWhitespace: true,
                    useShortDoctype: true,
                    removeComments: true,
                    minifyCSS: true,
                    minifyJS: true,
                });
                // Naming base rather than index to work more nicely with nginx:
                const newIndexLoc = genPath(conf.sameDomStaticPath, {
                    extra: ["base.html"],
                });
                // eslint-disable-next-line no-console
                console.log(
                    `Minified index.html from ${indexSrc.length} to ${indexMinified.length} bytes, writing to ${newIndexLoc}.`,
                );
                await fs.writeFile(newIndexLoc, indexMinified);
            },
        },
    ];

    // Can be monitor how code is transformed / plugins used at http://localhost:3000/__inspect
    if (isDev && conf.inspect) {
        plugins.push(Inspect());
    }

    // Don't add compile checks when testing to keep speedy:
    if (!isTest) {
        plugins.push(
            checker({
                typescript: true,
                enableBuild: true, // Want it to validate type checking on build too
                // Show errors in terminal & UI:
                terminal: true,
                overlay: true,
            }),
        );
    }

    const config: UserConfig = {
        clearScreen: false, // Don't clear screen when running vite commands
        plugins,
        resolve: {
            // Providing absolute paths:
            alias: conf.alias,
            // Fixes esm problem with rrd:
            // react-router-dom specifies "module" field in package.json for ESM entry
            // if it's not mapped, it uses the "main" field which is CommonJS that redirects to CJS preact
            mainFields: ["module"],
        },
        server: {
            // open: true, // Open the web page on start
            host: "localhost",
            port: 3000,
            watch: {
                // Don't check non frontend files:
                ignored: nonFrontendGlobs,
            },
            // Proxy all backend requests to django/fastapi:
            proxy: genBackendProxies(conf.proxy),
        },
        // When being served from django in production, js internal assets urls need to use url to the js assets inside the static url,
        // rather than in dev where they're just served from the root:
        base: genPath(isProd ? assetsUrl : "/"),
        // The production build config:
        build: {
            outDir: genPath(assetsPath),
        },
        define: {
            // Vite doesn't have process, so define the specifically needed ones directly:
            "process.env.NODE_ENV": JSON.stringify(mode),
        },

        // (Attempted!) size and perf optimisations:
        json: {
            stringify: true,
        },

        esbuild: {
            legalComments: "linked",
            exclude: nonFrontendGlobs,
        },
    };
    return defineConfig(config);
};
