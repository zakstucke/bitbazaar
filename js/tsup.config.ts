import type { Options } from "tsup";

// https://dev.to/orabazu/how-to-bundle-a-tree-shakable-typescript-library-with-tsup-and-publish-with-npm-3c46
export const tsup: Options = {
    clean: true, // clean up the dist folder
    dts: true, // generate dts files
    format: ["esm"], // Only support ESM, fixes lots of import problems
    minify: false, // Again allow downstream consumers to minify
    bundle: false, // Don't bundle, allow downstream consumers to bundle
    skipNodeModulesBundle: true,
    target: "es2020",
    outDir: "./dist",
    entry: ["bitbazaar/**/*.ts"], // look at all files in the project
};
