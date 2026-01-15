import { NodeModulesCollector } from "./nodeModulesCollector";
import { PM } from "./packageManager";
import { PnpmDependency } from "./types";
export declare class PnpmNodeModulesCollector extends NodeModulesCollector<PnpmDependency, PnpmDependency> {
    readonly installOptions: {
        manager: PM;
        lockfile: string;
    };
    protected getArgs(): string[];
    private getProductionDependencies;
    protected extractProductionDependencyGraph(tree: PnpmDependency, dependencyId: string): Promise<void>;
    protected collectAllDependencies(tree: PnpmDependency): Promise<void>;
    protected packageVersionString(pkg: PnpmDependency): string;
    protected parseDependenciesTree(jsonBlob: string): Promise<PnpmDependency>;
}
