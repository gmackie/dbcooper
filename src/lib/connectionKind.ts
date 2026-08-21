export function isObjectStore(type?: string | null): boolean {
	return type === "s3" || type === "r2";
}

export function isKeyValue(type?: string | null): boolean {
	return type === "redis";
}

export function usesSqlExplorer(type?: string | null): boolean {
	return !isObjectStore(type) && !isKeyValue(type);
}

export function isRemoteSqlite(type?: string | null): boolean {
	return type === "sqlite" || type === "d1" || type === "turso";
}
