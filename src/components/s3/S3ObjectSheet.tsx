import {
	Sheet,
	SheetContent,
	SheetDescription,
	SheetHeader,
	SheetTitle,
} from "@/components/ui/sheet";
import { Spinner } from "@/components/ui/spinner";
import type { S3ObjectPreview } from "@/lib/tauri";

interface S3ObjectSheetProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	preview: S3ObjectPreview | null;
	loading: boolean;
}

export function S3ObjectSheet({
	open,
	onOpenChange,
	preview,
	loading,
}: S3ObjectSheetProps) {
	return (
		<Sheet open={open} onOpenChange={onOpenChange}>
			<SheetContent className="sm:max-w-lg overflow-y-auto">
				<SheetHeader>
					<SheetTitle className="truncate">{preview?.key || "Object"}</SheetTitle>
					<SheetDescription>
						{preview
							? `${formatBytes(preview.size)}${preview.contentType ? ` · ${preview.contentType}` : ""}`
							: "Object preview"}
					</SheetDescription>
				</SheetHeader>
				<div className="mt-4">
					{loading && (
						<div className="flex justify-center py-8">
							<Spinner className="w-6 h-6" />
						</div>
					)}
					{!loading && preview?.isImage && preview.dataBase64 && (
						<img
							src={`data:${preview.contentType || "image/png"};base64,${preview.dataBase64}`}
							alt={preview.key}
							className="max-w-full rounded border"
						/>
					)}
					{!loading && preview?.isText && (
						<pre className="text-xs bg-muted p-3 rounded overflow-auto max-h-[60vh] whitespace-pre-wrap">
							{preview.text}
							{preview.truncated ? "\n… truncated" : ""}
						</pre>
					)}
					{!loading && preview && !preview.isImage && !preview.isText && (
						<p className="text-sm text-muted-foreground">
							Binary object ({formatBytes(preview.size)}). Download to inspect.
						</p>
					)}
				</div>
			</SheetContent>
		</Sheet>
	);
}

function formatBytes(size: number) {
	if (size < 1024) return `${size} B`;
	if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
	return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}
