import { PackageJson } from "./types";
import * as fs from "fs-extra";
type PackageJsonCache = Record<string, Promise<PackageJson>>;
type RealPathCache = Record<string, Promise<string>>;
type ExistsCache = Record<string, Promise<boolean>>;
type LstatCache = Record<string, Promise<fs.Stats>>;
type RequireResolveCache = Record<string, Promise<string | null>>;
export declare class ModuleCache {
    /** Cache for package.json contents (readJson/require) */
    readonly packageJson: PackageJsonCache;
    /** Cache for resolved real paths (realpath) */
    readonly realPath: RealPathCache;
    /** Cache for file/directory existence checks */
    readonly exists: ExistsCache;
    /** Cache for lstat results */
    readonly lstat: LstatCache;
    /** Cache for require.resolve results (key: "packageName::fromDir") */
    readonly requireResolve: RequireResolveCache;
    private readonly packageJsonMap;
    private readonly realPathMap;
    private readonly existsMap;
    private readonly lstatMap;
    private readonly requireResolveMap;
    constructor();
    private createAsyncProxy;
}
export {};
