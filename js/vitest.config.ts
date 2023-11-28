import { defineConfig } from "vitest/config";

const nonFrontendGlobs: string[] = [
    "**/.git/**",
    "**/venv/**",
    "**/.venv/**",
    "**/node_modules/**",
    "**/dist/**",
    "**/cypress/**",
    "**/coverage/**",
    "**/.{idea,git,cache,output,temp,mypy_cache,pytype,pytest,pyright}/**",
    "**/{karma,rollup,webpack,vite,vitest,jest,ava,babel,nyc,cypress,tsup,build}.config.*",
];

export default defineConfig({
    test: {
        environment: "happy-dom",
        setupFiles: [
            // All internal setup, polyfills and mocks etc:
            "./tests/setupTests.ts",
        ],
        coverage: {
            provider: "v8",
            all: true,
            lines: 100,
            functions: 100,
            branches: 100,
            statements: 100,
        },
        testTimeout: 15000,
        // Not sure if this does anything, but maybe makes loading faster:
        exclude: nonFrontendGlobs,
    },
});
