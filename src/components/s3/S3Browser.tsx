import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
	ArrowLeft,
	DownloadSimple,
	FolderPlus,
	PencilSimple,
	Trash,
	UploadSimple,
} from "@phosphor-icons/react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { S3ObjectSheet } from "@/components/s3/S3ObjectSheet";
import {
	AlertDialog,
	AlertDialogAction,
	AlertDialogCancel,
	AlertDialogContent,
	AlertDialogDescription,
	AlertDialogFooter,
	AlertDialogHeader,
	AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import {
	api,
	type S3ListResult,
	type S3ObjectPreview,
	type S3Source,
} from "@/lib/tauri";

interface S3BrowserProps {
	source: S3Source;
	initialPrefix?: string;
}

export function S3Browser({ source, initialPrefix = "" }: S3BrowserProps) {
	const [prefix, setPrefix] = useState(initialPrefix);
	const [listing, setListing] = useState<S3ListResult | null>(null);
	const [loading, setLoading] = useState(true);
	const [selected, setSelected] = useState<Set<string>>(new Set());
	const [preview, setPreview] = useState<S3ObjectPreview | null>(null);
	const [previewOpen, setPreviewOpen] = useState(false);
	const [previewLoading, setPreviewLoading] = useState(false);
	const [deleteKeys, setDeleteKeys] = useState<string[] | null>(null);
	const [folderName, setFolderName] = useState("");
	const [showFolder, setShowFolder] = useState(false);
	const [busy, setBusy] = useState(false);
	const previewClickTimer = useRef<number | null>(null);

	const crumbs = useMemo(() => {
		const parts = prefix.split("/").filter(Boolean);
		const items = [{ label: "root", value: "" }];
		let acc = "";
		for (const part of parts) {
			acc += `${part}/`;
			items.push({ label: part, value: acc });
		}
		return items;
	}, [prefix]);

	const load = useCallback(
		async (nextPrefix = prefix) => {
			setLoading(true);
			try {
				const result = await api.s3.listObjects(source, nextPrefix || undefined);
				setListing(result);
				setSelected(new Set());
			} catch (error) {
				toast.error(String(error));
			} finally {
				setLoading(false);
			}
		},
		[prefix, source],
	);

	useEffect(() => {
		load(prefix);
	}, [load, prefix]);

	const parentPrefix = prefix.includes("/")
		? prefix.replace(/[^/]+\/?$/, "")
		: "";

	const handlePreview = async (key: string) => {
		setPreviewOpen(true);
		setPreviewLoading(true);
		try {
			setPreview(await api.s3.previewObject(source, key));
		} catch (error) {
			toast.error(String(error));
			setPreviewOpen(false);
		} finally {
			setPreviewLoading(false);
		}
	};

	const handleOpen = async (key: string) => {
		if (previewClickTimer.current) {
			window.clearTimeout(previewClickTimer.current);
			previewClickTimer.current = null;
		}
		setBusy(true);
		try {
			const dest = await api.s3.openObject(source, key);
			toast.success(`Opened ${dest.split("/").pop() || "file"}`);
		} catch (error) {
			toast.error(String(error));
		} finally {
			setBusy(false);
		}
	};

	const handleNameClick = (key: string) => {
		if (previewClickTimer.current) {
			window.clearTimeout(previewClickTimer.current);
		}
		previewClickTimer.current = window.setTimeout(() => {
			previewClickTimer.current = null;
			void handlePreview(key);
		}, 250);
	};

	const handleUpload = async () => {
		const files = await open({ multiple: true });
		if (!files) return;
		const paths = Array.isArray(files) ? files : [files];
		setBusy(true);
		try {
			for (const filePath of paths) {
				const name = filePath.split("/").pop() || "upload";
				await api.s3.uploadObject(source, `${prefix}${name}`, filePath);
			}
			toast.success("Upload complete");
			await load();
		} catch (error) {
			toast.error(String(error));
		} finally {
			setBusy(false);
		}
	};

	const handleDownload = async (key: string) => {
		const name = key.split("/").pop() || "download";
		const dest = await save({ defaultPath: name });
		if (!dest) return;
		setBusy(true);
		try {
			await api.s3.downloadObject(source, key, dest);
			toast.success("Downloaded");
		} catch (error) {
			toast.error(String(error));
		} finally {
			setBusy(false);
		}
	};

	const handleRename = async (key: string) => {
		const current = key.split("/").pop() || key;
		const next = window.prompt("Rename to", current);
		if (!next || next === current) return;
		const dest = `${prefix}${next}`;
		setBusy(true);
		try {
			await api.s3.copyObject(source, key, dest);
			await api.s3.deleteObjects(source, [key]);
			toast.success("Renamed");
			await load();
		} catch (error) {
			toast.error(String(error));
		} finally {
			setBusy(false);
		}
	};

	const confirmDelete = async () => {
		if (!deleteKeys?.length) return;
		setBusy(true);
		try {
			await api.s3.deleteObjects(source, deleteKeys);
			toast.success("Deleted");
			setDeleteKeys(null);
			await load();
		} catch (error) {
			toast.error(String(error));
		} finally {
			setBusy(false);
		}
	};

	const createFolder = async () => {
		if (!folderName.trim()) return;
		setBusy(true);
		try {
			await api.s3.createFolder(source, `${prefix}${folderName.trim()}`);
			setShowFolder(false);
			setFolderName("");
			await load();
		} catch (error) {
			toast.error(String(error));
		} finally {
			setBusy(false);
		}
	};

	const objects = listing?.objects.filter((obj) => obj.key !== prefix) ?? [];
	const prefixes = listing?.prefixes ?? [];

	const entryName = (key: string) => {
		const normalizedKey = key.replace(/\/+$/, "");
		const normalizedCurrent = prefix.replace(/\/+$/, "");
		let rest = normalizedKey;
		if (
			normalizedCurrent &&
			normalizedKey.startsWith(`${normalizedCurrent}/`)
		) {
			rest = normalizedKey.slice(normalizedCurrent.length + 1);
		} else if (normalizedCurrent && normalizedKey === normalizedCurrent) {
			return "";
		}
		return rest.split("/").filter(Boolean)[0] ?? "";
	};

	return (
		<div className="flex flex-col h-full min-h-0">
			<div className="flex items-center gap-2 px-4 py-2 border-b">
				<Button
					variant="ghost"
					size="sm"
					disabled={!prefix}
					onClick={() => setPrefix(parentPrefix)}
				>
					<ArrowLeft className="size-4" />
					Up
				</Button>
				<div className="flex items-center gap-1 text-sm min-w-0 flex-1 overflow-x-auto">
					{crumbs.map((crumb, i) => (
						<button
							key={crumb.value}
							type="button"
							className="hover:underline shrink-0 text-muted-foreground"
							onClick={() => setPrefix(crumb.value)}
						>
							{i > 0 ? " / " : ""}
							{crumb.label}
						</button>
					))}
				</div>
				<Button variant="outline" size="sm" onClick={handleUpload} disabled={busy}>
					{busy ? <Spinner /> : <UploadSimple className="size-4" />}
					Upload
				</Button>
				<Button
					variant="outline"
					size="sm"
					onClick={() => setShowFolder(true)}
					disabled={busy}
				>
					<FolderPlus className="size-4" />
					Folder
				</Button>
				{selected.size > 0 && (
					<Button
						variant="destructive"
						size="sm"
						onClick={() => setDeleteKeys(Array.from(selected))}
					>
						<Trash className="size-4" />
						Delete
					</Button>
				)}
			</div>

			{showFolder && (
				<div className="flex items-center gap-2 px-4 py-2 border-b">
					<Input
						placeholder="folder-name"
						value={folderName}
						onChange={(e) => setFolderName(e.target.value)}
					/>
					<Button size="sm" onClick={createFolder} disabled={busy}>
						{busy && <Spinner />}
						Create
					</Button>
					<Button size="sm" variant="ghost" onClick={() => setShowFolder(false)}>
						Cancel
					</Button>
				</div>
			)}

			<div className="flex-1 overflow-auto">
				{loading ? (
					<div className="flex justify-center py-16">
						<Spinner className="w-6 h-6" />
					</div>
				) : (
					<table className="w-full text-sm">
						<thead className="sticky top-0 bg-background border-b">
							<tr className="text-left text-muted-foreground">
								<th className="w-8 px-3 py-2" />
								<th className="px-3 py-2">Name</th>
								<th className="px-3 py-2 w-28">Size</th>
								<th className="px-3 py-2 w-48">Modified</th>
								<th className="px-3 py-2 w-32" />
							</tr>
						</thead>
						<tbody>
							{prefixes.map((item) => {
								const name = entryName(item.prefix);
								if (!name) return null;
								return (
									<tr
										key={item.prefix}
										className="border-b hover:bg-accent/50 cursor-pointer"
										onDoubleClick={() => setPrefix(item.prefix)}
									>
										<td className="px-3 py-2" />
										<td className="px-3 py-2 font-medium" onClick={() => setPrefix(item.prefix)}>
											{name}/
										</td>
										<td className="px-3 py-2 text-muted-foreground">—</td>
										<td className="px-3 py-2 text-muted-foreground">—</td>
										<td />
									</tr>
								);
							})}
							{objects.map((obj) => {
								const name =
									prefix && obj.key.startsWith(prefix)
										? obj.key.slice(prefix.length)
										: obj.key;
								const checked = selected.has(obj.key);
								return (
									<tr
										key={obj.key}
										className="border-b hover:bg-accent/50"
										onDoubleClick={() => handleOpen(obj.key)}
									>
										<td className="px-3 py-2">
											<input
												type="checkbox"
												checked={checked}
												onChange={() => {
													setSelected((prev) => {
														const next = new Set(prev);
														if (next.has(obj.key)) next.delete(obj.key);
														else next.add(obj.key);
														return next;
													});
												}}
											/>
										</td>
										<td
											className="px-3 py-2 cursor-pointer"
											onClick={() => handleNameClick(obj.key)}
											onDoubleClick={(event) => {
												event.stopPropagation();
												void handleOpen(obj.key);
											}}
										>
											{name}
										</td>
										<td className="px-3 py-2 text-muted-foreground">
											{formatBytes(obj.size)}
										</td>
										<td className="px-3 py-2 text-muted-foreground truncate">
											{obj.lastModified || "—"}
										</td>
										<td className="px-3 py-2">
											<div className="flex justify-end">
												<Button
													variant="ghost"
													size="icon-sm"
													onClick={() => handleDownload(obj.key)}
												>
													<DownloadSimple className="size-4" />
												</Button>
												<Button
													variant="ghost"
													size="icon-sm"
													onClick={() => handleRename(obj.key)}
												>
													<PencilSimple className="size-4" />
												</Button>
												<Button
													variant="ghost"
													size="icon-sm"
													onClick={() => setDeleteKeys([obj.key])}
												>
													<Trash className="size-4" />
												</Button>
											</div>
										</td>
									</tr>
								);
							})}
							{prefixes.length === 0 && objects.length === 0 && (
								<tr>
									<td colSpan={5} className="px-3 py-10 text-center text-muted-foreground">
										This prefix is empty
									</td>
								</tr>
							)}
						</tbody>
					</table>
				)}
			</div>

			<S3ObjectSheet
				open={previewOpen}
				onOpenChange={setPreviewOpen}
				preview={preview}
				loading={previewLoading}
			/>

			<AlertDialog
				open={!!deleteKeys}
				onOpenChange={(open) => !open && setDeleteKeys(null)}
			>
				<AlertDialogContent>
					<AlertDialogHeader>
						<AlertDialogTitle>Delete objects</AlertDialogTitle>
						<AlertDialogDescription>
							Delete {deleteKeys?.length} object
							{deleteKeys && deleteKeys.length !== 1 ? "s" : ""}? This cannot be
							undone.
						</AlertDialogDescription>
					</AlertDialogHeader>
					<AlertDialogFooter>
						<AlertDialogCancel>Cancel</AlertDialogCancel>
						<AlertDialogAction onClick={confirmDelete} disabled={busy}>
							{busy && <Spinner />}
							Delete
						</AlertDialogAction>
					</AlertDialogFooter>
				</AlertDialogContent>
			</AlertDialog>
		</div>
	);
}

function formatBytes(size: number) {
	if (size < 1024) return `${size} B`;
	if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
	return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}
