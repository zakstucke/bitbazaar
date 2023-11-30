import type { Options } from "tsup";

const env = process.env.NODE_ENV;

// https://dev.to/orabazu/how-to-bundle-a-tree-shakable-typescript-library-with-tsup-and-publish-with-npm-3c46
export const tsup: Options = {
    clean: true, // clean up the dist folder
    dts: true, // generate dts files
    format: ["cjs", "esm"], // generate cjs and esm files
    minify: true,
    // This ones important as it fixes .js and .cjs etc import file endings which wouldn't otherwise.
    // It also compiles all nested files into the specific entrypoints, in our case just the main entry and top level submodules, which are all that should be imported.
    bundle: true,
    skipNodeModulesBundle: true,
    target: "es2020",
    outDir: "dist",
    // Only include the top level entry and submodule entries:
    entry: ["bitbazaar/index.ts", "bitbazaar/*/index.ts"],
};
