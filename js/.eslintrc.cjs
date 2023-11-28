module.exports = {
    root: true,
    parser: "@typescript-eslint/parser",
    parserOptions: {
        ecmaVersion: 2020,
        sourceType: "module",
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
    extends: ["eslint:recommended"],
    plugins: ["@typescript-eslint"],
};
