/** @type {import("prettier").Config} */
module.exports = {
    printWidth: 100,
    tabWidth: 4,
    useTabs: false,
    semi: true,
    singleQuote: false,
    trailingComma: "all",
    bracketSpacing: true,
    arrowParens: "always",
    plugins: [require.resolve("@trivago/prettier-plugin-sort-imports")],
    importOrderSeparation: true,
    importOrderSortSpecifiers: true,
    importOrderCaseInsensitive: true,
    importOrder: ["<THIRD_PARTY_MODULES>", "^../(.*)$", "^[./]", "(?=./styles.module.scss)"],
};
