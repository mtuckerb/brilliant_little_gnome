import { Lazy } from "lazy-val";
import { NodeModulesCollector } from "./nodeModulesCollector";
import { PM } from "./packageManager";
import { PackageJson, YarnDependency } from "./types";
export declare class YarnNodeModulesCollector extends NodeModulesCollector<YarnDependency, YarnDependency> {
    readonly installOptions: {
        manager: PM;
        lockfile: string;
    };
    protected isHoisted: Lazy<boolean>;
    protected getArgs(): string[];
    protected extractProductionDependencyGraph(tree: YarnDependency, dependencyId: string): Promise<void>;
    protected getDependencyType(pkgName: string, parentPkgJson: PackageJson): "prod" | "dev" | "optional";
    protected parseDependenciesTree(jsonBlob: string): Promise<YarnDependency>;
    private normalizeTree;
    protected collectAllDependencies(tree: YarnDependency, packageToExclude: string): Promise<void>;
}
