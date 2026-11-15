import { describe, expect, it } from "bun:test";

import type { DimensionFilter } from "@/constants";
import { parseReportFilters, serializeReportFilters } from "./report-filters";

const equal = (dimension: DimensionFilter["dimension"], value: string): DimensionFilter => ({
	dimension,
	filterType: "equal",
	value,
});

const search = (filters: unknown) => `?filters=${encodeURIComponent(JSON.stringify(filters))}`;

describe("report filters in the URL", () => {
	it("round-trips filters through the search parameter", () => {
		const filters: DimensionFilter[] = [
			equal("utm_source", "newsletter"),
			{ dimension: "property", key: "plan", filterType: "equal", value: "pro" },
			{ dimension: "mobile", filterType: "is_true" },
		];
		const serialized = serializeReportFilters(filters);
		expect(serialized).not.toBeNull();
		expect(parseReportFilters(`?filters=${encodeURIComponent(serialized as string)}`)).toEqual(filters);
	});

	it("serializes an empty list to null", () => {
		expect(serializeReportFilters([])).toBeNull();
	});

	it("ignores invalid JSON and a missing parameter", () => {
		expect(parseReportFilters("?filters=%7Bnot-json")).toEqual([]);
		expect(parseReportFilters("?filters=null")).toEqual([]);
		expect(parseReportFilters("")).toEqual([]);
		expect(parseReportFilters("?other=1")).toEqual([]);
	});

	it("drops unknown dimensions and unknown filter types", () => {
		expect(parseReportFilters(search([equal("unknown" as DimensionFilter["dimension"], "x")]))).toEqual([]);
		expect(
			parseReportFilters(
				search([{ dimension: "path", filterType: "matches", value: "/pricing" }, equal("path", "/pricing")]),
			),
		).toEqual([equal("path", "/pricing")]);
	});

	it("requires a value for value-based filters and drops boolean types elsewhere", () => {
		expect(parseReportFilters(search([{ dimension: "path", filterType: "equal" }]))).toEqual([]);
		expect(parseReportFilters(search([{ dimension: "path", filterType: "contains", value: "" }]))).toEqual([]);
		expect(parseReportFilters(search([{ dimension: "path", filterType: "is_true" }]))).toEqual([]);
		expect(parseReportFilters(search([{ dimension: "mobile", filterType: "equal", value: "x" }]))).toEqual([]);
		expect(parseReportFilters(search([equal("path", "/pricing")]))).toEqual([equal("path", "/pricing")]);
	});

	it("requires a key for property filters and strips keys from other dimensions", () => {
		expect(parseReportFilters(search([{ dimension: "property", filterType: "equal", value: "pro" }]))).toEqual([]);
		expect(
			parseReportFilters(search([{ dimension: "property", key: "  ", filterType: "equal", value: "pro" }])),
		).toEqual([]);
		expect(parseReportFilters(search([{ dimension: "path", key: "plan", filterType: "equal", value: "/" }]))).toEqual([
			{ dimension: "path", filterType: "equal", inversed: undefined, strict: undefined, value: "/" },
		]);
		// A key does not excuse a missing value.
		expect(parseReportFilters(search([{ dimension: "path", key: "plan", filterType: "equal" }]))).toEqual([]);
	});

	it("accepts custom-event style null filters", () => {
		expect(parseReportFilters(search([{ dimension: "city", filterType: "is_null", inversed: true }]))).toEqual([
			{ dimension: "city", filterType: "is_null", inversed: true },
		]);
	});

	it("keeps at most 20 filters", () => {
		const many = Array.from({ length: 25 }, (_, index) => equal("path", `/page-${index}`));
		const parsed = parseReportFilters(search(many));
		expect(parsed).toHaveLength(20);
		expect(parsed[19]).toEqual(equal("path", "/page-19"));
		expect(parseReportFilters(search([...many, ...many]))).toHaveLength(20);

		const serialized = serializeReportFilters(many);
		expect(JSON.parse(serialized as string)).toHaveLength(20);
	});
});
