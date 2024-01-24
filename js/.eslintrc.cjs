module.exports = {
    root: true,
    parser: "@typescript-eslint/parser",
    parserOptions: {
        ecmaVersion: 2020,
        sourceType: "module",
        project: "./tsconfig.json",
    },
    settings: {
        "import/resolver": {
            node: {
                paths: ["./bitbazaar"],
                extensions: [".js", ".cjs", "jsx", ".ts", ".tsx", ".d.ts"],
            },
        },
    },
    env: {
        browser: true,
        amd: true,
        node: true,
    },
    extends: [
        "eslint:recommended",
        "plugin:@typescript-eslint/recommended-requiring-type-checking",
        "plugin:react-hooks/recommended",
    ],
    plugins: ["@typescript-eslint"],
    ignorePatterns: [
        "**/*.spec.(js|jsx|ts|tsx)",
        "**/*.test.(js|jsx|ts|tsx)",
        "tests/**/*",
        "node_modules/**/*",
        "dist/**/*",
        "lib/**/*",
        "**/*.zetch.*",
        "**/*.zetch",
    ],
    rules: {
        // The js version has false positives in typescript:
        "no-unused-vars": "off",
        "@typescript-eslint/no-unused-vars": ["error"],

        // Ignore empty pattern:
        "no-empty-pattern": "off",

        // Allow explicity any, opt in should be allowed as commonly used:
        "@typescript-eslint/no-explicit-any": "off",

        // Error by default on console.log to prevent accidentally including when temp debugging:
        "no-console": "error",
    },
};
