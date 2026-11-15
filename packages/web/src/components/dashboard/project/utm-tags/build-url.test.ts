import { describe, expect, it } from "bun:test";

import { buildUtmUrl } from "./build-url";

describe("buildUtmUrl", () => {
	it("applies the values in source, medium, campaign, content, term order", () => {
		const url = buildUtmUrl("https://example.com/landing", {
			source: "google",
			medium: "cpc",
			campaign: "summer_sale",
			content: "banner",
			term: "shoes",
		});
		expect(url).toBe(
			"https://example.com/landing?utm_source=google&utm_medium=cpc&utm_campaign=summer_sale&utm_content=banner&utm_term=shoes",
		);
	});

	it("keeps other query parameters and the fragment", () => {
		const url = buildUtmUrl("https://example.com/page?ref=newsletter&page=2#pricing", {
			source: "newsletter",
		});
		expect(url).toBe("https://example.com/page?ref=newsletter&page=2&utm_source=newsletter#pricing");
	});

	it("replaces existing UTM parameters instead of duplicating them", () => {
		const url = buildUtmUrl("https://example.com/?utm_source=old&utm_campaign=old&utm_content=old", {
			source: "google",
			medium: "cpc",
		});
		expect(url).toBe("https://example.com/?utm_source=google&utm_medium=cpc");
	});

	it("trims values and skips empty ones", () => {
		const url = buildUtmUrl("https://example.com/", {
			source: "  google  ",
			medium: "   ",
			campaign: "",
			content: null,
			term: undefined,
		});
		expect(url).toBe("https://example.com/?utm_source=google");
	});

	it("returns the base URL when no value is set", () => {
		expect(buildUtmUrl("https://example.com/landing?ref=news", {})).toBe("https://example.com/landing?ref=news");
		expect(buildUtmUrl("http://localhost:3000/landing", {})).toBe("http://localhost:3000/landing");
	});

	it("returns null for a URL that is not absolute http or https", () => {
		expect(buildUtmUrl("example.com/landing", { source: "google" })).toBeNull();
		expect(buildUtmUrl("/landing", { source: "google" })).toBeNull();
		expect(buildUtmUrl("ftp://example.com/landing", { source: "google" })).toBeNull();
		expect(buildUtmUrl("javascript:alert(1)", { source: "google" })).toBeNull();
		expect(buildUtmUrl("", { source: "google" })).toBeNull();
	});
});
