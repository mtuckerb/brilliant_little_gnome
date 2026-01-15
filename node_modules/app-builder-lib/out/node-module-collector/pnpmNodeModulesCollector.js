"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.PnpmNodeModulesCollector = void 0;
const builder_util_1 = require("builder-util");
const path = require("path");
const nodeModulesCollector_1 = require("./nodeModulesCollector");
const packageManager_1 = require("./packageManager");
class PnpmNodeModulesCollector extends nodeModulesCollector_1.NodeModulesCollector {
    constructor() {
        super(...arguments);
        this.installOptions = {
            manager: packageManager_1.PM.PNPM,
            lockfile: "pnpm-lock.yaml",
        };
    }
    getArgs() {
        return ["list", "--prod", "--json", "--depth", "Infinity", "--long"];
    }
    async getProductionDependencies(depTree) {
        const packageName = depTree.name || depTree.from;
        if ((0, builder_util_1.isEmptyOrSpaces)(packageName)) {
            builder_util_1.log.error(depTree, `Cannot determine production dependencies for package with empty name`);
            throw new Error(`Cannot compute production dependencies for package with empty name: ${packageName}`);
        }
        const actualPath = await this.locatePackageVersion(depTree.path, packageName, depTree.version).then(it => it === null || it === void 0 ? void 0 : it.packageDir);
        const resolvedLocalPath = await this.cache.realPath[actualPath !== null && actualPath !== void 0 ? actualPath : depTree.path];
        const p = path.normalize(resolvedLocalPath);
        const pkgJsonPath = path.join(p, "package.json");
        let packageJson;
        try {
            packageJson = await this.cache.packageJson[pkgJsonPath];
        }
        catch (error) {
            builder_util_1.log.warn(null, `Failed to read package.json for ${p}: ${error.message}`);
            return { path: p, dependencies: {}, optionalDependencies: {} };
        }
        return { path: p, dependencies: { ...packageJson.dependencies }, optionalDependencies: { ...packageJson.optionalDependencies } };
    }
    async extractProductionDependencyGraph(tree, dependencyId) {
        var _a, _b, _c, _d;
        if (this.productionGraph[dependencyId]) {
            return;
        }
        this.productionGraph[dependencyId] = { dependencies: [] };
        const packageName = tree.name || tree.from;
        const treeDep = { ...(tree.dependencies || {}), ...(tree.optionalDependencies || {}) };
        const json = packageName === dependencyId ? null : await this.getProductionDependencies(tree);
        const prodDependencies = json ? { ...json.dependencies, ...json.optionalDependencies } : treeDep;
        const collectedDependencies = [];
        for (const packageName in treeDep) {
            if (!prodDependencies[packageName]) {
                continue;
            }
            // Then check if optional dependency path exists (using actual resolved path)
            const version = ((_a = json === null || json === void 0 ? void 0 : json.optionalDependencies) === null || _a === void 0 ? void 0 : _a[packageName]) || ((_c = (_b = tree.optionalDependencies) === null || _b === void 0 ? void 0 : _b[packageName]) === null || _c === void 0 ? void 0 : _c.version);
            const actualPath = await this.locatePackageVersion((_d = json === null || json === void 0 ? void 0 : json.path) !== null && _d !== void 0 ? _d : tree.path, packageName, version).then(it => it === null || it === void 0 ? void 0 : it.packageDir);
            if (actualPath == null || !(await this.cache.exists[actualPath])) {
                builder_util_1.log.debug({ packageName, version: version, searchPath: actualPath }, `optional dependency not installed, skipping`);
                continue;
            }
            const dependency = treeDep[packageName];
            const childDependencyId = this.packageVersionString(dependency);
            await this.extractProductionDependencyGraph(dependency, childDependencyId);
            collectedDependencies.push(childDependencyId);
        }
        this.productionGraph[dependencyId] = { dependencies: collectedDependencies };
    }
    async collectAllDependencies(tree) {
        // Collect regular dependencies
        for (const [key, value] of Object.entries(tree.dependencies || {})) {
            const json = await this.getProductionDependencies(value);
            this.allDependencies.set(`${key}@${value.version}`, { ...value, path: json.path });
            await this.collectAllDependencies(value);
        }
        // Collect optional dependencies if they exist
        for (const [key, value] of Object.entries(tree.optionalDependencies || {})) {
            const json = await this.getProductionDependencies(value);
            this.allDependencies.set(`${key}@${value.version}`, { ...value, path: json.path });
            await this.collectAllDependencies(value);
        }
    }
    packageVersionString(pkg) {
        // we use 'from' field because 'name' may be different in case of aliases
        return `${pkg.from}@${pkg.version}`;
    }
    async parseDependenciesTree(jsonBlob) {
        const dependencyTree = JSON.parse(jsonBlob);
        // pnpm returns an array of dependency trees
        return Promise.resolve(dependencyTree[0]);
    }
}
exports.PnpmNodeModulesCollector = PnpmNodeModulesCollector;
//# sourceMappingURL=pnpmNodeModulesCollector.js.map