import { NodeModulesCollector } from "./nodeModulesCollector";
import { PM } from "./packageManager";
import { NpmDependency } from "./types";
export declare class NpmNodeModulesCollector extends NodeModulesCollector<NpmDependency, string> {
    readonly installOptions: {
        manager: PM;
        lockfile: string;
    };
    protected getArgs(): string[];
    protected getDependenciesTree(pm: PM): Promise<NpmDependency>;
    protected collectAllDependencies(tree: NpmDependency): Promise<void>;
    protected extractProductionDependencyGraph(tree: NpmDependency, dependencyId: string): Promise<void>;
    private isDuplicatedNpmDependency;
    protected isProdDependency(packageName: string, tree: NpmDependency): boolean;
    /**
     * Builds a dependency tree using only package.json dependencies and optionalDependencies.
     * This skips devDependencies and uses Node.js module resolution (require.resolve).
     */
    protected buildNodeModulesTreeManually(baseDir: string): Promise<NpmDependency>;
    protected parseDependenciesTree(jsonBlob: string): Promise<NpmDependency>;
}
