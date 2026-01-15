"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.BunNodeModulesCollector = void 0;
const builder_util_1 = require("builder-util");
const npmNodeModulesCollector_1 = require("./npmNodeModulesCollector");
const packageManager_1 = require("./packageManager");
class BunNodeModulesCollector extends npmNodeModulesCollector_1.NpmNodeModulesCollector {
    constructor() {
        super(...arguments);
        this.installOptions = { manager: packageManager_1.PM.BUN, lockfile: "bun.lock" };
    }
    async getDependenciesTree(_pm) {
        builder_util_1.log.info(null, "note: bun does not support any CLI for dependency tree extraction, utilizing NPM node module collector instead");
        return super.getDependenciesTree(packageManager_1.PM.NPM);
    }
    isProdDependency(packageName, tree) {
        var _a, _b;
        return ((_a = tree.dependencies) === null || _a === void 0 ? void 0 : _a[packageName]) != null || ((_b = tree.optionalDependencies) === null || _b === void 0 ? void 0 : _b[packageName]) != null;
    }
}
exports.BunNodeModulesCollector = BunNodeModulesCollector;
//# sourceMappingURL=bunNodeModulesCollector.js.map