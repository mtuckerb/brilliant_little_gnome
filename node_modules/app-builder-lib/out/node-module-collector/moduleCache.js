"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.ModuleCache = void 0;
const builder_util_1 = require("builder-util");
const fs = require("fs-extra");
const path_1 = require("path");
class ModuleCache {
    constructor() {
        this.packageJsonMap = new Map();
        this.realPathMap = new Map();
        this.existsMap = new Map();
        this.lstatMap = new Map();
        this.requireResolveMap = new Map();
        this.packageJson = this.createAsyncProxy(this.packageJsonMap, (path) => fs.readJson(path));
        this.exists = this.createAsyncProxy(this.existsMap, (path) => (0, builder_util_1.exists)(path));
        this.lstat = this.createAsyncProxy(this.lstatMap, (path) => fs.lstat(path));
        this.requireResolve = this.createAsyncProxy(this.requireResolveMap, (cacheKey) => {
            try {
                const [name, path] = cacheKey.split("::");
                return Promise.resolve(require.resolve(name, { paths: [path] }));
            }
            catch {
                return Promise.resolve(null);
            }
        });
        this.realPath = this.createAsyncProxy(this.realPathMap, async (path) => {
            const p = (0, path_1.resolve)(path);
            try {
                const stats = await this.lstat[p];
                if (stats.isSymbolicLink()) {
                    return await fs.realpath(p);
                }
                return p;
            }
            catch (error) {
                builder_util_1.log.debug({ filePath: p, message: error.message || error.stack }, "error resolving path");
            }
            return p;
        });
    }
    // this allows dot-notation access while still supporting async retrieval
    // e.g., cache.packageJson[somePath] returns Promise<PackageJson>
    createAsyncProxy(map, compute) {
        return new Proxy({}, {
            async get(_, key) {
                if (map.has(key)) {
                    return Promise.resolve(map.get(key));
                }
                return await Promise.resolve(compute(key)).then(value => {
                    map.set(key, value);
                    return value;
                });
            },
            set(_, key, value) {
                map.set(key, value);
                return true;
            },
            has(_, key) {
                return map.has(key);
            },
        });
    }
}
exports.ModuleCache = ModuleCache;
//# sourceMappingURL=moduleCache.js.map