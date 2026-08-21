import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { ArrowsClockwise, CaretDown, CaretRight } from "@phosphor-icons/react";
import { toast } from "sonner";
import { D1Icon } from "@/components/icons/d1";
import { S3Icon } from "@/components/icons/s3";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import type { CloudflareResource } from "@/lib/cloudflare";
import { api } from "@/lib/tauri";

interface CloudflareTreeProps {
	resources: CloudflareResource[];
	d1Error?: string | null;
	r2Error?: string | null;
	onSync: () => Promise<void>;
}

export function CloudflareTree({
	resources,
	d1Error,
	r2Error,
	onSync,
}: CloudflareTreeProps) {
	const navigate = useNavigate();
	const [syncing, setSyncing] = useState(false);
	const [expanded, setExpanded] = useState<Set<string>>(new Set(["d1", "r2"]));
	const [connecting, setConnecting] = useState<string | null>(null);

	const d1 = resources.filter((r) => r.kind === "d1");
	const r2 = resources.filter((r) => r.kind === "r2");

	const toggle = (key: string) => {
		setExpanded((prev) => {
			const next = new Set(prev);
			if (next.has(key)) next.delete(key);
			else next.add(key);
			return next;
		});
	};

	const handleSync = async () => {
		setSyncing(true);
		try {
			await onSync();
		} finally {
			setSyncing(false);
		}
	};

	const handleOpen = async (resource: CloudflareResource) => {
		const key = `${resource.kind}:${resource.resourceId}`;
		setConnecting(key);
		try {
			const result = await api.cloudflare.connect(
				resource.kind,
				resource.resourceId,
			);
			if (result.error) {
				toast.error(result.error);
				return;
			}
			const poolKey = await api.cloudflare.poolKey(
				resource.kind,
				resource.resourceId,
			);
			navigate(`/connections/${encodeURIComponent(poolKey)}`, {
				state: {
					cloudflare: true,
					kind: resource.kind,
					resourceId: resource.resourceId,
					name: resource.name,
					accountId: resource.accountId,
					dbType: resource.kind === "d1" ? "d1" : "s3",
				},
			});
		} catch (error) {
			toast.error(String(error));
		} finally {
			setConnecting(null);
		}
	};

	return (
		<div className="px-3 py-2 mb-4">
			<div className="flex items-center justify-between mb-2">
				<span className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
					Cloudflare
				</span>
				<Button
					variant="ghost"
					size="sm"
					className="h-6 w-6 p-0"
					onClick={handleSync}
					disabled={syncing}
				>
					{syncing ? (
						<Spinner className="size-3" />
					) : (
						<ArrowsClockwise className="size-3" />
					)}
				</Button>
			</div>

			<Group
				label="D1"
				count={d1.length}
				expanded={expanded.has("d1")}
				onToggle={() => toggle("d1")}
				error={d1Error}
			>
				{d1.map((resource) => (
					<ResourceButton
						key={resource.resourceId}
						resource={resource}
						icon={<D1Icon className="size-4 shrink-0" />}
						connecting={connecting === `d1:${resource.resourceId}`}
						onOpen={handleOpen}
					/>
				))}
				{d1.length === 0 && !d1Error && (
					<p className="ml-6 px-2 py-1 text-xs text-muted-foreground">
						No D1 databases
					</p>
				)}
			</Group>

			<Group
				label="R2"
				count={r2.length}
				expanded={expanded.has("r2")}
				onToggle={() => toggle("r2")}
				error={r2Error}
			>
				{r2.map((resource) => (
					<ResourceButton
						key={resource.resourceId}
						resource={resource}
						icon={<S3Icon className="size-4 shrink-0" />}
						connecting={connecting === `r2:${resource.resourceId}`}
						onOpen={handleOpen}
					/>
				))}
				{r2.length === 0 && !r2Error && (
					<p className="ml-6 px-2 py-1 text-xs text-muted-foreground">
						No R2 buckets
					</p>
				)}
			</Group>
		</div>
	);
}

function Group({
	label,
	count,
	expanded,
	onToggle,
	error,
	children,
}: {
	label: string;
	count: number;
	expanded: boolean;
	onToggle: () => void;
	error?: string | null;
	children: React.ReactNode;
}) {
	return (
		<div>
			<button
				type="button"
				className="flex items-center gap-1.5 w-full px-2 py-1 text-sm rounded hover:bg-accent text-left"
				onClick={onToggle}
			>
				{expanded ? (
					<CaretDown className="size-3" />
				) : (
					<CaretRight className="size-3" />
				)}
				<span className="truncate font-medium">{label}</span>
				<span className="text-xs text-muted-foreground ml-auto">{count}</span>
			</button>
			{expanded && (
				<div>
					{error && (
						<p className="ml-6 px-2 py-1 text-xs text-destructive">{error}</p>
					)}
					{children}
				</div>
			)}
		</div>
	);
}

function ResourceButton({
	resource,
	icon,
	connecting,
	onOpen,
}: {
	resource: CloudflareResource;
	icon: React.ReactNode;
	connecting: boolean;
	onOpen: (resource: CloudflareResource) => void;
}) {
	return (
		<button
			type="button"
			className="flex items-center gap-2 w-full ml-3 px-2 py-1 text-sm rounded text-left hover:bg-accent"
			onClick={() => onOpen(resource)}
			disabled={connecting}
		>
			{icon}
			<span className="truncate">{resource.name}</span>
			{connecting && <Spinner className="size-3 ml-auto" />}
		</button>
	);
}
