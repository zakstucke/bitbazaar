import type { Options } from "tsup";

// https://dev.to/orabazu/how-to-bundle-a-tree-shakable-typescript-library-with-tsup-and-publish-with-npm-3c46
export const tsup: Options = {
    splitting: true,
    clean: true, // clean up the dist folder
    dts: true, // generate dts files
    format: ["cjs", "esm"], // generate cjs and esm files
    minify: true,
    bundle: false,
    skipNodeModulesBundle: true,
    entryPoints: ["bitbazaar/index.ts"],
    watch: false,
    target: "es2020",
    outDir: "./dist",
    entry: ["bitbazaar/**/*.ts"], // look at all files in the project
    esbuildOptions(options, context) {
        // the directory structure will be the same as the source
        options.outbase = "./";
    },
};
