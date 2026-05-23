import { describe, expect, test } from "vitest";
import {
  applyNamespaceToPushArtifacts,
  extractTableNamesFromSchemaSource,
  prefixConvexTableReferencesInSource,
  resolveMultiProjectOptions,
  validateDeploymentNamespace,
} from "./components.js";

describe("multi-project deployment helpers", () => {
  test("resolves CLI options before convex.json config", () => {
    expect(
      resolveMultiProjectOptions(
        { namespace: "CliNamespace", projectId: "cli-project" },
        {
          functions: "convex/",
          namespace: "ConfigNamespace",
          projectId: "config-project",
        },
      ),
    ).toEqual({
      namespace: "CliNamespace",
      projectId: "cli-project",
    });
  });

  test("uses config values and defaults projectId to functions directory", () => {
    expect(
      resolveMultiProjectOptions(
        {},
        {
          functions: "convex/",
          namespace: "Catalog",
        },
      ),
    ).toEqual({
      namespace: "Catalog",
      projectId: "convex/",
    });

    expect(
      resolveMultiProjectOptions(
        {},
        {
          functions: "convex/",
        },
      ),
    ).toEqual({});
  });

  test("validates namespace syntax", () => {
    expect(validateDeploymentNamespace("Catalog_V2")).toBeNull();
    expect(validateDeploymentNamespace("2Catalog")).toContain(
      "Invalid namespace",
    );
    expect(validateDeploymentNamespace("Catalog-Service")).toContain(
      "Invalid namespace",
    );
    expect(validateDeploymentNamespace("A".repeat(65))).toContain(
      "Invalid namespace",
    );
  });

  test("extracts table names from schema source", () => {
    const source = `
      export default defineSchema({
        products: defineTable({}),
        "price_history": defineTable({}),
        'categories': defineTable({}),
        _internal: defineTable({}),
      });
      const notATable = "reviews: defineTable({})";
    `;

    expect(extractTableNamesFromSchemaSource(source)).toEqual([
      "products",
      "price_history",
      "categories",
    ]);
  });

  test("prefixes exact table string literals", () => {
    const source = `
      await ctx.db.insert("products", {});
      await ctx.db.query('categories').collect();
      const untouched = "product";
      const alreadyPrefixed = "Catalog/products";
    `;

    expect(
      prefixConvexTableReferencesInSource(source, "Catalog", [
        "products",
        "categories",
      ]),
    ).toContain('ctx.db.insert("Catalog/products", {})');
    expect(
      prefixConvexTableReferencesInSource(source, "Catalog", [
        "products",
        "categories",
      ]),
    ).toContain("ctx.db.query('Catalog/categories')");
    expect(
      prefixConvexTableReferencesInSource(source, "Catalog", [
        "products",
        "categories",
      ]),
    ).toContain('const untouched = "product"');
    expect(
      prefixConvexTableReferencesInSource(source, "Catalog", [
        "products",
        "categories",
      ]),
    ).toContain('const alreadyPrefixed = "Catalog/products"');
  });

  test("applies namespace to deploy artifacts", () => {
    const appSchema = {
      source: 'const tableName = "products";',
    };
    const changedModules = [
      {
        path: "products.js",
        source: 'await ctx.db.query("products").collect();',
      },
      {
        path: "_deps/chunk.js",
        source: 'const tableName = "products";',
      },
      {
        path: "http.js",
        source: "",
      },
    ];
    const unchangedModuleHashes = [
      {
        path: "categories.js",
      },
      {
        path: "crons.js",
      },
    ];

    applyNamespaceToPushArtifacts({
      namespace: "Catalog",
      tableNames: ["products"],
      appSchema,
      changedModules,
      unchangedModuleHashes,
    });

    expect(appSchema.source).toContain('"Catalog/products"');
    expect(changedModules).toMatchObject([
      {
        path: "Catalog/products.js",
        source: 'await ctx.db.query("Catalog/products").collect();',
      },
      {
        path: "Catalog/_deps/chunk.js",
        source: 'const tableName = "products";',
      },
      {
        path: "http.js",
      },
    ]);
    expect(unchangedModuleHashes).toEqual([
      {
        path: "Catalog/categories.js",
      },
      {
        path: "crons.js",
      },
    ]);
  });
});
