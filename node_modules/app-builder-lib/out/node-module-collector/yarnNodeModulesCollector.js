"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.YarnNodeModulesCollector = void 0;
const builder_util_1 = require("builder-util");
const lazy_val_1 = require("lazy-val");
const path = require("path");
const nodeModulesCollector_1 = require("./nodeModulesCollector");
const packageManager_1 = require("./packageManager");
class YarnNodeModulesCollector extends nodeModulesCollector_1.NodeModulesCollector {
    constructor() {
        super(...arguments);
        this.installOptions = {
            manager: packageManager_1.PM.YARN,
            lockfile: "yarn.lock",
        };
        this.isHoisted = new lazy_val_1.Lazy(async () => {
            return Promise.resolve(true); // Yarn Classic always hoists
        });
    }
    getArgs() {
        return ["list", "--production", "--json", "--depth=Infinity", "--no-progress"];
    }
    async extractProductionDependencyGraph(tree, dependencyId) {
        if (this.productionGraph[dependencyId]) {
            return;
        }
        const productionDeps = Object.entries(tree.dependencies || {}).map(async ([, dependency]) => {
            const dep = {
                ...dependency,
                path: await this.cache.realPath[dependency.path],
            };
            const childDependencyId = this.packageVersionString(dep);
            await this.extractProductionDependencyGraph(dep, childDependencyId);
            return childDependencyId;
        });
        const dependencies = [];
        for (const dep of productionDeps) {
            dependencies.push(await dep);
        }
        this.productionGraph[dependencyId] = { dependencies };
    }
    getDependencyType(pkgName, parentPkgJson) {
        var _a, _b, _c;
        if ((_a = parentPkgJson.optionalDependencies) === null || _a === void 0 ? void 0 : _a[pkgName]) {
            return "optional";
        }
        if ((_b = parentPkgJson.devDependencies) === null || _b === void 0 ? void 0 : _b[pkgName]) {
            return "dev";
        }
        if ((_c = parentPkgJson.dependencies) === null || _c === void 0 ? void 0 : _c[pkgName]) {
            return "prod";
        }
        return "prod";
    }
    async parseDependenciesTree(jsonBlob) {
        const lines = jsonBlob
            .split("\n")
            .map(l => l.trim())
            .filter(Boolean)
            .map(l => {
            try {
                return JSON.parse(l);
            }
            catch {
                return undefined;
            }
        })
            .filter(Boolean);
        const parsedTree = lines
            .filter(l => l.type === "tree")
            .map(l => l.data.trees)
            .shift();
        if (!parsedTree) {
            throw new Error('Failed to extract Yarn tree: no "type":"tree" line found in console output');
        }
        const rootPkgJson = await this.cache.packageJson[path.join(this.rootDir, "package.json")];
        const normalizedTree = await this.normalizeTree({ tree: parsedTree, seen: new Set(), appName: rootPkgJson.name, parentPath: this.rootDir, parentPkgJson: rootPkgJson });
        const dependencies = {};
        for (const [name, dep] of Object.entries(normalizedTree)) {
            dependencies[name] = dep;
        }
        return Promise.resolve({
            name: rootPkgJson.name,
            version: rootPkgJson.version,
            path: this.rootDir,
            dependencies,
            workspaces: rootPkgJson === null || rootPkgJson === void 0 ? void 0 : rootPkgJson.workspaces,
        });
    }
    async normalizeTree(options) {
        const { tree, seen, appName, parentPath = this.rootDir } = options;
        let parentPkgJson = options.parentPkgJson;
        const normalized = {};
        // Load parent's package.json if not provided
        if (!parentPkgJson && parentPath) {
            const parentPkgPath = path.join(parentPath, "package.json");
            try {
                parentPkgJson = await this.cache.packageJson[parentPkgPath];
            }
            catch {
                // Parent might not have package.json (e.g., root workspace)
            }
        }
        for (const node of tree) {
            const match = node.name.match(/^(.*)@([^@]+)$/);
            if (!match) {
                builder_util_1.log.debug({ name: node.name }, "invalid node name format");
                continue;
            }
            const [, pkgName, version] = match;
            const id = `${pkgName}@${version}`;
            const isShadow = node.shadow && node.color === "dim";
            if (isShadow) {
                builder_util_1.log.debug({ pkgName, version }, "registering shadow node (hoisted elsewhere), will resolve");
            }
            if (seen.has(id)) {
                continue;
            }
            // Find the correct package path that matches the required version
            const pkg = await this.locatePackageVersion(parentPath, pkgName, version);
            const pkgPath = pkg === null || pkg === void 0 ? void 0 : pkg.packageDir;
            if (!pkgPath) {
                builder_util_1.log.warn({ pkgName, version, parentPath }, "could not find package matching version");
                continue;
            }
            seen.add(id);
            const normalizedDep = {
                name: pkgName,
                version,
                path: pkgPath,
                dependencies: {},
                optionalDependencies: {},
            };
            // Recursively process children, passing this package's info
            if (node.children && node.children.length > 0) {
                const childPkgJson = await this.cache.packageJson[path.join(pkgPath, "package.json")];
                const childDeps = await this.normalizeTree({
                    tree: node.children,
                    seen,
                    appName,
                    parentPath: pkgPath,
                    parentPkgJson: childPkgJson, // Pass this package's package.json to children
                });
                for (const [childDepName, childDep] of Object.entries(childDeps)) {
                    normalizedDep.dependencies[childDepName] = childDep;
                }
            }
            normalized[pkgName] = normalizedDep;
        }
        return normalized;
    }
    async collectAllDependencies(tree, packageToExclude) {
        const rootPkgJson = await this.cache.packageJson[path.join(this.rootDir, "package.json")];
        const failedPackages = new Set();
        const collect = async (deps = {}, isOptionalDependency, parentIsOptional = false) => {
            var _a, _b, _c, _d;
            for (const [, value] of Object.entries(deps)) {
                const isRootOptional = !!((_a = rootPkgJson.optionalDependencies) === null || _a === void 0 ? void 0 : _a[value.name]);
                const isDirectRootDep = ((_b = rootPkgJson.dependencies) === null || _b === void 0 ? void 0 : _b[value.name]) || ((_c = rootPkgJson.optionalDependencies) === null || _c === void 0 ? void 0 : _c[value.name]) || ((_d = rootPkgJson.devDependencies) === null || _d === void 0 ? void 0 : _d[value.name]);
                const treatAsOptional = isOptionalDependency || parentIsOptional || isRootOptional;
                const logFields = { name: value.name, version: value.version, path: value.path };
                const p = await this.cache.realPath[value.path];
                if (!(await this.cache.exists[p])) {
                    if (treatAsOptional) {
                        builder_util_1.log.debug(logFields, "failed to find optional dependency, skipping");
                        failedPackages.add(value.name);
                        continue;
                    }
                    if (!isDirectRootDep) {
                        builder_util_1.log.debug(logFields, "failed to find transitive dependency, treating as optional");
                        failedPackages.add(value.name);
                        continue;
                    }
                    const message = "unable to find module directory; is the path correct?";
                    builder_util_1.log.error(logFields, message);
                    throw new Error(`Failed to resolve module directory for ${value.name}@${value.version} at path: ${value.path}`);
                }
                let resolvedVersion = value.version;
                const versionMatch = p.match(/[/\\]node_modules[/\\][^@]+@([^/\\]+)[/\\]/);
                if (versionMatch) {
                    resolvedVersion = versionMatch[1];
                    if (resolvedVersion !== value.version) {
                        builder_util_1.log.debug({ name: value.name, declared: value.version, resolved: resolvedVersion }, "resolved actual version from path");
                    }
                }
                const m = {
                    ...value,
                    version: resolvedVersion,
                    path: p,
                };
                const moduleKey = this.packageVersionString(m);
                if (this.allDependencies.has(moduleKey)) {
                    continue;
                }
                this.allDependencies.set(moduleKey, m);
                const childIsOptional = treatAsOptional;
                await collect(m.dependencies, false, childIsOptional);
                await collect(m.optionalDependencies, true, true);
            }
        };
        await collect(tree.dependencies, false, false);
        await collect(tree.optionalDependencies, true, true);
        // Final cleanup: remove the app package from allDependencies
        if (packageToExclude) {
            for (const [key, dep] of this.allDependencies.entries()) {
                if (dep.name === packageToExclude) {
                    builder_util_1.log.debug({ key, name: dep.name }, "removing app package from allDependencies");
                    for (const [, d] of Object.entries(dep.dependencies || {})) {
                        this.allDependencies.set(this.packageVersionString(d), d);
                    }
                    this.allDependencies.delete(key);
                }
            }
        }
        if (failedPackages.size > 0) {
            const cleanDependencies = (deps = {}) => {
                for (const [key, dep] of Object.entries(deps)) {
                    if (failedPackages.has(dep.name)) {
                        builder_util_1.log.debug({ name: dep.name }, "removing failed package from tree");
                        delete deps[key];
                    }
                    else {
                        if (dep.dependencies) {
                            cleanDependencies(dep.dependencies);
                        }
                        if (dep.optionalDependencies) {
                            cleanDependencies(dep.optionalDependencies);
                        }
                    }
                }
            };
            cleanDependencies(tree.dependencies);
            cleanDependencies(tree.optionalDependencies);
        }
    }
}
exports.YarnNodeModulesCollector = YarnNodeModulesCollector;
//# sourceMappingURL=yarnNodeModulesCollector.js.map