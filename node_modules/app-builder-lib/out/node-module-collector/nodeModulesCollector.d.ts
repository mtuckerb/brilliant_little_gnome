import { TmpDir } from "builder-util";
import { CancellationToken } from "builder-util-runtime";
import { Lazy } from "lazy-val";
import { ModuleCache } from "./moduleCache";
import { PM } from "./packageManager";
import type { Dependency, DependencyGraph, NodeModuleInfo } from "./types";
type Result = {
    packageDir: string;
    version: string;
} | null;
export declare abstract class NodeModulesCollector<ProdDepType extends Dependency<ProdDepType, OptionalDepType>, OptionalDepType> {
    protected readonly rootDir: string;
    private readonly tempDirManager;
    private readonly nodeModules;
    protected readonly allDependencies: Map<string, ProdDepType>;
    protected readonly productionGraph: DependencyGraph;
    protected readonly cache: ModuleCache;
    protected isHoisted: Lazy<boolean>;
    constructor(rootDir: string, tempDirManager: TmpDir);
    getNodeModules({ cancellationToken, packageName }: {
        cancellationToken: CancellationToken;
        packageName: string;
    }): Promise<NodeModuleInfo[]>;
    abstract readonly installOptions: {
        manager: PM;
        lockfile: string;
    };
    protected abstract getArgs(): string[];
    protected abstract parseDependenciesTree(jsonBlob: string): Promise<ProdDepType>;
    protected abstract extractProductionDependencyGraph(tree: Dependency<ProdDepType, OptionalDepType>, dependencyId: string): Promise<void>;
    protected abstract collectAllDependencies(tree: Dependency<ProdDepType, OptionalDepType>, appPackageName: string): Promise<void>;
    protected getDependenciesTree(pm: PM): Promise<ProdDepType>;
    protected cacheKey(pkg: Pick<ProdDepType, "name" | "version" | "path">): string;
    protected packageVersionString(pkg: Pick<ProdDepType, "name" | "version">): string;
    protected isProdDependency(depName: string, pkg: ProdDepType): boolean;
    /**
     * Parse a dependency identifier like "@scope/pkg@1.2.3" or "pkg@1.2.3"
     */
    protected parseNameVersion(identifier: string): {
        name: string;
        version: string;
    };
    protected getTreeFromWorkspaces(tree: ProdDepType, packageName: string): Promise<ProdDepType>;
    private transformToHoisterTree;
    private _getNodeModules;
    asyncExec(command: string, args: string[], cwd?: string): Promise<{
        stdout: string | undefined;
        stderr: string | undefined;
    }>;
    streamCollectorCommandToFile(command: string, args: string[], cwd: string, tempOutputFile: string): Promise<void>;
    protected locatePackageVersion(parentDir: string, pkgName: string, requiredRange?: string): Promise<Result | null>;
    protected readPackageVersion(pkgJsonPath: string): Promise<string | null>;
    protected semverSatisfies(found: string, range?: string): boolean;
    /**
     * Upward search (hoisted)
     */
    private upwardSearch;
    /**
     * Breadth-first downward search from parentDir/node_modules
     * Looks for node_modules/\*\/node_modules/pkgName (and deeper)
     */
    private downwardSearch;
}
export {};
