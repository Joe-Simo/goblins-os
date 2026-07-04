import nextPlugin from "@next/eslint-plugin-next";
import nextParser from "eslint-config-next/parser";
import globals from "globals";

const eslintConfig = [
  {
    ignores: [".next/**", "out/**", "build/**", "next-env.d.ts"],
  },
  {
    name: "goblins-os-site/next",
    files: ["src/**/*.{ts,tsx}", "next.config.ts"],
    plugins: {
      "@next/next": nextPlugin,
    },
    languageOptions: {
      parser: nextParser,
      parserOptions: {
        requireConfigFile: false,
        sourceType: "module",
        allowImportExportEverywhere: true,
        babelOptions: {
          presets: ["next/babel"],
          caller: {
            supportsTopLevelAwait: true,
          },
        },
      },
      globals: {
        ...globals.browser,
        ...globals.node,
      },
    },
    settings: {
      next: {
        rootDir: ["."],
      },
    },
    rules: {
      ...nextPlugin.configs.recommended.rules,
      ...nextPlugin.configs["core-web-vitals"].rules,
    },
  },
];

export default eslintConfig;
