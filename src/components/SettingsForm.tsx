import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Eye, EyeSlash, Folder } from "@phosphor-icons/react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "@/lib/tauri";
import { Spinner } from "@/components/ui/spinner";
import { toast } from "sonner";
import {
	Combobox,
	ComboboxInput,
	ComboboxContent,
	ComboboxList,
	ComboboxItem,
} from "@/components/ui/combobox";

type Theme = "light" | "dark" | "system";

interface SettingsFormProps {
	onSaveSuccess?: () => void;
	compact?: boolean;
}

export function SettingsForm({ onSaveSuccess, compact }: SettingsFormProps) {
	const [loading, setLoading] = useState(true);
	const [saving, setSaving] = useState(false);
	const [showApiKey, setShowApiKey] = useState(false);

	const [theme, setTheme] = useState<Theme>("system");
	const [checkUpdates, setCheckUpdates] = useState(true);
	const [openaiEndpoint, setOpenaiEndpoint] = useState("");
	const [openaiApiKey, setOpenaiApiKey] = useState("");
	const [openaiModel, setOpenaiModel] = useState("gpt-4.1");
	const [forgegraphServer, setForgegraphServer] = useState("");
	const [forgegraphToken, setForgegraphToken] = useState("");
	const [forgegraphCredSource, setForgegraphCredSource] = useState<
		"cli" | "settings"
	>("settings");
	const [showForgegraphToken, setShowForgegraphToken] = useState(false);
	const [testingForgegraph, setTestingForgegraph] = useState(false);
	const [cloudflareToken, setCloudflareToken] = useState("");
	const [cloudflareAccountId, setCloudflareAccountId] = useState("");
	const [cloudflareR2Key, setCloudflareR2Key] = useState("");
	const [cloudflareR2Secret, setCloudflareR2Secret] = useState("");
	const [showCloudflareToken, setShowCloudflareToken] = useState(false);
	const [showCloudflareR2Secret, setShowCloudflareR2Secret] = useState(false);
	const [testingCloudflare, setTestingCloudflare] = useState(false);
	const [downloadTmpDir, setDownloadTmpDir] = useState("");
	const [downloadTmpDirPlaceholder, setDownloadTmpDirPlaceholder] =
		useState("");

	useEffect(() => {
		loadSettings();
	}, []);

	const loadSettings = async () => {
		setLoading(true);
		try {
			const [settings, fg, tmpDir] = await Promise.all([
				api.settings.getAll(),
				api.forgegraph.credentials(),
				api.s3.downloadTmpDir(),
			]);
			setTheme((settings.theme as Theme) || "system");
			setCheckUpdates(settings.check_updates_on_startup !== "false");
			setOpenaiEndpoint(settings.openai_endpoint || "");
			setOpenaiApiKey(settings.openai_api_key || "");
			setOpenaiModel(settings.openai_model || "gpt-4.1");
			setForgegraphServer(fg.server || settings.forgegraph_server || "");
			setForgegraphToken(fg.token || settings.forgegraph_token || "");
			setForgegraphCredSource(fg.source === "cli" ? "cli" : "settings");
			setCloudflareToken(settings.cloudflare_api_token || "");
			setCloudflareAccountId(settings.cloudflare_account_id || "");
			setCloudflareR2Key(settings.cloudflare_r2_access_key || "");
			setCloudflareR2Secret(settings.cloudflare_r2_secret_key || "");
			setDownloadTmpDir(settings.download_tmp_dir || "");
			setDownloadTmpDirPlaceholder(tmpDir || "");
		} catch (error) {
			console.error("Failed to load settings:", error);
		} finally {
			setLoading(false);
		}
	};

	const handleSave = async () => {
		setSaving(true);
		try {
			await api.settings.set("theme", theme);
			await api.settings.set(
				"check_updates_on_startup",
				checkUpdates.toString(),
			);
			await api.settings.set("openai_endpoint", openaiEndpoint);
			await api.settings.set("openai_api_key", openaiApiKey);
			await api.settings.set("openai_model", openaiModel);
			await api.settings.set("forgegraph_server", forgegraphServer);
			await api.settings.set("forgegraph_token", forgegraphToken);
			await api.settings.set("cloudflare_api_token", cloudflareToken);
			await api.settings.set("cloudflare_account_id", cloudflareAccountId);
			await api.settings.set("cloudflare_r2_access_key", cloudflareR2Key);
			await api.settings.set("cloudflare_r2_secret_key", cloudflareR2Secret);
			await api.settings.set("download_tmp_dir", downloadTmpDir);

			applyTheme(theme);
			toast.success("Settings saved");
			onSaveSuccess?.();
		} catch (error) {
			console.error("Failed to save settings:", error);
			toast.error("Failed to save settings");
		} finally {
			setSaving(false);
		}
	};

	const handleTestForgegraph = async () => {
		setTestingForgegraph(true);
		try {
			await api.settings.set("forgegraph_server", forgegraphServer);
			await api.settings.set("forgegraph_token", forgegraphToken);
			const services = await api.forgegraph.sync();
			toast.success(`Connected — found ${services.length} service${services.length !== 1 ? "s" : ""}`);
		} catch (error) {
			toast.error(String(error));
		} finally {
			setTestingForgegraph(false);
		}
	};

	const handleTestCloudflare = async () => {
		setTestingCloudflare(true);
		try {
			await api.settings.set("cloudflare_api_token", cloudflareToken);
			await api.settings.set("cloudflare_account_id", cloudflareAccountId);
			await api.settings.set("cloudflare_r2_access_key", cloudflareR2Key);
			await api.settings.set("cloudflare_r2_secret_key", cloudflareR2Secret);
			const result = await api.cloudflare.test();
			if (result.accountId && !cloudflareAccountId) {
				setCloudflareAccountId(result.accountId);
			}
			const parts = [`${result.d1Count} D1`, `${result.r2Count} R2`];
			toast.success(`Connected — ${parts.join(", ")}`);
			if (result.d1Error) toast.error(`D1: ${result.d1Error}`);
			if (result.r2Error) toast.error(`R2: ${result.r2Error}`);
		} catch (error) {
			toast.error(String(error));
		} finally {
			setTestingCloudflare(false);
		}
	};

	const applyTheme = (t: Theme) => {
		const root = window.document.documentElement;
		if (t === "system") {
			const systemTheme = window.matchMedia("(prefers-color-scheme: dark)")
				.matches
				? "dark"
				: "light";
			root.classList.toggle("dark", systemTheme === "dark");
		} else {
			root.classList.toggle("dark", t === "dark");
		}
		localStorage.setItem("theme", t);
	};

	if (loading) {
		return (
			<div className="flex items-center justify-center py-8">
				<Spinner className="w-8 h-8" />
			</div>
		);
	}

	const spacing = compact ? "space-y-4" : "space-y-8";
	const headingSize = compact ? "text-sm font-medium" : "text-lg font-medium";

	return (
		<div className={spacing}>
			<div className="space-y-3">
				<h3 className={headingSize}>Appearance</h3>
				<div className="flex gap-2">
					{(["light", "dark", "system"] as Theme[]).map((t) => (
						<Button
							key={t}
							variant={theme === t ? "default" : "outline"}
							onClick={() => setTheme(t)}
							className="capitalize"
							size={compact ? "sm" : "default"}
						>
							{t}
						</Button>
					))}
				</div>
			</div>

			<div className="space-y-3">
				<h3 className={headingSize}>Downloads</h3>
				<p className="text-[0.8rem] text-muted-foreground">
					Double-clicking an S3 or R2 object downloads it here and opens it.
				</p>
				<div className="space-y-2">
					<Label htmlFor="download-tmp-dir" className={compact ? "text-sm" : ""}>
						Temporary folder
					</Label>
					<div className="flex gap-2">
						<Input
							id="download-tmp-dir"
							placeholder={downloadTmpDirPlaceholder || "OS temp /dbcooper"}
							value={downloadTmpDir}
							onChange={(e) => setDownloadTmpDir(e.target.value)}
						/>
						<Button
							type="button"
							variant="outline"
							onClick={async () => {
								const selected = await open({
									directory: true,
									multiple: false,
									defaultPath: downloadTmpDir || downloadTmpDirPlaceholder || undefined,
								});
								if (typeof selected === "string") {
									setDownloadTmpDir(selected);
								}
							}}
						>
							<Folder className="size-4" />
							Browse
						</Button>
					</div>
				</div>
			</div>

			<div className="space-y-3">
				<h3 className={headingSize}>Updates</h3>
				<div className="flex items-center justify-between">
					<Label htmlFor="check-updates" className={compact ? "text-sm" : ""}>
						Check for updates on startup
					</Label>
					<Switch
						id="check-updates"
						checked={checkUpdates}
						onCheckedChange={setCheckUpdates}
					/>
				</div>
			</div>

			<div className="space-y-3">
				<h3 className={headingSize}>OpenAI</h3>
				<div className="space-y-2">
					<Label htmlFor="openai-endpoint" className={compact ? "text-sm" : ""}>
						Endpoint (optional)
					</Label>
					<Input
						id="openai-endpoint"
						placeholder="https://api.openai.com/v1"
						value={openaiEndpoint}
						onChange={(e) => setOpenaiEndpoint(e.target.value)}
					/>
				</div>
				<div className="space-y-2">
					<Label className={compact ? "text-sm" : ""}>Model</Label>
					<Combobox
						value={openaiModel}
						onValueChange={(val) => val && setOpenaiModel(val as string)}
					>
						<ComboboxInput
							placeholder="Select or type model..."
							value={openaiModel}
							onChange={(e) => setOpenaiModel(e.target.value)}
						/>
						<ComboboxContent>
							<ComboboxList>
								<ComboboxItem value="gpt-4o">gpt-4o</ComboboxItem>
								<ComboboxItem value="gpt-4o-mini">gpt-4o-mini</ComboboxItem>
								<ComboboxItem value="gpt-4.1">gpt-4.1</ComboboxItem>
								<ComboboxItem value="gpt-4.1-mini">gpt-4.1-mini</ComboboxItem>
								{![
									"gpt-4o",
									"gpt-4o-mini",
									"gpt-4.1",
									"gpt-4.1-mini",
								].includes(openaiModel) && (
									<ComboboxItem value={openaiModel}>{openaiModel}</ComboboxItem>
								)}
							</ComboboxList>
						</ComboboxContent>
					</Combobox>
					<p className="text-[0.8rem] text-muted-foreground">
						You can select a predefined model or type a custom model ID
						{compact ? "." : " for your endpoint."}
					</p>
				</div>
				<div className="space-y-2">
					<Label htmlFor="openai-key" className={compact ? "text-sm" : ""}>
						API Key
					</Label>
					<div className="relative">
						<Input
							id="openai-key"
							type={showApiKey ? "text" : "password"}
							placeholder="sk-..."
							value={openaiApiKey}
							onChange={(e) => setOpenaiApiKey(e.target.value)}
							className="pr-10"
						/>
						<Button
							type="button"
							variant="ghost"
							size="icon"
							className="absolute right-0 top-0 h-full"
							onClick={() => setShowApiKey(!showApiKey)}
						>
							{showApiKey ? (
								<EyeSlash className="h-4 w-4" />
							) : (
								<Eye className="h-4 w-4" />
							)}
						</Button>
					</div>
				</div>
			</div>

			<div className="space-y-3">
				<h3 className={headingSize}>ForgeGraph</h3>
				<p className="text-[0.8rem] text-muted-foreground">
					{forgegraphCredSource === "cli"
						? "Loaded from ~/.forgegraph/credentials.json (forge CLI). That file is used for sync even if these fields are empty."
						: "Paste a server URL and API token, or run `forge login` so DBcooper can read ~/.forgegraph/credentials.json."}
				</p>
				<div className="space-y-2">
					<Label htmlFor="forgegraph-server" className={compact ? "text-sm" : ""}>
						Server URL
					</Label>
					<Input
						id="forgegraph-server"
						type="url"
						placeholder="https://forgegraf.com"
						value={forgegraphServer}
						onChange={(e) => setForgegraphServer(e.target.value)}
					/>
				</div>
				<div className="space-y-2">
					<Label htmlFor="forgegraph-token" className={compact ? "text-sm" : ""}>
						API Token
					</Label>
					<div className="relative">
						<Input
							id="forgegraph-token"
							type={showForgegraphToken ? "text" : "password"}
							placeholder="fg_..."
							value={forgegraphToken}
							onChange={(e) => setForgegraphToken(e.target.value)}
							className="pr-10"
						/>
						<Button
							type="button"
							variant="ghost"
							size="icon"
							className="absolute right-0 top-0 h-full"
							onClick={() => setShowForgegraphToken(!showForgegraphToken)}
						>
							{showForgegraphToken ? (
								<EyeSlash className="h-4 w-4" />
							) : (
								<Eye className="h-4 w-4" />
							)}
						</Button>
					</div>
				</div>
				{forgegraphServer && forgegraphToken && (
					<Button
						type="button"
						variant="outline"
						size={compact ? "sm" : "default"}
						onClick={handleTestForgegraph}
						disabled={testingForgegraph}
					>
						{testingForgegraph && <Spinner />}
						Test Connection
					</Button>
				)}
			</div>

			<div className="space-y-3">
				<h3 className={headingSize}>Cloudflare</h3>
				<p className="text-[0.8rem] text-muted-foreground">
					User tokens (My Profile → API Tokens) or account tokens (`cfat_`)
					both work. Needs D1 Read/Write and Workers R2 Storage Read/Write.
					Account tokens may need an Account ID. Object browsing derives S3
					keys from the token id + SHA-256 of the token value; if that fails,
					set optional R2 S3 keys below.
				</p>
				<div className="space-y-2">
					<Label htmlFor="cf-token" className={compact ? "text-sm" : ""}>
						API Token
					</Label>
					<div className="relative">
						<Input
							id="cf-token"
							type={showCloudflareToken ? "text" : "password"}
							placeholder="cfut_… or cfat_…"
							value={cloudflareToken}
							onChange={(e) => setCloudflareToken(e.target.value)}
							className="pr-10"
						/>
						<Button
							type="button"
							variant="ghost"
							size="icon"
							className="absolute right-0 top-0 h-full"
							onClick={() => setShowCloudflareToken(!showCloudflareToken)}
						>
							{showCloudflareToken ? (
								<EyeSlash className="h-4 w-4" />
							) : (
								<Eye className="h-4 w-4" />
							)}
						</Button>
					</div>
				</div>
				<div className="space-y-2">
					<Label htmlFor="cf-account" className={compact ? "text-sm" : ""}>
						Account ID
					</Label>
					<Input
						id="cf-account"
						placeholder="Required for some account tokens; otherwise auto-detected"
						value={cloudflareAccountId}
						onChange={(e) => setCloudflareAccountId(e.target.value)}
					/>
				</div>
				<div className="space-y-2">
					<Label htmlFor="cf-r2-key" className={compact ? "text-sm" : ""}>
						R2 Access Key (optional)
					</Label>
					<Input
						id="cf-r2-key"
						value={cloudflareR2Key}
						onChange={(e) => setCloudflareR2Key(e.target.value)}
					/>
				</div>
				<div className="space-y-2">
					<Label htmlFor="cf-r2-secret" className={compact ? "text-sm" : ""}>
						R2 Secret Access Key (optional)
					</Label>
					<div className="relative">
						<Input
							id="cf-r2-secret"
							type={showCloudflareR2Secret ? "text" : "password"}
							value={cloudflareR2Secret}
							onChange={(e) => setCloudflareR2Secret(e.target.value)}
							className="pr-10"
						/>
						<Button
							type="button"
							variant="ghost"
							size="icon"
							className="absolute right-0 top-0 h-full"
							onClick={() =>
								setShowCloudflareR2Secret(!showCloudflareR2Secret)
							}
						>
							{showCloudflareR2Secret ? (
								<EyeSlash className="h-4 w-4" />
							) : (
								<Eye className="h-4 w-4" />
							)}
						</Button>
					</div>
				</div>
				{cloudflareToken && (
					<Button
						type="button"
						variant="outline"
						size={compact ? "sm" : "default"}
						onClick={handleTestCloudflare}
						disabled={testingCloudflare}
					>
						{testingCloudflare && <Spinner />}
						Test
					</Button>
				)}
			</div>

			<div className={compact ? "pt-2" : "pt-4"}>
				<Button
					onClick={handleSave}
					disabled={saving}
					className={compact ? "w-full" : ""}
				>
					{saving && <Spinner />}
					Save Settings
				</Button>
			</div>
		</div>
	);
}
