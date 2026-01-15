import { NpmNodeModulesCollector } from "./npmNodeModulesCollector";
import { PM } from "./packageManager";
import { NpmDependency } from "./types";
export declare class BunNodeModulesCollector extends NpmNodeModulesCollector {
    readonly installOptions: {
        manager: PM;
        lockfile: string;
    };
    protected getDependenciesTree(_pm: PM): Promise<NpmDependency>;
    protected isProdDependency(packageName: string, tree: NpmDependency): boolean;
}
