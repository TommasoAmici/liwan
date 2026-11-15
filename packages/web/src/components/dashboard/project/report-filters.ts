import type { Dimension, DimensionFilter, FilterType } from "@/constants";
import { dimensions, filterNames } from "@/constants";

const MAX_REPORT_FILTERS = 20;

const booleanFilterTypes = ["is_true", "is_false"] as const;

const isFilterType = (value: unknown): value is FilterType =>
	typeof value === "string" && Object.hasOwn(filterNames, value);

const isBooleanFilterType = (value: FilterType) => (booleanFilterTypes as readonly FilterType[]).includes(value);

const isDimension = (value: unknown): value is Dimension =>
	typeof value === "string" && (dimensions as readonly string[]).includes(value);

const isOptionalBoolean = (value: unknown) => value === undefined || typeof value === "boolean";

/**
 * Parse one entry of the `filters` parameter, returning a filter the report
 * accepts or `undefined` when the entry is not usable.
 */
const parseFilter = (entry: unknown): DimensionFilter | undefined => {
	if (!entry || typeof entry !== "object") return undefined;

	const candidate = entry as Record<string, unknown>;
	const { dimension, filterType, value } = candidate;
	if (!isDimension(dimension) && dimension !== "property") return undefined;
	if (!isFilterType(filterType)) return undefined;
	if (!isOptionalBoolean(candidate.inversed) || !isOptionalBoolean(candidate.strict)) return undefined;

	// The report rejects a key on any dimension except property, so drop it
	// rather than the filter.
	const key = dimension === "property" ? candidate.key : undefined;
	if (dimension === "property" && (typeof key !== "string" || key.trim() === "")) return undefined;

	const inversed = candidate.inversed as boolean | undefined;
	if (dimension === "mobile") {
		return isBooleanFilterType(filterType) ? { dimension, filterType, inversed } : undefined;
	}
	if (isBooleanFilterType(filterType)) return undefined;
	if (filterType === "is_null") return { dimension, key: key as string | undefined, filterType, inversed };

	if (typeof value !== "string" || value === "") return undefined;
	return {
		dimension,
		key: key as string | undefined,
		filterType,
		inversed,
		strict: candidate.strict as boolean | undefined,
		value,
	};
};

/**
 * Read the `filters` search parameter, dropping entries the report would reject.
 * Invalid JSON is an empty list, and at most 20 filters are kept.
 */
export const parseReportFilters = (search: string): DimensionFilter[] => {
	const raw = new URLSearchParams(search).get("filters");
	if (!raw) return [];

	let parsed: unknown;
	try {
		parsed = JSON.parse(raw);
	} catch {
		return [];
	}
	if (!Array.isArray(parsed)) return [];

	const filters: DimensionFilter[] = [];
	for (const entry of parsed) {
		const filter = parseFilter(entry);
		if (filter) filters.push(filter);
		if (filters.length === MAX_REPORT_FILTERS) break;
	}
	return filters;
};

/**
 * Serialize filters for the `filters` search parameter. Returns `null` for an
 * empty list so the caller can remove the parameter. The value is plain JSON;
 * `URLSearchParams` handles the URL encoding.
 */
export const serializeReportFilters = (filters: DimensionFilter[]): string | null =>
	filters.length === 0 ? null : JSON.stringify(filters.slice(0, MAX_REPORT_FILTERS));
