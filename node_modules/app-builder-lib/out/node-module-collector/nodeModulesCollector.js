"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.NodeModulesCollector = void 0;
const builder_util_1 = require("builder-util");
const childProcess = require("child_process");
const fs = require("fs-extra");
const fs_extra_1 = require("fs-extra");
const lazy_val_1 = require("lazy-val");
const path = require("path");
const semver = require("semver");
const hoist_1 = require("./hoist");
const moduleCache_1 = require("./moduleCache");
const packageManager_1 = require("./packageManager");
class NodeModulesCollector {
    constructor(rootDir, tempDirManager) {
        this.rootDir = rootDir;
        this.tempDirManager = tempDirManager;
        this.nodeModules = [];
        this.allDependencies = new Map();
        this.productionGraph = {};
        this.cache = new moduleCache_1.ModuleCache();
        this.isHoisted = new lazy_val_1.Lazy(async () => {
            const { manager } = this.installOptions;
            const command = (0, packageManager_1.getPackageManagerCommand)(manager);
            const config = (await this.asyncExec(command, ["config", "list"])).stdout;
            if (config == null) {
                builder_util_1.log.debug({ manager }, "unable to determine if node_modules are hoisted: no config output. falling back to hoisted mode");
                return false;
            }
            const lines = Object.fromEntries(config.split("\n").map(line => line.split("=").map(s => s.trim())));
            if (lines["node-linker"] === "hoisted") {
                builder_util_1.log.debug({ manager }, "node_modules are hoisted");
                return true;
            }
            return false;
        });
    }
    async getNodeModules({ cancellationToken, packageName }) {
        const tree = await this.getDependenciesTree(this.installOptions.manager);
        if (cancellationToken.cancelled) {
            throw new Error("getNodeModules cancelled after fetching dependency tree");
        }
        await this.collectAllDependencies(tree, packageName);
        const realTree = await this.getTreeFromWorkspaces(tree, packageName);
        await this.extractProductionDependencyGraph(realTree, packageName);
        if (cancellationToken.cancelled) {
            throw new Error("getNodeModules cancelled after building production graph");
        }
        const hoisterResult = (0, hoist_1.hoist)(this.transformToHoisterTree(this.productionGraph, packageName), {
            check: builder_util_1.log.isDebugEnabled,
        });
        await this._getNodeModules(hoisterResult.dependencies, this.nodeModules);
        builder_util_1.log.debug({ packageName, depCount: this.nodeModules.length }, "node modules collection complete");
        return this.nodeModules;
    }
    async getDependenciesTree(pm) {
        const command = (0, packageManager_1.getPackageManagerCommand)(pm);
        const args = this.getArgs();
        const tempOutputFile = await this.tempDirManager.getTempFile({
            prefix: path.basename(command, path.extname(command)),
            suffix: "output.json",
        });
        return (0, builder_util_1.retry)(async () => {
            await this.streamCollectorCommandToFile(command, args, this.rootDir, tempOutputFile);
            const shellOutput = await fs.readFile(tempOutputFile, { encoding: "utf8" });
            return await this.parseDependenciesTree(shellOutput.trim());
        }, {
            retries: 1,
            interval: 2000,
            backoff: 2000,
            shouldRetry: async (error) => {
                var _a;
                const logFields = { error: error.message, tempOutputFile, cwd: this.rootDir };
                if (!(await this.cache.exists[tempOutputFile])) {
                    builder_util_1.log.debug(logFields, "dependency tree output file missing, retrying");
                    return true;
                }
                const fileContent = await fs.readFile(tempOutputFile, { encoding: "utf8" });
                const fields = { ...logFields, fileContent };
                if (fileContent.trim().length === 0) {
                    builder_util_1.log.debug(fields, "dependency tree output file empty, retrying");
                    return true;
                }
                if ((_a = error.message) === null || _a === void 0 ? void 0 : _a.includes("Unexpected end of JSON input")) {
                    builder_util_1.log.debug(fields, "JSON parse error in dependency tree, retrying");
                    return true;
                }
                builder_util_1.log.error(fields, "error parsing dependencies tree");
                return false;
            },
        });
    }
    cacheKey(pkg) {
        const rel = path.relative(this.rootDir, pkg.path);
        return `${pkg.name}::${pkg.version}::${rel !== null && rel !== void 0 ? rel : "."}`;
    }
    packageVersionString(pkg) {
        return `${pkg.name}@${pkg.version}`;
    }
    isProdDependency(depName, pkg) {
        const prodDeps = { ...pkg.dependencies, ...pkg.optionalDependencies };
        return prodDeps[depName] != null;
    }
    /**
     * Parse a dependency identifier like "@scope/pkg@1.2.3" or "pkg@1.2.3"
     */
    parseNameVersion(identifier) {
        const lastAt = identifier.lastIndexOf("@");
        if (lastAt <= 0) {
            // fallback for scoped packages or malformed strings
            return { name: identifier, version: "unknown" };
        }
        const name = identifier.slice(0, lastAt);
        const version = identifier.slice(lastAt + 1);
        return { name, version };
    }
    async getTreeFromWorkspaces(tree, packageName) {
        var _a;
        if (!(tree.workspaces && tree.dependencies)) {
            return tree;
        }
        if ((_a = tree.dependencies) === null || _a === void 0 ? void 0 : _a[packageName]) {
            const { name, path, dependencies } = tree.dependencies[packageName];
            builder_util_1.log.debug({ name, path, dependencies: JSON.stringify(dependencies) }, "pruning root app/self reference from workspace tree");
            for (const [name, pkg] of Object.entries(dependencies !== null && dependencies !== void 0 ? dependencies : {})) {
                tree.dependencies[name] = pkg;
                this.allDependencies.set(this.packageVersionString(pkg), pkg);
            }
            delete tree.dependencies[packageName];
        }
        return Promise.resolve(tree);
    }
    transformToHoisterTree(obj, key, nodes = new Map()) {
        let node = nodes.get(key);
        const { name, version } = this.parseNameVersion(key);
        if (!node) {
            node = {
                name,
                identName: name,
                reference: version,
                dependencies: new Set(),
                peerNames: new Set(),
            };
            nodes.set(key, node);
            const deps = (obj[key] || {}).dependencies || [];
            for (const dep of deps) {
                const child = this.transformToHoisterTree(obj, dep, nodes);
                node.dependencies.add(child);
            }
        }
        return node;
    }
    async _getNodeModules(dependencies, result) {
        var _a;
        if (dependencies.size === 0) {
            return;
        }
        for (const d of dependencies.values()) {
            const reference = [...d.references][0];
            const p = (_a = this.allDependencies.get(`${d.name}@${reference}`)) === null || _a === void 0 ? void 0 : _a.path;
            if (p === undefined) {
                builder_util_1.log.warn({ name: d.name, reference }, "cannot find path for dependency");
                continue;
            }
            // fix npm list issue
            // https://github.com/npm/cli/issues/8535
            if (!(await this.cache.exists[p])) {
                builder_util_1.log.debug({ name: d.name, reference, p }, "dependency path does not exist");
                continue;
            }
            const node = {
                name: d.name,
                version: reference,
                dir: await this.cache.realPath[p],
            };
            result.push(node);
            if (d.dependencies.size > 0) {
                node.dependencies = [];
                await this._getNodeModules(d.dependencies, node.dependencies);
            }
        }
        result.sort((a, b) => a.name.localeCompare(b.name));
    }
    async asyncExec(command, args, cwd = this.rootDir) {
        const file = await this.tempDirManager.getTempFile({ prefix: "exec-", suffix: ".txt" });
        try {
            await this.streamCollectorCommandToFile(command, args, cwd, file);
            const result = await fs.readFile(file, { encoding: "utf8" });
            return { stdout: result === null || result === void 0 ? void 0 : result.trim(), stderr: undefined };
        }
        catch (error) {
            builder_util_1.log.debug({ error: error.message }, "failed to execute command");
            return { stdout: undefined, stderr: error.message };
        }
    }
    async streamCollectorCommandToFile(command, args, cwd, tempOutputFile) {
        const execName = path.basename(command, path.extname(command));
        const isWindowsScriptFile = process.platform === "win32" && path.extname(command).toLowerCase() === ".cmd";
        if (isWindowsScriptFile) {
            // If the command is a Windows script file (.cmd), we need to wrap it in a .bat file to ensure it runs correctly with cmd.exe
            // This is necessary because .cmd files are not directly executable in the same way as .bat files.
            // We create a temporary .bat file that calls the .cmd file with the provided arguments. The .bat file will be executed by cmd.exe.
            // Note: This is a workaround for Windows command execution quirks for specifically when `shell: false`
            const tempBatFile = await this.tempDirManager.getTempFile({
                prefix: execName,
                suffix: ".bat",
            });
            const batScript = `@echo off\r\n"${command}" %*\r\n`; // <-- CRLF required for .bat
            await fs.writeFile(tempBatFile, batScript, { encoding: "utf8" });
            command = "cmd.exe";
            args = ["/c", tempBatFile, ...args];
        }
        await new Promise((resolve, reject) => {
            const outStream = (0, fs_extra_1.createWriteStream)(tempOutputFile);
            const child = childProcess.spawn(command, args, {
                cwd,
                env: { COREPACK_ENABLE_STRICT: "0", ...process.env }, // allow `process.env` overrides
                shell: false, // required to prevent console logs polution from shell profile loading when `true`
            });
            let stderr = "";
            child.stdout.pipe(outStream);
            child.stderr.on("data", chunk => {
                stderr += chunk.toString();
            });
            child.on("error", err => {
                reject(new Error(`Node module collector spawn failed: ${err.message}`));
            });
            child.on("close", code => {
                outStream.close();
                // https://github.com/npm/npm/issues/17624
                const shouldIgnore = code === 1 && "npm" === execName.toLowerCase() && args.includes("list");
                if (shouldIgnore) {
                    builder_util_1.log.debug(null, "`npm list` returned non-zero exit code, but it MIGHT be expected (https://github.com/npm/npm/issues/17624). Check stderr for details.");
                }
                if (stderr.length > 0) {
                    builder_util_1.log.debug({ stderr }, "note: there was node module collector output on stderr");
                }
                const shouldResolve = code === 0 || shouldIgnore;
                return shouldResolve ? resolve() : reject(new Error(`Node module collector process exited with code ${code}:\n${stderr}`));
            });
        });
    }
    async locatePackageVersion(parentDir, pkgName, requiredRange) {
        // 1) check direct parent node_modules/pkgName first
        const direct = path.join(path.resolve(parentDir), "node_modules", pkgName, "package.json");
        if (await this.cache.exists[direct]) {
            const ver = await this.readPackageVersion(direct);
            if (ver && this.semverSatisfies(ver, requiredRange)) {
                return { packageDir: path.dirname(direct), version: ver };
            }
        }
        // 2) upward hoisted search, then 3) downward non-hoisted search
        return (await this.upwardSearch(parentDir, pkgName, requiredRange)) || (await this.downwardSearch(parentDir, pkgName, requiredRange)) || null;
    }
    async readPackageVersion(pkgJsonPath) {
        return await this.cache.packageJson[pkgJsonPath].then(pkg => pkg.version).catch(() => null);
    }
    semverSatisfies(found, range) {
        if (!range || range === "*" || range === "") {
            return true;
        }
        if (range === found) {
            return true;
        }
        if (semver.validRange(range) == null) {
            // ignore, we can't verify non-semver ranges
            // e.g. git urls, file:, patch:, etc. Example:
            // "@ai-sdk/google": "patch:@ai-sdk/google@npm%3A2.0.43#~/.yarn/patches/@ai-sdk-google-npm-2.0.43-689ed559b3.patch"
            builder_util_1.log.debug({ found, range }, "unable to validate semver version range, assuming match");
            return true;
        }
        try {
            return semver.satisfies(found, range);
        }
        catch {
            // fallback: simple equality or basic prefix handling (^, ~)
            if (range.startsWith("^") || range.startsWith("~")) {
                const r = range.slice(1);
                return r === found;
            }
            // if range is like "8.x" or "8.*" match major
            const m = range.match(/^(\d+)[.(*|x)]*/);
            const fm = found.match(/^(\d+)\./);
            if (m && fm) {
                return m[1] === fm[1];
            }
            return false;
        }
    }
    /**
     * Upward search (hoisted)
     */
    async upwardSearch(parentDir, pkgName, requiredRange) {
        let current = path.resolve(parentDir);
        const root = path.parse(current).root;
        while (true) {
            const candidate = path.join(current, "node_modules", pkgName, "package.json");
            if (await this.cache.exists[candidate]) {
                const ver = await this.readPackageVersion(candidate);
                if (ver && this.semverSatisfies(ver, requiredRange)) {
                    return { packageDir: path.dirname(candidate), version: ver };
                }
                // otherwise keep searching upward (we may find a different hoisted version)
            }
            if (current === root) {
                break;
            }
            const parent = path.dirname(current);
            if (parent === current) {
                break;
            }
            current = parent;
        }
        return null;
    }
    /**
     * Breadth-first downward search from parentDir/node_modules
     * Looks for node_modules/\*\/node_modules/pkgName (and deeper)
     */
    async downwardSearch(parentDir, pkgName, requiredRange, maxExplored = 2000, maxDepth = 6) {
        const start = path.join(path.resolve(parentDir), "node_modules");
        if (!(await this.cache.exists[start]) || !(await this.cache.lstat[start]).isDirectory()) {
            return null;
        }
        const visited = new Set();
        const queue = [{ dir: start, depth: 0 }];
        let explored = 0;
        while (queue.length > 0) {
            const { dir, depth } = queue.shift();
            if (explored++ > maxExplored) {
                break;
            }
            if (depth > maxDepth) {
                continue;
            }
            let entries;
            try {
                entries = await fs.readdir(dir);
            }
            catch {
                continue;
            }
            for (const entry of entries) {
                if (entry.startsWith(".")) {
                    continue;
                }
                const entryPath = path.join(dir, entry);
                // handle scoped packages @scope/name
                if (entry.startsWith("@")) {
                    // queue the scope directory itself to explore its children
                    if ((await this.cache.exists[entryPath]) && (await this.cache.lstat[entryPath]).isDirectory()) {
                        const scopeEntries = await fs.readdir(entryPath);
                        for (const sc of scopeEntries) {
                            const scPath = path.join(entryPath, sc);
                            // check scPath/node_modules/pkgName
                            const candidatePkgJson = path.join(scPath, "node_modules", pkgName, "package.json");
                            if (await this.cache.exists[candidatePkgJson]) {
                                const ver = await this.readPackageVersion(candidatePkgJson);
                                if (ver && this.semverSatisfies(ver, requiredRange)) {
                                    return { packageDir: path.dirname(candidatePkgJson), version: ver };
                                }
                            }
                            // enqueue scPath/node_modules to explore further
                            const scNodeModules = path.join(scPath, "node_modules");
                            if ((await this.cache.exists[scNodeModules]) && (await this.cache.lstat[scNodeModules]).isDirectory()) {
                                if (!visited.has(scNodeModules)) {
                                    visited.add(scNodeModules);
                                    queue.push({ dir: scNodeModules, depth: depth + 1 });
                                }
                            }
                        }
                    }
                    continue;
                }
                // check for direct candidate: entry/node_modules/pkgName
                try {
                    const stat = await this.cache.lstat[entryPath];
                    if (!stat.isDirectory()) {
                        continue;
                    }
                }
                catch {
                    continue;
                }
                const candidatePkgJson = path.join(entryPath, "node_modules", pkgName, "package.json");
                if (await this.cache.exists[candidatePkgJson]) {
                    const ver = await this.readPackageVersion(candidatePkgJson);
                    if (ver && this.semverSatisfies(ver, requiredRange)) {
                        return { packageDir: path.dirname(candidatePkgJson), version: ver };
                    }
                }
                // also check entry/node_modules directly for pkgName (some layouts)
                const candidateDirect = path.join(entryPath, pkgName, "package.json");
                if (await this.cache.exists[candidateDirect]) {
                    const ver = await this.readPackageVersion(candidateDirect);
                    if (ver && this.semverSatisfies(ver, requiredRange)) {
                        return { packageDir: path.dirname(candidateDirect), version: ver };
                    }
                }
                // enqueue entry/node_modules for deeper traversal
                const nextNodeModules = path.join(entryPath, "node_modules");
                if ((await this.cache.exists[nextNodeModules]) && (await this.cache.lstat[nextNodeModules]).isDirectory()) {
                    if (!visited.has(nextNodeModules)) {
                        visited.add(nextNodeModules);
                        queue.push({ dir: nextNodeModules, depth: depth + 1 });
                    }
                }
            }
        }
        return null;
    }
}
exports.NodeModulesCollector = NodeModulesCollector;
//# sourceMappingURL=nodeModulesCollector.js.map