import { defineConfig } from "vitest/config";

import tsconfig from "./tsconfig.json";

// Default globs to exclude from test and coverage, important dist/lib is in here to prevent trying to run on compiled output:
const excludeGlobs: string[] = [
    "**/.git/**",
    "**/venv/**",
    "**/.venv/**",
    "**/node_modules/**",
    "**/dist/**",
    "**/lib/**",
    "**/cypress/**",
    "**/coverage/**",
    "**/.eslintrc.*/**",
    "**/*.etch.*",
    "**/.{idea,git,cache,output,temp,mypy_cache,pytype,pytest,pyright}/**",
    "**/{karma,rollup,webpack,vite,vitest,jest,ava,babel,nyc,cypress,tsup,build}.config.*",
];

export default defineConfig({
    resolve: {
        // @ts-expect-error
        alias: tsconfig.compilerOptions.paths,
    },
    test: {
        environment: "happy-dom",
        setupFiles: [
            // All internal setup, polyfills and mocks etc:
            "./tests/setupTests.ts",
        ],
        coverage: {
            provider: "istanbul",
            all: true,
            lines: 100,
            functions: 100,
            branches: 100,
            statements: 100,
            exclude: excludeGlobs,
        },
        testTimeout: 15000,
        exclude: excludeGlobs,
    },
});
