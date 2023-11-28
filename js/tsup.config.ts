import type { Options } from "tsup";

const env = process.env.NODE_ENV;

// https://dev.to/orabazu/how-to-bundle-a-tree-shakable-typescript-library-with-tsup-and-publish-with-npm-3c46
export const tsup: Options = {
    splitting: true,
    clean: true, // clean up the dist folder
    dts: true, // generate dts files
    format: ["cjs", "esm"], // generate cjs and esm files
    minify: env === "production",
    bundle: env === "production",
    skipNodeModulesBundle: true,
    entryPoints: ["bitbazaar/index.ts"],
    watch: env === "development",
    target: "es2020",
    outDir: env === "production" ? "dist" : "lib",
    entry: ["bitbazaar/**/*.ts"], // look at all files in the project
};
