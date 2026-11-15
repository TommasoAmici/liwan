import { describe, expect, it } from "bun:test";

import { projectIdFromPath } from "./config";

describe("projectIdFromPath", () => {
	it("reads the id from a project path", () => {
		expect(projectIdFromPath("/p/liwan.dev", "")).toBe("liwan.dev");
		expect(projectIdFromPath("/p/public-project/", "")).toBe("public-project");
		expect(projectIdFromPath("/p/my%20project", "")).toBe("my project");
	});

	it("ignores a trailing screen segment", () => {
		expect(projectIdFromPath("/p/liwan.dev/utm-tags", "")).toBe("liwan.dev");
		expect(projectIdFromPath("/p/liwan.dev/utm-tags/", "")).toBe("liwan.dev");
		expect(projectIdFromPath("/p/liwan.dev/funnels", "")).toBe("liwan.dev");
	});

	it("strips a configured base path", () => {
		expect(projectIdFromPath("/analytics/p/public-project/utm-tags", "/analytics")).toBe("public-project");
		expect(projectIdFromPath("/analytics/p/public-project", "/analytics")).toBe("public-project");
		expect(projectIdFromPath("/p/public-project", "/analytics")).toBeUndefined();
		expect(projectIdFromPath("/analytics-other/p/public-project", "/analytics")).toBeUndefined();
	});

	it("returns undefined for paths that are not a project", () => {
		expect(projectIdFromPath("/", "")).toBeUndefined();
		expect(projectIdFromPath("/p", "")).toBeUndefined();
		expect(projectIdFromPath("/p/", "")).toBeUndefined();
		expect(projectIdFromPath("/settings/projects/public-project", "")).toBeUndefined();
		expect(projectIdFromPath("/projects", "")).toBeUndefined();
	});
});
