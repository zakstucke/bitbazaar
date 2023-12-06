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
    plugins: [
        require.resolve("@trivago/prettier-plugin-sort-imports"),
        require.resolve("prettier-plugin-tailwindcss"),
    ],
    importOrderSeparation: true,
    importOrderSortSpecifiers: true,
    importOrderCaseInsensitive: true,
    importOrder: [
        "<THIRD_PARTY_MODULES>",
        "@root(.*)$",
        "@scripts(.*)$",
        "^..$", // index
        // Two layers up if not a css file:
        "^../(?!.*.(scss|css|less)$)(.*)$",
        "^.$", // index
        // One layer up if a css file:
        "^./(?!.*.(scss|css|less)$)(.*)$",
        // Non module css: (check no .module. in the name)
        "^.*(?<!\\.module)\\.(scss|css|less)$",
        // Then module css:
        "^.*\\.module\\.(scss|css|less)$",
    ],
};
