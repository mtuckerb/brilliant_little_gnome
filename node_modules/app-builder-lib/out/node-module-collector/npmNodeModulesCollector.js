"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.NpmNodeModulesCollector = void 0;
const builder_util_1 = require("builder-util");
const path = require("path");
const nodeModulesCollector_1 = require("./nodeModulesCollector");
const packageManager_1 = require("./packageManager");
class NpmNodeModulesCollector extends nodeModulesCollector_1.NodeModulesCollector {
    constructor() {
        super(...arguments);
        this.installOptions = {
            manager: packageManager_1.PM.NPM,
            lockfile: "package-lock.json",
        };
    }
    getArgs() {
        return ["list", "-a", "--include", "prod", "--include", "optional", "--omit", "dev", "--json", "--long", "--silent"];
    }
    async getDependenciesTree(pm) {
        try {
            return await super.getDependenciesTree(pm);
        }
        catch (error) {
            builder_util_1.log.info({ pm: this.installOptions.manager, parser: packageManager_1.PM.NPM, error: error.message }, "unable to process dependency tree, falling back to using manual node_modules traversal");
        }
        // node_modules linker fallback. (Slower due to system ops, so we only use it as a fallback) [such as when corepack env will not allow npm CLI to extract tree]
        return this.buildNodeModulesTreeManually(this.rootDir);
    }
    async collectAllDependencies(tree) {
        for (const [, value] of Object.entries(tree.dependencies || {})) {
            if (this.isDuplicatedNpmDependency(value)) {
                continue;
            }
            this.allDependencies.set(this.packageVersionString(value), value);
            await this.collectAllDependencies(value);
        }
    }
    async extractProductionDependencyGraph(tree, dependencyId) {
        var _a;
        if (this.productionGraph[dependencyId]) {
            return;
        }
        const isDuplicateDep = this.isDuplicatedNpmDependency(tree);
        const resolvedDeps = isDuplicateDep ? (_a = this.allDependencies.get(dependencyId)) === null || _a === void 0 ? void 0 : _a.dependencies : tree.dependencies;
        // Initialize with empty dependencies array first to mark this dependency as "in progress"
        // After initialization, if there are libraries with the same name+version later, they will not be searched recursively again
        // This will prevents infinite loops when circular dependencies are encountered.
        this.productionGraph[dependencyId] = { dependencies: [] };
        const collectedDependencies = [];
        if (resolvedDeps && Object.keys(resolvedDeps).length > 0) {
            for (const packageName in resolvedDeps) {
                if (!this.isProdDependency(packageName, tree)) {
                    continue;
                }
                const dependency = resolvedDeps[packageName];
                const childDependencyId = this.packageVersionString(dependency);
                await this.extractProductionDependencyGraph(dependency, childDependencyId);
                collectedDependencies.push(childDependencyId);
            }
        }
        this.productionGraph[dependencyId] = { dependencies: collectedDependencies };
    }
    // Check: is package already included as a prod dependency due to another package?
    // We need to check this to prevent infinite loops in case of duplicated dependencies
    isDuplicatedNpmDependency(tree) {
        const { _dependencies = {}, dependencies = {} } = tree;
        const isDuplicateDep = Object.keys(_dependencies).length > 0 && Object.keys(dependencies).length === 0;
        return isDuplicateDep;
    }
    isProdDependency(packageName, tree) {
        var _a;
        return ((_a = tree._dependencies) === null || _a === void 0 ? void 0 : _a[packageName]) != null;
    }
    /**
     * Builds a dependency tree using only package.json dependencies and optionalDependencies.
     * This skips devDependencies and uses Node.js module resolution (require.resolve).
     */
    buildNodeModulesTreeManually(baseDir) {
        // Track visited packages by their resolved path to prevent infinite loops
        const visited = new Set();
        /**
         * Recursively builds dependency tree starting from a package directory.
         */
        const buildFromPackage = async (packageDir) => {
            const pkgPath = path.join(packageDir, "package.json");
            builder_util_1.log.debug({ pkgPath }, "building dependency node from package.json");
            if (!(await this.cache.exists[pkgPath])) {
                throw new Error(`package.json not found at ${pkgPath}`);
            }
            const pkg = await this.cache.packageJson[pkgPath];
            const resolvedPackageDir = await this.cache.realPath[packageDir];
            // Use resolved path as the unique identifier to prevent circular dependencies
            if (visited.has(resolvedPackageDir)) {
                builder_util_1.log.debug({ name: pkg.name, version: pkg.version, path: resolvedPackageDir }, "skipping already visited package");
                return {
                    name: pkg.name,
                    version: pkg.version,
                    path: resolvedPackageDir,
                };
            }
            visited.add(resolvedPackageDir);
            const prodDeps = {};
            const allProdDepNames = {
                ...pkg.dependencies,
                ...pkg.optionalDependencies,
            };
            // Process all production and optional dependencies
            for (const [depName, depVersion] of Object.entries(allProdDepNames)) {
                try {
                    const depPath = await this.locatePackageVersion(resolvedPackageDir, depName, depVersion);
                    if (!depPath || depPath.packageDir.length === 0) {
                        builder_util_1.log.warn({ package: pkg.name, dependency: depName, version: depVersion }, "dependency not found, skipping");
                        continue;
                    }
                    const resolvedDepPath = await this.cache.realPath[depPath.packageDir];
                    const logFields = { package: pkg.name, dependency: depName, resolvedPath: resolvedDepPath };
                    // Skip if this dependency resolves to the base directory or any parent we're already processing
                    if (resolvedDepPath === resolvedPackageDir || resolvedDepPath === (await this.cache.realPath[baseDir])) {
                        builder_util_1.log.debug(logFields, "skipping self-referential dependency");
                        continue;
                    }
                    builder_util_1.log.debug(logFields, "processing production dependency");
                    // Recursively build the dependency tree for this dependency
                    prodDeps[depName] = await buildFromPackage(resolvedDepPath);
                }
                catch (error) {
                    builder_util_1.log.warn({ package: pkg.name, dependency: depName, error: error.message }, "failed to process dependency, skipping");
                }
            }
            return {
                name: pkg.name,
                version: pkg.version,
                path: resolvedPackageDir,
                dependencies: Object.keys(prodDeps).length > 0 ? prodDeps : undefined,
                optionalDependencies: pkg.optionalDependencies,
            };
        };
        return buildFromPackage(baseDir);
    }
    async parseDependenciesTree(jsonBlob) {
        return Promise.resolve(JSON.parse(jsonBlob));
    }
}
exports.NpmNodeModulesCollector = NpmNodeModulesCollector;
//# sourceMappingURL=npmNodeModulesCollector.js.map