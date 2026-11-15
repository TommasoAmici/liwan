type RuntimeConfig = {
	baseUrl: string;
	disableFavicons: boolean;
};

const readConfig = (): RuntimeConfig | undefined => {
	if (typeof document === "undefined") return undefined;

	const text = document.getElementById("liwan-config")?.textContent;
	if (!text) return undefined;

	try {
		const config = JSON.parse(text) as Partial<RuntimeConfig>;
		if (typeof config.baseUrl === "string" && typeof config.disableFavicons === "boolean") {
			return { baseUrl: config.baseUrl, disableFavicons: config.disableFavicons };
		}
	} catch {
		return undefined;
	}

	return undefined;
};
export const runtimeConfig = readConfig();

export const basePath = runtimeConfig ? new URL(runtimeConfig.baseUrl).pathname.replace(/\/$/, "") : "";

export const appPath = (path: string) => `${basePath}${path}`;

/**
 * Read the project id from a `/p/<id>` path, ignoring a trailing screen segment
 * such as `/utm-tags`. Returns `undefined` for any other path.
 */
export const projectIdFromPath = (pathname: string, path: string = basePath): string | undefined => {
	let local = pathname;
	if (path) {
		if (local !== path && !local.startsWith(`${path}/`)) return undefined;
		local = local.slice(path.length);
	}

	const [root, id] = local.split("/").filter(Boolean);
	if (root !== "p" || !id) return undefined;

	try {
		return decodeURIComponent(id) || undefined;
	} catch {
		return undefined;
	}
};

/** The project id of the page currently open, or `undefined` when there is none. */
export const getDashboardProjectId = () =>
	typeof window === "undefined" ? undefined : projectIdFromPath(window.location.pathname);
