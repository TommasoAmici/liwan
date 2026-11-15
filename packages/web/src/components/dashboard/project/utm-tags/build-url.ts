export type UtmValues = {
	source?: string | null;
	medium?: string | null;
	campaign?: string | null;
	content?: string | null;
	term?: string | null;
};

/** The five UTM parameters this screen owns, in the order they are applied. */
const managedParameters = [
	["source", "utm_source"],
	["medium", "utm_medium"],
	["campaign", "utm_campaign"],
	["content", "utm_content"],
	["term", "utm_term"],
] as const;

/**
 * Build the tagged URL for a saved tag: remove the five managed parameters from
 * the base URL, then apply the non-empty values in source, medium, campaign,
 * content, term order. Other query parameters and the fragment are kept.
 *
 * Returns `null` when the base URL is not an absolute http or https URL.
 */
export const buildUtmUrl = (baseUrl: string, values: UtmValues): string | null => {
	let url: URL;
	try {
		url = new URL(baseUrl.trim());
	} catch {
		return null;
	}
	if ((url.protocol !== "http:" && url.protocol !== "https:") || !url.host) return null;

	for (const [, parameter] of managedParameters) url.searchParams.delete(parameter);
	for (const [field, parameter] of managedParameters) {
		const value = values[field]?.trim();
		if (value) url.searchParams.set(parameter, value);
	}

	return url.toString();
};
