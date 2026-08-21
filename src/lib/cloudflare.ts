import type { CachedCloudflareResource } from "@/lib/tauri";

export interface CloudflareResource {
	id: number;
	kind: "d1" | "r2";
	resourceId: string;
	name: string;
	accountId: string;
	extra: Record<string, unknown>;
	syncedAt: string;
}

export function parseCachedCloudflareResources(
	cached: CachedCloudflareResource[],
): CloudflareResource[] {
	return cached.map((row) => {
		let extra: Record<string, unknown> = {};
		if (row.extra) {
			try {
				extra = JSON.parse(row.extra);
			} catch {
				extra = {};
			}
		}
		return {
			id: row.id,
			kind: row.kind === "r2" ? "r2" : "d1",
			resourceId: row.resourceId,
			name: row.name,
			accountId: row.accountId,
			extra,
			syncedAt: row.syncedAt,
		};
	});
}
